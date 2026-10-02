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

# The probe has a name of its own, so a detection can only be this file.
New-Item -ItemType Directory -Force $Probe | Out-Null
$file = Join-Path $Probe "devhq-antivirus-probe-$([guid]::NewGuid()).txt"
# Built from two halves, so that no file in the repository is itself the test file.
$eicar = 'X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR' + '-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*'
$blocked = $false
try {
  [IO.File]::WriteAllText($file, $eicar)
} catch {
  # Defender may refuse the write itself: ERROR_VIRUS_INFECTED (225) or ERROR_VIRUS_DELETED (226).
  $cause = $_.Exception
  while ($cause.InnerException) { $cause = $cause.InnerException }
  if (($cause.HResult -band 0xFFFF) -notin 225, 226) { throw }
  "Writing the probe was refused: $($cause.Message)"
  $blocked = $true
}
$deadline = (Get-Date).AddSeconds(30)
while (-not $blocked -and (Test-Path -LiteralPath $file) -and (Get-Date) -lt $deadline) {
  Start-Sleep -Milliseconds 250
}
if (Test-Path -LiteralPath $file) {
  if ($blocked) {
    Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue
  } else {
    Remove-Item -LiteralPath $file -Force
    throw "Real-time protection left a test file in $Probe alone: that folder is not scanned."
  }
}
"Defender detected a test file written to $Probe, so real-time protection scans that folder."
Get-MpThreatDetection | Where-Object { $_.Resources -match [regex]::Escape((Split-Path -Leaf $file)) } |
  Format-List ThreatID, ActionSuccess, InitialDetectionTime, Resources
