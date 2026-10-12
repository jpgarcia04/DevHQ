# Phases B to H of the upgrade rehearsal (.github/workflows/upgrade-rehearsal.yml), one per call,
# on a throwaway Windows runner with synthetic data only: install the old build, give it an
# inventory, install the new build over it, check the data before and after opening it, check
# that an unpacked build keeps development data apart, see what the silent installer does to
# running DevHQ processes, and go back to the old build.
#
# Each phase records its checks in report/phase-<phase>.json under $env:REHEARSAL_DIR and fails
# when one fails; NOTE lines record what was observed without judging it. Physical checks compare
# SHA-256 hashes of every file in the data folders; logical checks compare SQLite snapshots
# (rehearsal_inventory.py), made on copies so the inventory under test is never opened by them,
# and ask DevHQ's own agent gate. Never run this on a computer whose DevHQ data matters: it
# installs DevHQ and writes into %APPDATA% and %LOCALAPPDATA%.
param([Parameter(Mandatory)] [ValidateSet('Inputs', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'Report')] [string] $Phase)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'upgrade-rehearsal-common.ps1')

if ($env:GITHUB_ACTIONS -ne 'true' -or -not $env:REHEARSAL_DIR) {
  throw 'The upgrade rehearsal runs only on a throwaway GitHub Actions runner.'
}
$Work = $env:REHEARSAL_DIR
$Inputs = Join-Path $Work 'inputs'
$State = Join-Path $Work 'state'
$ReportDir = Join-Path $Work 'report'
New-Item -ItemType Directory -Force $State, $ReportDir | Out-Null
$Python = Join-Path $PSScriptRoot 'rehearsal_inventory.py'

$Roaming = Join-Path $env:APPDATA 'com.devhq.desktop'
$Local = Join-Path $env:LOCALAPPDATA 'com.devhq.desktop'
$RoamingDev = "$Roaming.dev"
$LocalDev = "$Local.dev"
$Inventory = Join-Path $Roaming 'inventory.sqlite3'
$DevInventory = Join-Path $RoamingDev 'inventory.sqlite3'
$InstallDir = Join-Path $env:LOCALAPPDATA 'DevHQ'
$Installed = Join-Path $InstallDir 'devhq.exe'
$Unpacked = Join-Path $Work 'unpacked\devhq.exe'
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DevHQ'
# What sits beside the everyday installation it rehearses: an older inventory set aside under
# another name in both folders, and an archive of it in the profile.
$SetAside = @((Join-Path $env:APPDATA 'com.devhq.desktop.v6-apartado'), (Join-Path $env:LOCALAPPDATA 'com.devhq.desktop.v6-apartado'))
$Archive = Join-Path $env:USERPROFILE 'DevHQ-archivo\v6-original'
$Witnesses = @((Join-Path $Roaming 'rehearsal-witness.txt'), (Join-Path $Local 'rehearsal-witness.txt'),
  (Join-Path $SetAside[0] 'rehearsal-witness.txt'), (Join-Path $SetAside[1] 'rehearsal-witness.txt'),
  (Join-Path $Archive 'rehearsal-witness.txt'))
$InstallWitness = Join-Path $InstallDir 'rehearsal-witness.txt'
$Marker = 'PRUEBA-INSTALACION-2026-10'
$Server = 'PRUEBA-SERVIDOR'
$Workstation = 'PRUEBA-ESTACION'

$Manifest = if (Test-Path (Join-Path $Inputs 'build-manifest.json')) {
  Get-Content -Raw (Join-Path $Inputs 'build-manifest.json') | ConvertFrom-Json -AsHashtable -Depth 30
}

# --- Recording -------------------------------------------------------------------------------

$script:Results = [Collections.Generic.List[object]]::new()

function Check([string] $Name, [bool] $Ok, $Detail = '') {
  $script:Results.Add([ordered]@{ check = $Name; ok = $Ok; detail = "$Detail" })
  Write-Host "$(if ($Ok) { 'PASS' } else { 'FAIL' })  $Name$(if ("$Detail") { " :: $Detail" })"
}

function Note([string] $Name, $Detail) {
  $script:Results.Add([ordered]@{ check = $Name; ok = $null; detail = "$Detail" })
  Write-Host "NOTE  $Name :: $Detail"
}

function Save-State($Value, [string] $Name) { Save-Json $Value (Join-Path $State "$Name.json") }

function Read-State([string] $Name) {
  Get-Content -Raw (Join-Path $State "$Name.json") | ConvertFrom-Json -AsHashtable -Depth 30
}

# --- Files -----------------------------------------------------------------------------------

# Every file under $Root with its SHA-256 and length, by relative path; $null when $Root does not
# exist. $Skip lists relative prefixes left out (WebView2's profile, which its own processes
# rewrite whenever the window opens).
function Get-Tree([string] $Root, [string[]] $Skip = @()) {
  if (-not (Test-Path -LiteralPath $Root)) { return $null }
  $tree = [ordered]@{}
  foreach ($file in Get-ChildItem -LiteralPath $Root -Recurse -Force -File | Sort-Object FullName) {
    $relative = $file.FullName.Substring($Root.Length + 1)
    if ($Skip | Where-Object { $relative -like "$_*" }) { continue }
    $tree[$relative] = "$(Get-Sha256 $file.FullName) $($file.Length)"
  }
  $tree
}

