# Microsoft Defender's real-time protection on the Windows runner, which ships with it off and
# with C:\ and D:\ excluded from scanning, so that what DevHQ writes is scanned as on a desktop.
#
# -On turns on real-time protection with behaviour monitoring and the scanning of downloaded
# files (a desktop's defaults), removes every exclusion and then proves that the folder -Probe is
# scanned: it writes the EICAR test file there, the harmless string every antivirus detects on
# purpose, and Defender must remove or block it. -Check proves that protection is still on.
# -Off turns real-time protection off again, so that saving the caches when a job ends does not
# have every file it reads scanned. Each fails the step when Defender did not comply.
[CmdletBinding(DefaultParameterSetName = 'On')]
param(
  [Parameter(Mandatory, ParameterSetName = 'On')] [switch] $On,
  [Parameter(Mandatory, ParameterSetName = 'On')] [string] $Probe,
  [Parameter(Mandatory, ParameterSetName = 'Check')] [switch] $Check,
  [Parameter(Mandatory, ParameterSetName = 'Off')] [switch] $Off
)
$ErrorActionPreference = 'Stop'

function Show-Status {
  Get-MpComputerStatus | Format-List AMRunningMode, AntivirusEnabled, RealTimeProtectionEnabled,
    OnAccessProtectionEnabled, BehaviorMonitorEnabled, IoavProtectionEnabled, IsTamperProtected
}

# Defender applies a preference a moment after it is set.
function Wait-Protection([bool] $enabled) {
  $deadline = (Get-Date).AddMinutes(1)
  do {
    $status = Get-MpComputerStatus
    if ($status.RealTimeProtectionEnabled -eq $enabled -and $status.OnAccessProtectionEnabled -eq $enabled) {
      return $true
    }
    Start-Sleep -Seconds 2
  } while ((Get-Date) -lt $deadline)
  return $false
}

if ($Off) {
  Set-MpPreference -DisableRealtimeMonitoring $true
  if (-not (Wait-Protection $false)) { Show-Status; throw 'Real-time protection did not turn off.' }
  'Real-time protection is off again.'
  return
}

if ($Check) {
  Show-Status
  if (-not (Wait-Protection $true)) { throw 'Real-time protection is no longer on.' }
  'Real-time protection is still on.'
  return
}

'Before:'
Show-Status
$preference = Get-MpPreference
"Excluded paths: $($preference.ExclusionPath -join ', ')"
"Excluded extensions: $($preference.ExclusionExtension -join ', ')"
"Excluded processes: $($preference.ExclusionProcess -join ', ')"
if ($preference.ExclusionPath) { Remove-MpPreference -ExclusionPath $preference.ExclusionPath }
if ($preference.ExclusionExtension) { Remove-MpPreference -ExclusionExtension $preference.ExclusionExtension }
if ($preference.ExclusionProcess) { Remove-MpPreference -ExclusionProcess $preference.ExclusionProcess }
Set-MpPreference -DisableRealtimeMonitoring $false -DisableBehaviorMonitoring $false -DisableIOAVProtection $false
$enabled = Wait-Protection $true
'After:'
Show-Status
if (-not $enabled) {
  # A group policy wins over Set-MpPreference without saying so.
  Get-ChildItem -Recurse 'HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender' -ErrorAction SilentlyContinue |
    Format-List Name, Property
  throw 'Real-time protection did not turn on.'
}
$preference = Get-MpPreference
$left = @($preference.ExclusionPath) + @($preference.ExclusionExtension) + @($preference.ExclusionProcess) |
  Where-Object { $_ }
if ($left) { throw "Exclusions remain: $($left -join ', ')" }
# What Defender does with what it finds. On a desktop each threat gets its signature's action
# (EICAR's is quarantine); Allow, NoAction or UserDefined here would leave the probe in place.
$actions = 'LowThreatDefaultAction', 'ModerateThreatDefaultAction', 'HighThreatDefaultAction',
  'SevereThreatDefaultAction', 'UnknownThreatDefaultAction'
$preference | Format-List $actions
foreach ($action in $actions) {
  if ($preference.$action -in 6, 8, 9) {
    "Resetting $action ($($preference.$action)) to its signature's action."
    $reset = @{ $action = $true }
    Remove-MpPreference @reset
  }
}

# Built from two halves, so that no file in the repository is itself the test file.
$eicar = 'X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR' + '-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*'

# ERROR_VIRUS_INFECTED (225) or ERROR_VIRUS_DELETED (226): Defender refused the operation.
function Test-Refused($record) {
  $cause = $record.Exception
  while ($cause.InnerException) { $cause = $cause.InnerException }
  if (($cause.HResult -band 0xFFFF) -in 225, 226) { return $cause.Message }
}

# Writes probes until one is detected. Each gets a name of its own, so a detection can only be
# that file. Real-time protection scans a file when it is closed after a write and again when it
# is opened, so each probe is also read back for a few seconds: the first probe can be written
# while Defender is still applying the new settings, which leaves it unscanned until opened.
function Find-Probe([string] $folder, [int] $seconds) {
  New-Item -ItemType Directory -Force $folder | Out-Null
  $started = Get-Date
  $attempt = 0
  while ((Get-Date) -lt $started.AddSeconds($seconds)) {
    $attempt += 1
    if ($script:undetected) { Remove-Item -LiteralPath $script:undetected -Force -ErrorAction SilentlyContinue }
    $file = Join-Path $folder "devhq-antivirus-probe-$([guid]::NewGuid()).txt"
    try {
      [IO.File]::WriteAllText($file, $eicar)
    } catch {
      $refused = Test-Refused $_
      if (-not $refused) { throw }
      return "Defender refused to write probe $attempt after $([int]((Get-Date) - $started).TotalSeconds) s: $refused"
    }
    $until = (Get-Date).AddSeconds(5)
    while ((Get-Date) -lt $until) {
      if (-not (Test-Path -LiteralPath $file)) {
        return "Defender removed probe $attempt after $([int]((Get-Date) - $started).TotalSeconds) s."
      }
      try {
        [IO.File]::ReadAllBytes($file) | Out-Null
      } catch {
        $refused = Test-Refused $_
        if ($refused) {
          Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue
          return "Defender refused to open probe $attempt after $([int]((Get-Date) - $started).TotalSeconds) s: $refused"
        }
      }
      Start-Sleep -Milliseconds 500
    }
    $script:undetected = $file
  }
}

$undetected = $null
$found = Find-Probe $Probe 60
if (-not $found) {
  # Everything that could explain it, then the engine itself asked to scan the probe.
  Get-MpComputerStatus | Format-List AMEngineVersion, AMProductVersion, AMServiceEnabled,
    AntivirusSignatureVersion, AntivirusSignatureLastUpdated, AntivirusSignatureAge,
    DefenderSignaturesOutOfDate, RealTimeProtectionEnabled, OnAccessProtectionEnabled
  Get-MpPreference | Format-List (@('DisableRealtimeMonitoring', 'RealTimeScanDirection',
      'DisableBehaviorMonitoring', 'DisableIOAVProtection', 'DisableScriptScanning') + $actions)
  Get-MpThreatDetection | Format-List ThreatID, ActionSuccess, InitialDetectionTime, Resources
  Start-MpScan -ScanType CustomScan -ScanPath $undetected
  "After a scan on demand, the last probe is $(if (Test-Path -LiteralPath $undetected) { 'still there' } else { 'gone' })."
  Get-MpThreatDetection | Format-List ThreatID, ActionSuccess, InitialDetectionTime, Resources
  Remove-Item -LiteralPath $undetected -Force -ErrorAction SilentlyContinue
  throw "Real-time protection left every test file written to $Probe for a minute alone: that folder is not scanned."
}
$found
"So real-time protection scans $Probe."