# The installed DevHQ's data and what sits beside it, WebView2's profile apart.
function Get-DataTrees {
  $trees = [ordered]@{}
  $trees[$Roaming] = Get-Tree $Roaming
  $trees[$Local] = Get-Tree $Local @('EBWebView\')
  foreach ($folder in $SetAside + $Archive) { $trees[$folder] = Get-Tree $folder }
  $trees
}

function Get-DevTrees {
  $trees = [ordered]@{}
  $trees[$RoamingDev] = Get-Tree $RoamingDev
  $trees[$LocalDev] = Get-Tree $LocalDev @('EBWebView\')
  $trees
}

function Compare-Trees($Before, $After) {
  $found = [Collections.Generic.List[string]]::new()
  foreach ($root in @(@($Before.Keys) + @($After.Keys) | Sort-Object -Unique)) {
    $old = $Before[$root]
    $new = $After[$root]
    if ($null -eq $old -and $null -eq $new) { continue }
    if ($null -eq $old) { $found.Add("$root appeared"); continue }
    if ($null -eq $new) { $found.Add("$root disappeared"); continue }
    foreach ($file in @(@($old.Keys) + @($new.Keys) | Sort-Object -Unique)) {
      if ($null -eq $old[$file]) { $found.Add("$root\$file appeared") }
      elseif ($null -eq $new[$file]) { $found.Add("$root\$file disappeared") }
      elseif ($old[$file] -ne $new[$file]) { $found.Add("$root\$file changed") }
    }
  }
  , $found.ToArray()
}

# The inventory's own files: the database, its write-ahead log and shared memory, and the lock
# the window holds. Opening DevHQ may legitimately change them; their contents are checked
# logically instead.
function Test-InventoryFile([string] $Difference) { $Difference -match '\\inventory\.sqlite3(-wal|-shm|\.window-lock)? (changed|appeared|disappeared)$' }

function Write-Witnesses {
  foreach ($witness in $Witnesses + $InstallWitness) {
    New-Item -ItemType Directory -Force (Split-Path $witness) | Out-Null
    [IO.File]::WriteAllText($witness, "DevHQ upgrade rehearsal witness $([guid]::NewGuid())`n")
  }
}

function Test-NoDevFolders([string] $When) {
  $found = @($RoamingDev, $LocalDev | Where-Object { Test-Path $_ })
  Check "No development data folder exists $When" ($found.Count -eq 0) ($found -join ', ')
}

# --- SQLite ----------------------------------------------------------------------------------

function Save-Inventory([string] $Name, [string] $Database = $Inventory) {
  $out = Join-Path $State "inventory-$Name.json"
  $line = & python -I $Python snapshot --database $Database --out $out
  if ($LASTEXITCODE -ne 0) { throw "The SQLite snapshot $Name failed: $line" }
  $snapshot = Get-Content -Raw $out | ConvertFrom-Json -AsHashtable -Depth 30
  $sound = $snapshot.userVersion -eq 12 -and ($snapshot.integrity -join ',') -eq 'ok' -and $snapshot.foreignKeyViolations -eq 0
  Check "Inventory snapshot '$Name': schema version 12, integrity ok, no foreign key violation" $sound $line
  $snapshot
}

function Compare-Inventory([string] $Before, [string] $After, [string] $What) {
  $lines = @(& python -I $Python compare (Join-Path $State "inventory-$Before.json") (Join-Path $State "inventory-$After.json"))
  Check $What ($LASTEXITCODE -eq 0) ("'$Before' vs '$After': " + ($lines -join '; '))
}

function Set-Access([string] $Mode) {
  Assert-NoDevHQ "before setting the gate's access to $Mode"
  $line = & python -I $Python set-access --database $Inventory --mode $Mode
  Check "The gate's access is set to $Mode with Settings' own statement, DevHQ closed" ($LASTEXITCODE -eq 0) $line
}

# --- Processes, installer, window, gate ------------------------------------------------------

function Get-DevHQProcesses {
  @(Get-CimInstance Win32_Process -Filter "Name = 'devhq.exe'" | ForEach-Object {
      [ordered]@{ id = [int] $_.ProcessId; path = $_.ExecutablePath; commandLine = $_.CommandLine }
    })
}

function Assert-NoDevHQ([string] $When) {
  $running = @(Get-DevHQProcesses)
  Check "No DevHQ process runs $When" ($running.Count -eq 0) (($running | ForEach-Object { "$($_.id): $($_.commandLine)" }) -join '; ')
}

function Install-DevHQ([string] $Which) {
  $installer = Join-Path $Inputs $Manifest.builds[$Which].installer.file
  $hash = Get-Sha256 $installer
  Check "The $Which installer is the one the build described" ($hash -eq $Manifest.builds[$Which].installer.sha256) "$(Split-Path -Leaf $installer) $hash"
  $watch = [Diagnostics.Stopwatch]::StartNew()
  $process = Start-Process -FilePath $installer -ArgumentList '/S' -PassThru
  if (-not $process.WaitForExit(300000)) {
    Stop-Process -Id $process.Id -Force
    throw "The $Which installer did not finish within 5 minutes"
  }
  $seconds = [math]::Round($watch.Elapsed.TotalSeconds, 1)
  Check "The $Which installer, run with /S, ends with exit code 0" ($process.ExitCode -eq 0) "exit code $($process.ExitCode) after $seconds s"
}

function Get-UninstallEntry {
  if (-not (Test-Path $UninstallKey)) { return $null }
  $entry = Get-ItemProperty $UninstallKey
  [ordered]@{
    DisplayName = $entry.DisplayName; DisplayVersion = $entry.DisplayVersion; Publisher = $entry.Publisher
    InstallLocation = $entry.InstallLocation; UninstallString = $entry.UninstallString
  }
}

function Test-Installation([string] $Which) {
  $expected = if ($Manifest.builds[$Which].packedInInstaller) { $Manifest.builds[$Which].packedInInstaller.sha256 } else { $Manifest.builds[$Which].packedPredicted }
  $actual = if (Test-Path $Installed) { Get-Sha256 $Installed }
  Check "The installed executable is the one the $Which installer holds" ($actual -eq $expected) "$Installed $actual, expected $expected"
  Save-State ([ordered]@{ path = $Installed; sha256 = $actual }) "installed-$Which-$Phase"
  if ($actual) {
    $markers = (Get-UnpackedMarkers $Installed).Count
    Check 'The installed executable is packed (no unpatched bundle marker): it keeps com.devhq.desktop' ($markers -eq 0) "unpatched markers: $markers"
  }
  $entry = Get-UninstallEntry
  Check 'Windows lists DevHQ 0.1.0 among the installed programs, in the install folder' ($entry -and $entry.DisplayVersion -eq '0.1.0' -and $entry.InstallLocation -and ($entry.InstallLocation.Trim('"').TrimEnd('\') -eq $InstallDir)) ($entry | ConvertTo-Json -Compress)
  Check 'The uninstaller is in the install folder' (Test-Path (Join-Path $InstallDir 'uninstall.exe')) $InstallDir
  Note 'The install folder holds' ((Get-ChildItem -Force $InstallDir | ForEach-Object { $_.Name }) -join ', ')
  Start-Sleep -Seconds 3
  Assert-NoDevHQ 'after the installer ended (it started none)'
}

Add-Type -Namespace Rehearsal -Name User32 -MemberDefinition '[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr w, IntPtr l);'

function Start-Window([string] $Exe) {
  $process = Start-Process -FilePath $Exe -PassThru
  $deadline = (Get-Date).AddMinutes(1)
  do { Start-Sleep -Milliseconds 500; $process.Refresh() }
  until ($process.HasExited -or $process.MainWindowHandle -ne [IntPtr]::Zero -or (Get-Date) -gt $deadline)
  if ($process.HasExited) { throw "$Exe exited at start with code $($process.ExitCode)" }
  if ($process.MainWindowHandle -eq [IntPtr]::Zero) { throw "$Exe showed no window within a minute" }
  # Time for WebView2 to start and the window to load the inventory.
  Start-Sleep -Seconds 8
  $process.Refresh()
  if ($process.HasExited) { throw "$Exe exited after showing its window, with code $($process.ExitCode)" }
  $process
}

function Close-Window($Process) {
  [Rehearsal.User32]::PostMessageW($Process.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
  if (-not $Process.WaitForExit(30000)) { throw 'DevHQ did not end within 30 s of its window closing' }
  $Process.ExitCode
}

# The WebView2 profiles the running browser processes name: the window's local folder, for real.
function Get-WebViewProfiles {
  @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" | ForEach-Object {
      if ($_.CommandLine -match '--user-data-dir="?([^"]+?)"?(\s|$)') { $Matches[1] }
    } | Sort-Object -Unique)
}

# Opens $Exe's window, asks $Gate (an executable whose gate opens $Database) whether a window is
# open on that inventory while it is, and closes it as a person does.
function Open-AndClose([string] $Exe, [string] $Title, [string] $What, [string] $Gate, [string] $Database, [string] $WebViewProfile) {
  $process = Start-Window $Exe
  $title = $process.MainWindowTitle
  Check "$What opens a window titled '$Title'" ($title -ceq $Title) "title '$title'"
  # The first opening creates the inventory once the page asks for it.
  $deadline = (Get-Date).AddSeconds(30)
  while (-not (Test-Path $Database) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
  $status = (Invoke-Gate $Gate @((Call 'status')) "while $What is open")[0].structured
  Check "While $What is open, the gate sees a window open on $Database" ($status.windowOpen -eq $true -and $status.database.database -eq $Database) "windowOpen $($status.windowOpen), database $($status.database.database)"
  $profiles = @(Get-WebViewProfiles)
  if ($profiles.Count) {
    Check "$What runs WebView2 with its profile in $WebViewProfile" (@($profiles | Where-Object { $_.TrimEnd('\') -eq $WebViewProfile }).Count -eq 1) ('profiles named: ' + ($profiles -join '; '))
  } else {
    Note "WebView2's command lines name no profile while $What is open" 'its folder is checked instead'
  }
  Check "$What has WebView2's profile in $WebViewProfile" (Test-Path $WebViewProfile) ''
  $code = Close-Window $process
  Check "$What ends when its window is closed" ($code -eq 0) "exit code $code"
  Start-Sleep -Seconds 3
  Assert-NoDevHQ "after $What closed"
}

function Start-Gate([string] $Exe) {
  $start = [Diagnostics.ProcessStartInfo]::new($Exe, '--mcp')
  $start.UseShellExecute = $false
  $start.RedirectStandardInput = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
  $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
  $gate = [Diagnostics.Process]::Start($start)
  $initialize = @{ jsonrpc = '2.0'; id = 0; method = 'initialize'; params = @{ protocolVersion = '2025-06-18'; clientInfo = @{ name = 'upgrade-rehearsal'; version = '1' } } }
  $gate.StandardInput.WriteLine(($initialize | ConvertTo-Json -Depth 10 -Compress))
  $gate
}

function Call([string] $Tool, $Arguments = @{}) { [ordered]@{ tool = $Tool; arguments = $Arguments } }

# Calls the tools in $Calls (made by Call) through $Exe's agent gate, as an MCP
# client does, and keeps the exchange as evidence. Returns one result per call.
$script:GateLog = 0
function Invoke-Gate([string] $Exe, [object[]] $Calls, [string] $Why) {
  $gate = Start-Gate $Exe
  $out = $gate.StandardOutput.ReadToEndAsync()
  $err = $gate.StandardError.ReadToEndAsync()
  $id = 0
  foreach ($call in $Calls) {
    $id += 1
    $request = @{ jsonrpc = '2.0'; id = $id; method = 'tools/call'; params = @{ name = $call.tool; arguments = $call.arguments } }
    $gate.StandardInput.WriteLine(($request | ConvertTo-Json -Depth 20 -Compress))
  }
  $gate.StandardInput.Close()
  if (-not $gate.WaitForExit(60000)) { $gate.Kill(); throw "The gate of $Exe did not end within a minute of its input closing" }
  $replies = @($out.Result -split "`n" | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json -AsHashtable -Depth 50 })
  if ($replies.Count -ne $Calls.Count + 1 -or $gate.ExitCode -ne 0) {
    throw "The gate of $Exe answered $($replies.Count) of $($Calls.Count + 1) requests, exit code $($gate.ExitCode): $($out.Result) $($err.Result)"
  }
  $results = for ($i = 1; $i -le $Calls.Count; $i++) {
    $reply = $replies | Where-Object { $_.id -eq $i }
    [ordered]@{
      tool = $Calls[$i - 1].tool
      arguments = $Calls[$i - 1].arguments
      isError = [bool] $reply.result.isError
      text = $reply.result.content[0].text
      structured = $reply.result.structuredContent
    }
  }
  $script:GateLog += 1
  Save-Json ([ordered]@{ executable = $Exe; why = $Why; stderr = $err.Result; calls = @($results) }) (Join-Path $ReportDir ("gate-$Phase-{0:d2}.json" -f $script:GateLog))
  , @($results)
}

function Test-Denied($Result) {
  Check "The gate refuses $($Result.tool) in Read only" ($Result.isError -and $Result.text -like '*(access_denied)') $Result.text
}

# The everyday installation's gate status, as the agent gate gives it to Claude Code.
function Test-InstalledStatus($Status, [string] $Build) {
  Check "The $Build build's gate opens $Inventory without --database" ($Status.database.database -eq $Inventory -and $null -eq $Status.database.storedAt) "database $($Status.database.database), storedAt $($Status.database.storedAt)"
  Check "The $Build build's gate finds schema version 12 and Read only" ($Status.exists -eq $true -and $Status.schemaVersion -eq 12 -and $Status.access -eq 'read') "exists $($Status.exists), schema $($Status.schemaVersion), access $($Status.access)"
  Check "The $Build build's gate counts 1 application and 2 resources" ($Status.counts.applications -eq 1 -and $Status.counts.resources -eq 2) ($Status.counts | ConvertTo-Json -Compress)
  $ids = Read-State 'ids'
  Check "The $Build build's gate knows this computer is $Workstation (this-workstation.json in $Local)" ($Status.thisWorkstation.resourceId -eq $ids.workstation -and $Status.thisWorkstation.file -eq (Join-Path $Local 'this-workstation.json')) ($Status.thisWorkstation | ConvertTo-Json -Compress)
  Check "The $Build build's gate does not say it keeps development data" (-not ($Status.notes -match 'development data')) ($Status.notes -join ' | ')
}

# The synthetic inventory, through list_systems and get_system: the marker, its relations and its
# records, with the ids phase C recorded.
function Test-Contents([string] $Exe, [string] $Build) {
  $ids = Read-State 'ids'
  $results = Invoke-Gate $Exe @(
    (Call 'list_systems' @{}),
    (Call 'get_system' @{ type = 'application'; name = $Marker }),
    (Call 'get_system' @{ type = 'resource'; name = $Server }),
    (Call 'attention' @{})
  ) "what the $Build build reads"
  foreach ($result in $results) { Check "The $Build build's gate answers $($result.tool)" (-not $result.isError) $result.text.Substring(0, [math]::Min(200, $result.text.Length)) }
  $list = $results[0].structured
  $application = @($list.applications | Where-Object { $_.system.name -eq $Marker })
  Check "$Marker is there, with the same id" ($application.Count -eq 1 -and $application[0].system.id -eq $ids.application) "found $($application.Count), id $($application[0].system.id)"
  $component = @($application[0].components)
  Check "Its component still runs on $Server, by id" ($component.Count -eq 1 -and $component[0].runsOn.id -eq $ids.server) ($component | ConvertTo-Json -Compress -Depth 5)
  $references = @($application[0].references)
  Check 'Its repository and its local copy on the workstation are still declared' ($references.Count -eq 2) ($references | ConvertTo-Json -Compress -Depth 6)
  # Opening the incident logged an event of its own ("Incident opened"), beside the deployment.
  $detail = $results[1].structured.system
  $events = @($detail.events | ForEach-Object { $_.id } | Sort-Object)
  $expected = @($ids.event, $ids.incidentEvent | Sort-Object)
  Check 'Its deployment event, its incident and the event that opened it are still there, same ids' ((($events -join ',') -eq ($expected -join ',')) -and @($detail.incidents).Count -eq 1 -and $detail.incidents[0].id -eq $ids.incident) "events $($events -join ', '); incidents $(@($detail.incidents | ForEach-Object { $_.id }) -join ', ')"
  $serverDetail = $results[2].structured.system
  Check "$Server keeps its observation, and $Marker still uses it" (@($serverDetail.observations).Count -eq 1 -and $serverDetail.observations[0].id -eq $ids.observation -and @($serverDetail.usedBy).Count -ge 1) "observations $(@($serverDetail.observations).Count), usedBy $($serverDetail.usedBy | ConvertTo-Json -Compress -Depth 5)"
  $results
}

function Test-WritesRefused([string] $Exe, [string] $Build, [string] $Before) {
  $results = Invoke-Gate $Exe @(
    (Call 'record_event' @{ type = 'application'; name = $Marker; kind = 'note'; outcome = 'info'; title = 'Must be refused' }),
    (Call 'save_resource' @{ name = 'MUST-BE-REFUSED'; kind = 'server' }),
    (Call 'backup' @{})
  ) "writes the $Build build must refuse in Read only"
  foreach ($result in $results) { Test-Denied $result }
  Save-Inventory "$Before-after-refusals" | Out-Null
  Compare-Inventory $Before "$Before-after-refusals" "The refused writes left every table as it was ($Build build)"
}

# --- Phases ----------------------------------------------------------------------------------

# The downloaded builds are the ones described, and the runner holds no DevHQ yet.
function Invoke-PhaseInputs {
  foreach ($which in 'old', 'new') {
    $build = $Manifest.builds[$which]
    $expected = if ($which -eq 'old') { $env:OLD_COMMIT } else { $env:NEW_COMMIT }
    Check "The $which build comes from commit $expected" ($build.commit -eq $expected) $build.commit
    $installer = Join-Path $Inputs $build.installer.file
    $hash = Get-Sha256 $installer
    Check "The $which installer's SHA-256 is the build's" ($hash -eq $build.installer.sha256) "$($build.installer.file) $hash"
    if ($build.packedInInstaller) {
      Check "The executable the $which installer holds is its unpacked build with the marker patched" ($build.packedInInstaller.sha256 -eq $build.packedPredicted) "extracted $($build.packedInInstaller.sha256), predicted $($build.packedPredicted)"
    } else {
      Note "7-Zip did not extract the executable the $which installer holds" "installed hash only predicted: $($build.packedPredicted)"
    }
  }
  New-Item -ItemType Directory -Force (Split-Path $Unpacked) | Out-Null
  Copy-Item (Join-Path $Inputs $Manifest.builds.new.unpacked.file) $Unpacked
  $hash = Get-Sha256 $Unpacked
  Check "The new build's unpacked executable is the build's" ($hash -eq $Manifest.builds.new.unpacked.sha256) $hash
  Check 'It holds one unpatched bundle marker: it keeps development data' ((Get-UnpackedMarkers $Unpacked).Count -eq 1) ''
  $present = @($Roaming, $Local, $RoamingDev, $LocalDev, $InstallDir) + $SetAside + $Archive | Where-Object { Test-Path $_ }
  Check 'The runner holds no DevHQ data, installation or set-aside folder' ($present.Count -eq 0) ($present -join ', ')
  Check 'Windows lists no DevHQ installation' (-not (Test-Path $UninstallKey)) ''
  Assert-NoDevHQ 'before the rehearsal'
}

function Invoke-PhaseB {
  Install-DevHQ 'old'
  Test-Installation 'old'
  Check 'The installer created no data folder' (-not (Test-Path $Roaming) -and -not (Test-Path $Local)) ''
  Open-AndClose $Installed 'DevHQ' 'The installed old build' $Installed $Inventory (Join-Path $Local 'EBWebView')
  Check "The first opening created the inventory in $Roaming" (Test-Path $Inventory) $Inventory
  Check "and WebView2's profile in $Local" (Test-Path (Join-Path $Local 'EBWebView')) ''
  Check 'No write-ahead log stayed after the window closed' (-not (Test-Path "$Inventory-wal")) ''
  Test-NoDevFolders 'after the old build opened'
  $snapshot = Save-Inventory 'B'
  Check 'The new inventory is closed to agents (no agent_access row)' (@($snapshot.agentAccess).Count -eq 0) ($snapshot.agentAccess | ConvertTo-Json -Compress)
  $status = (Invoke-Gate $Installed @((Call 'status')) 'the old build after its first opening')[0].structured
  Check 'The old build gate opens the inventory the window created, schema 12, Off' ($status.database.database -eq $Inventory -and $status.schemaVersion -eq 12 -and $status.access -eq 'off') ($status | ConvertTo-Json -Compress -Depth 5)
}

function Invoke-PhaseC {
  # Read and write only to build the synthetic inventory through the gate's own use cases.
  Set-Access 'write'
  $results = Invoke-Gate $Installed @(
    (Call 'save_resource' @{ name = $Server; kind = 'server'; description = 'Synthetic server of the upgrade rehearsal'; operatingSystem = 'Windows Server' }),
    (Call 'save_resource' @{ name = $Workstation; kind = 'workstation'; description = 'Synthetic workstation of the upgrade rehearsal' }),
    (Call 'save_application' @{
        name = $Marker; lifecycle = 'testing'; version = '0.1.0'
        description = 'Synthetic marker of the upgrade rehearsal'
        components = @(@{ name = 'API de ensayo'; kind = 'backend'; runsOn = $Server })
        references = @(
          @{ kind = 'repository'; target = 'https://example.com/devhq-upgrade-rehearsal' },
          @{ kind = 'local_copy'; target = 'C:\DevHQ-ensayo\copia'; machine = $Workstation })
      }),
    (Call 'record_observation' @{ type = 'resource'; name = $Server; result = 'healthy'; summary = 'Synthetic observation of the upgrade rehearsal'; validForMinutes = 43200 }),
    (Call 'record_event' @{ type = 'application'; name = $Marker; kind = 'deployment'; outcome = 'success'; title = 'Synthetic deployment of the upgrade rehearsal'; detail = 'Recorded through the agent gate' }),
    (Call 'open_incident' @{ type = 'application'; name = $Marker; title = 'Synthetic incident of the upgrade rehearsal'; severity = 'info' }),
    (Call 'backup' @{})
  ) 'building the synthetic inventory with the old build'
  foreach ($result in $results) { Check "The old build's gate did $($result.tool)" (-not $result.isError) $result.text.Substring(0, [math]::Min(200, $result.text.Length)) }
  Set-Access 'read'

  # Which workstation this computer is, as the window's Settings > This computer writes it
  # (src-tauri/src/workstation.rs): the installed build has no DevTools port to drive that page.
  $list = (Invoke-Gate $Installed @((Call 'list_systems')) 'the ids the rehearsal follows')[0].structured
  $application = @($list.applications | Where-Object { $_.system.name -eq $Marker })[0]
  $ids = [ordered]@{
    application = $application.system.id
    server = @($list.resources | Where-Object { $_.system.name -eq $Server })[0].system.id
    workstation = @($list.resources | Where-Object { $_.system.name -eq $Workstation })[0].system.id
  }
  $machine = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Cryptography').MachineGuid
  $fingerprint = Get-Sha256OfBytes ([Text.Encoding]::UTF8.GetBytes("DevHQ this workstation`0$($machine.Trim())"))
  $choice = [ordered]@{ computers = @([ordered]@{ fingerprint = $fingerprint; choices = @([ordered]@{ resourceId = $ids.workstation; chosenAt = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'") }) }) }
  [IO.File]::WriteAllText((Join-Path $Local 'this-workstation.json'), ($choice | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))

  # The window opens and closes the inventory once, as a person leaves it.
  Open-AndClose $Installed 'DevHQ' 'The installed old build' $Installed $Inventory (Join-Path $Local 'EBWebView')
  $detail = Invoke-Gate $Installed @(
    (Call 'get_system' @{ type = 'application'; name = $Marker }),
    (Call 'get_system' @{ type = 'resource'; name = $Server })
  ) 'the record ids the rehearsal follows'
  $ids.event = @($detail[0].structured.system.events | Where-Object { $_.kind -eq 'deployment' })[0].id
  $ids.incidentEvent = @($detail[0].structured.system.events | Where-Object { $_.kind -eq 'incident' })[0].id
  $ids.incident = $detail[0].structured.system.incidents[0].id
  $ids.observation = $detail[1].structured.system.observations[0].id
  Save-State $ids 'ids'
  Note 'Synthetic ids' ($ids | ConvertTo-Json -Compress)

  $status = (Invoke-Gate $Installed @((Call 'status')) 'the old build before the upgrade')[0].structured
  Test-InstalledStatus $status 'old'
  Test-Contents $Installed 'old' | Out-Null
  $snapshot = Save-Inventory 'C'
  Check 'agent_access holds Read only' (@($snapshot.agentAccess).Count -eq 1 -and $snapshot.agentAccess[0][1] -eq 'read') ($snapshot.agentAccess | ConvertTo-Json -Compress)
  Note 'Rows per table' (($snapshot.tables.Keys | Sort-Object | ForEach-Object { "$_ $($snapshot.tables[$_].rows)" }) -join ', ')
  Test-WritesRefused $Installed 'old' 'C'
  Copy-Item (Join-Path $State 'inventory-C-after-refusals.json') (Join-Path $State 'inventory-baseline.json')
  $backups = @(Get-ChildItem (Join-Path $Roaming 'backups') -Filter 'inventory-*.sqlite3')
  Check 'The inventory has one SQLite backup' ($backups.Count -eq 1) ($backups.Name -join ', ')

  Write-Witnesses
  Assert-NoDevHQ 'before the snapshot of the files'
  $trees = Get-DataTrees
  Save-State $trees 'files-pre-upgrade'
  Save-State ([ordered]@{ profile = Get-Tree (Join-Path $Local 'EBWebView'); installWitness = Get-Sha256 $InstallWitness }) 'aside-pre-upgrade'
  Note 'Files before the upgrade' (($trees.Keys | ForEach-Object { "$_ : $(if ($null -eq $trees[$_]) { 'absent' } else { "$($trees[$_].Count) files" })" }) -join '; ')
}

function Invoke-PhaseD {
  Assert-NoDevHQ 'before the upgrade'
  Install-DevHQ 'new'
  # Physical, before anything opens the inventory.
  $differences = Compare-Trees (Read-State 'files-pre-upgrade') (Get-DataTrees)
  Check 'Every data file, backup, witness and set-aside file is byte for byte as before the upgrade' ($differences.Count -eq 0) ($differences -join '; ')
  $aside = Read-State 'aside-pre-upgrade'
  $profileChanges = Compare-Trees @{ p = $aside.profile } @{ p = (Get-Tree (Join-Path $Local 'EBWebView')) }
  Note "WebView2's profile after the installer" $(if ($profileChanges.Count) { $profileChanges -join '; ' } else { 'unchanged' })
  Note 'A foreign file in the install folder after the installer' $(if (Test-Path $InstallWitness) { if ((Get-Sha256 $InstallWitness) -eq $aside.installWitness) { 'kept, unchanged' } else { 'kept, changed' } } else { 'removed' })
  Test-Installation 'new'
  Check 'The new executable replaced the old one' ((Read-State 'installed-new-D').sha256 -ne (Read-State 'installed-old-B').sha256) ''
  Test-NoDevFolders 'after the upgrade'
  Check 'The inventory and its backup are still there (no uninstall ran)' ((Test-Path $Inventory) -and @(Get-ChildItem (Join-Path $Roaming 'backups') -Filter 'inventory-*.sqlite3').Count -eq 1) ''
}

function Invoke-PhaseE {
  Open-AndClose $Installed 'DevHQ' 'The installed new build' $Installed $Inventory (Join-Path $Local 'EBWebView')
  $differences = Compare-Trees (Read-State 'files-pre-upgrade') (Get-DataTrees)
  Note 'Files the first opening of the new build changed' $(if ($differences.Count) { $differences -join '; ' } else { 'none' })
  $unexpected = @($differences | Where-Object { -not (Test-InventoryFile $_) })
  Check "Backups, witnesses, this-workstation.json and the set-aside files are byte for byte as before" ($unexpected.Count -eq 0) ($unexpected -join '; ')
  Check 'No write-ahead log stayed after the window closed' (-not (Test-Path "$Inventory-wal")) ''
  Save-Inventory 'E' | Out-Null
  Compare-Inventory 'baseline' 'E' 'After the new build opened it, every table holds exactly the rows it held before the upgrade'
  $status = (Invoke-Gate $Installed @((Call 'status')) 'the new build after the upgrade')[0].structured
  Test-InstalledStatus $status 'new'
  Test-Contents $Installed 'new' | Out-Null
  Test-WritesRefused $Installed 'new' 'E'
  Test-NoDevFolders 'after the new build and its gate ran'
  Save-State (Get-DataTrees) 'files-post-E'
}

function Invoke-PhaseF {
  Assert-NoDevHQ 'before the unpacked build runs'
  $results = Invoke-Gate $Unpacked @((Call 'status'), (Call 'attention')) 'the unpacked new build before it has data'
  $status = $results[0].structured
  Check "The unpacked build's gate opens $DevInventory" ($status.database.database -eq $DevInventory -and $null -eq $status.database.storedAt) "database $($status.database.database)"
  Check 'It finds no inventory there, and says it keeps development data' ($status.exists -eq $false -and ($status.notes -match 'development data \(com\.devhq\.desktop\.dev\)')) "exists $($status.exists); $($status.notes -join ' | ')"
  Check 'It refuses attention instead of creating an inventory' ($results[1].isError) $results[1].text
  Test-NoDevFolders 'after the unpacked gate ran (it creates nothing)'

  Open-AndClose $Unpacked 'DevHQ (development data)' 'The unpacked new build' $Unpacked $DevInventory (Join-Path $LocalDev 'EBWebView')
  Check "Its window created its inventory in $RoamingDev" (Test-Path $DevInventory) ''
  Check "and its WebView2 profile in $LocalDev" (Test-Path (Join-Path $LocalDev 'EBWebView')) ''
  $dev = Save-Inventory 'F-dev' $DevInventory
  Check 'The development inventory is empty and closed to agents' (@($dev.agentAccess).Count -eq 0 -and $dev.tables.applications.rows -eq 0 -and $dev.tables.resources.rows -eq 0) ''
  $results = Invoke-Gate $Unpacked @((Call 'status')) 'the unpacked new build after its window ran'
  Check "The unpacked build's gate sees its own inventory, Off: the installed Read only does not apply" ($results[0].structured.exists -eq $true -and $results[0].structured.access -eq 'off') "access $($results[0].structured.access)"

  $differences = Compare-Trees (Read-State 'files-post-E') (Get-DataTrees)
  Check "The installed DevHQ's files are byte for byte as before the unpacked build ran" ($differences.Count -eq 0) ($differences -join '; ')
  Save-Inventory 'F' | Out-Null
  Compare-Inventory 'baseline' 'F' "The installed DevHQ's inventory is as before"
  $status = (Invoke-Gate $Installed @((Call 'status')) 'the installed new build after the unpacked one ran')[0].structured
  Check "The installed build's gate still opens its own inventory, Read only" ($status.database.database -eq $Inventory -and $status.access -eq 'read') "database $($status.database.database), access $($status.access)"
}

function Invoke-PhaseG {
  Assert-NoDevHQ 'before the processes this phase starts'
  Save-Inventory 'G0' | Out-Null
  Save-Inventory 'G0-dev' $DevInventory | Out-Null
  # What a person might have running: the installed window, the installed gate an MCP client
  # keeps open, and an unpacked build's gate, from another folder.
  $window = Start-Window $Installed
  $installedGate = Start-Gate $Installed
  $devGate = Start-Gate $Unpacked
  foreach ($gate in $installedGate, $devGate) {
    $read = $gate.StandardOutput.ReadLineAsync()
    if (-not $read.Wait(30000)) { throw 'A gate did not answer initialize' }
  }
  $before = @(Get-DevHQProcesses)
  Note 'DevHQ processes before the installer' (($before | ForEach-Object { "$($_.id): $($_.commandLine)" }) -join '; ')
  Check 'The window and both gates are running' ($before.Count -ge 3 -and -not $window.HasExited -and -not $installedGate.HasExited -and -not $devGate.HasExited) "$($before.Count) devhq.exe"
  # The old installer, so that the installed executable's hash says whether the installer
  # replaced it while DevHQ ran. Both builds pin the same Tauri CLI, so both installers come from
  # the same NSIS template and close running processes alike.
  $watch = [Diagnostics.Stopwatch]::StartNew()
  $installer = Join-Path $Inputs $Manifest.builds.old.installer.file
  $process = Start-Process -FilePath $installer -ArgumentList '/S' -PassThru
  $finished = $process.WaitForExit(300000)
  if (-not $finished) { Stop-Process -Id $process.Id -Force }
  Note 'The old installer, run with /S over the running new build' "finished: $finished, exit code $(if ($finished) { $process.ExitCode }), $([math]::Round($watch.Elapsed.TotalSeconds, 1)) s"
  Start-Sleep -Seconds 3
  foreach ($pair in @(@('the installed window', $window), @('the installed gate', $installedGate), @('the unpacked gate, from another folder', $devGate))) {
    $pair[1].Refresh()
    Note "After the installer, $($pair[0])" $(if ($pair[1].HasExited) { "ended, exit code $($pair[1].ExitCode)" } else { 'still runs' })
  }
  $after = @(Get-DevHQProcesses)
  Note 'DevHQ processes still running after the installer' $(if ($after.Count) { ($after | ForEach-Object { "$($_.id): $($_.commandLine)" }) -join '; ' } else { 'none' })
  foreach ($left in $after) { Stop-Process -Id $left.id -Force -ErrorAction SilentlyContinue }
  if ($after.Count) { Start-Sleep -Seconds 3; Note 'The rehearsal ended those processes itself' "$($after.Count)" }
  $now = if (Test-Path $Installed) { Get-Sha256 $Installed }
  $which = if ($now -eq (Read-State 'installed-old-B').sha256) { 'the old build: the installer replaced it' } elseif ($now -eq (Read-State 'installed-new-D').sha256) { 'still the new build: the installer did not replace it' } else { "neither build ($now)" }
  Note 'The installed executable after the installer' $which
  Note 'Inventory files after the forced close' ((Get-ChildItem -Force $Roaming -Filter 'inventory.sqlite3*' | ForEach-Object { "$($_.Name) $($_.Length)" }) -join ', ')
  Note 'Development inventory files after the forced close' ((Get-ChildItem -Force $RoamingDev -Filter 'inventory.sqlite3*' | ForEach-Object { "$($_.Name) $($_.Length)" }) -join ', ')
  Save-Inventory 'G1' | Out-Null
  Compare-Inventory 'G0' 'G1' 'What SQLite had committed survived the forced close (installed inventory, read with its log)'
  Save-Inventory 'G1-dev' $DevInventory | Out-Null
  Compare-Inventory 'G0-dev' 'G1-dev' 'and the development inventory too'

  # Back to the state phase F left, as a person would: everything closed, then the new build
  # installed and opened.
  Assert-NoDevHQ 'before installing the new build again'
  Install-DevHQ 'new'
  Test-Installation 'new'
  Open-AndClose $Installed 'DevHQ' 'The installed new build, after the forced close' $Installed $Inventory (Join-Path $Local 'EBWebView')
  Check 'No write-ahead log stayed after this clean closing' (-not (Test-Path "$Inventory-wal")) ''
  Save-Inventory 'G2' | Out-Null
  Compare-Inventory 'baseline' 'G2' 'Every table still holds exactly the rows it held before the upgrade'
  $status = (Invoke-Gate $Installed @((Call 'status')) 'the new build after the forced close')[0].structured
  Check 'The gate still finds Read only' ($status.access -eq 'read') "access $($status.access)"
  Assert-NoDevHQ 'at the end of the phase'
  Save-State (Get-DataTrees) 'files-post-G'
  Save-State (Get-DevTrees) 'files-dev-post-G'
}

function Invoke-PhaseH {
  Assert-NoDevHQ 'before going back to the old build'
  Install-DevHQ 'old'
  $differences = Compare-Trees (Read-State 'files-post-G') (Get-DataTrees)
  Check 'Every data file, backup, witness and set-aside file is byte for byte as before the reinstallation' ($differences.Count -eq 0) ($differences -join '; ')
  $differences = Compare-Trees (Read-State 'files-dev-post-G') (Get-DevTrees)
  Check 'The development data is byte for byte as before' ($differences.Count -eq 0) ($differences -join '; ')
  Test-Installation 'old'
  Check 'The old executable is back, the same as the first installation' ((Read-State 'installed-old-B').sha256 -eq (Get-Sha256 $Installed)) ''
  Open-AndClose $Installed 'DevHQ' 'The reinstalled old build' $Installed $Inventory (Join-Path $Local 'EBWebView')
  Save-Inventory 'H' | Out-Null
  Compare-Inventory 'baseline' 'H' 'After the old build opened it again, every table holds exactly the rows it held before the upgrade'
  $status = (Invoke-Gate $Installed @((Call 'status')) 'the old build after going back')[0].structured
  Test-InstalledStatus $status 'old'
  Test-Contents $Installed 'old' | Out-Null
  Test-WritesRefused $Installed 'old' 'H'
  # Compare-Trees hands back its array as one object: unroll it before filtering.
  $trees = Get-DataTrees
  Save-State $trees 'files-post-H'
  $all = Compare-Trees (Read-State 'files-pre-upgrade') $trees
  Note 'Files that differ from before the upgrade, after the old build opened again' $(if ($all.Count) { $all -join '; ' } else { 'none' })
  $differences = @($all | Where-Object { -not (Test-InventoryFile $_) })
  Check 'Backups, witnesses, this-workstation.json and the set-aside files are byte for byte as before the upgrade' ($differences.Count -eq 0) ($differences -join '; ')
  $differences = Compare-Trees (Read-State 'files-dev-post-G') (Get-DevTrees)
  Check 'The old build left the development data alone' ($differences.Count -eq 0) ($differences -join '; ')
}

function Invoke-PhaseReport {
  $lines = [Collections.Generic.List[string]]::new()
  $lines.Add('# DevHQ upgrade rehearsal')
  $lines.Add('')
  $lines.Add("Run $env:GITHUB_SERVER_URL/$env:GITHUB_REPOSITORY/actions/runs/$env:GITHUB_RUN_ID (attempt $env:GITHUB_RUN_ATTEMPT), workflow commit $env:GITHUB_SHA, runner $env:ImageOS $env:ImageVersion.")
  $lines.Add('')
  if ($Manifest) {
    $lines.Add('| Build | Commit | Installer | Installer SHA-256 | Unpacked devhq.exe SHA-256 | Installed devhq.exe SHA-256 (in the installer) | Installed devhq.exe SHA-256 (observed) |')
    $lines.Add('|---|---|---|---|---|---|---|')
    foreach ($which in 'old', 'new') {
      $build = $Manifest.builds[$which]
      $first = if ($which -eq 'old') { 'installed-old-B' } else { 'installed-new-D' }
      $observed = if (Test-Path (Join-Path $State "$first.json")) { (Read-State $first).sha256 }
      $lines.Add("| $which | $($build.commit) | $($build.installer.file) | $($build.installer.sha256) | $($build.unpacked.sha256) | $($build.packedInInstaller.sha256) | $observed |")
    }
    $lines.Add('')
  }
  foreach ($name in 'Inputs', 'B', 'C', 'D', 'E', 'F', 'G', 'H') {
    $file = Join-Path $ReportDir "phase-$name.json"
    if (-not (Test-Path $file)) { $lines.Add("## Phase ${name}: did not run"); $lines.Add(''); continue }
    $result = Get-Content -Raw $file | ConvertFrom-Json -AsHashtable -Depth 30
    $failed = @($result.checks | Where-Object { $_.ok -eq $false }).Count
    $passed = @($result.checks | Where-Object { $_.ok -eq $true }).Count
    $lines.Add("## Phase ${name}: $(if ($failed) { "FAILED ($failed failed, $passed passed)" } else { "passed ($passed checks)" }), $($result.seconds) s")
    $lines.Add('')
    foreach ($check in $result.checks) {
      $mark = if ($check.ok -eq $true) { 'PASS' } elseif ($check.ok -eq $false) { '**FAIL**' } else { 'NOTE' }
      $detail = if ($check.detail) { " :: $($check.detail -replace "`r?`n", ' ')" } else { '' }
      $lines.Add("- $mark $($check.check)$detail")
    }
    $lines.Add('')
  }
  $report = Join-Path $ReportDir 'report.md'
  $lines | Set-Content -LiteralPath $report -Encoding utf8NoBOM
  if ($env:GITHUB_STEP_SUMMARY) { Get-Content -Raw $report | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Encoding utf8NoBOM }
  Get-Content -Raw $report
}

# --- Run one phase ---------------------------------------------------------------------------

if ($Phase -eq 'Report') { Invoke-PhaseReport; return }
$watch = [Diagnostics.Stopwatch]::StartNew()
try {
  & "Invoke-Phase$Phase"
} catch {
  Check 'The phase ran to its end' $false "$($_ | Out-String) $($_.ScriptStackTrace)"
} finally {
  Save-Json ([ordered]@{ phase = $Phase; seconds = [math]::Round($watch.Elapsed.TotalSeconds); checks = $script:Results }) (Join-Path $ReportDir "phase-$Phase.json")
}
$failed = @($script:Results | Where-Object { $_.ok -eq $false })
if ($failed.Count) { throw "$($failed.Count) checks failed in phase $Phase" }
