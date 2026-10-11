# Phase A of the upgrade rehearsal (.github/workflows/upgrade-rehearsal.yml): builds DevHQ's NSIS
# installer from one exact commit, in the checkout it is run from, and describes what it built.
#
# The checkout must be clean at that commit before and after the build: what is built is the
# commit and nothing else. The installer leaves target/ for $Out, so the next build's bundle
# folder holds only its own. The description (<Label>.json in $Out) gives the SHA-256 of the
# installer, of the unpacked executable in target/, and of the executable the installer holds,
# found two ways that must agree: extracted from the installer with 7-Zip, and predicted by
# patching the unpacked executable's marker as the bundler does (upgrade-rehearsal-common.ps1).
param(
  [Parameter(Mandatory)] [ValidatePattern('^[0-9a-f]{40}$')] [string] $Commit,
  [Parameter(Mandatory)] [string] $Label,
  [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'upgrade-rehearsal-common.ps1')

function Invoke-Native([string] $What, [scriptblock] $Command) {
  & $Command
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

function Assert-Clean([string] $When) {
  $dirty = @(git status --porcelain --untracked-files=all)
  if ($LASTEXITCODE -ne 0) { throw 'git status failed' }
  if ($dirty.Count) { throw "The checkout is not clean $When`n$($dirty -join "`n")" }
}

New-Item -ItemType Directory -Force $Out | Out-Null
Invoke-Native 'git checkout' { git -c advice.detachedHead=false checkout --quiet --detach $Commit }
$head = git rev-parse HEAD
if ($head -ne $Commit) { throw "HEAD is $head, not $Commit" }
Assert-Clean 'before the build'
$version = (Get-Content -Raw src-tauri/tauri.conf.json | ConvertFrom-Json).version
"Building $Label = $head, version $version"

Invoke-Native 'npm ci' { npm ci --no-audit --no-fund }
Invoke-Native 'tauri build' { npm run tauri build }
Assert-Clean 'after the build'

$bundles = @(Get-ChildItem src-tauri/target/release/bundle/nsis -Filter '*-setup.exe')
if ($bundles.Count -ne 1) { throw "Expected one installer, found: $($bundles.Name -join ', ')" }
# DevHQ_0.1.0_x64-setup.exe becomes DevHQ_0.1.0_<label>_x64-setup.exe: the bytes are the same.
$name = $bundles[0].Name -replace '_x64-setup\.exe$', "_${Label}_x64-setup.exe"
$installer = Join-Path $Out $name
Move-Item -LiteralPath $bundles[0].FullName -Destination $installer
$unpacked = Join-Path $Out "devhq-$Label-unpacked.exe"
Copy-Item -LiteralPath src-tauri/target/release/devhq.exe -Destination $unpacked

# One unpatched marker, or the prediction below cannot be made (and, for a build with
# src-tauri/src/identity.rs, which data it keeps would be unclear): the phases check it.
$markers = Get-UnpackedMarkers $unpacked
"The unpacked executable holds $($markers.Count) unpatched bundle markers"
$predicted = Get-PackedSha256 $unpacked

# 7-Zip reads NSIS installers. Tauri's installer holds devhq.exe as it installs it.
$extracted = Join-Path $env:RUNNER_TEMP "extracted-$Label"
Remove-Item -Recurse -Force $extracted -ErrorAction SilentlyContinue
& 7z x -y "-o$extracted" $installer | Out-Null
$extractedExe = @(Get-ChildItem -Recurse -File $extracted -Filter devhq.exe -ErrorAction SilentlyContinue)
$fromInstaller = $null
if ($LASTEXITCODE -eq 0 -and $extractedExe.Count -eq 1) {
  $fromInstaller = [ordered]@{
    path = $extractedExe[0].FullName.Substring($extracted.Length + 1)
    sha256 = Get-Sha256 $extractedExe[0].FullName
    unpatchedMarkers = (Get-UnpackedMarkers $extractedExe[0].FullName).Count
    differenceFromUnpacked = Compare-Executables $unpacked $extractedExe[0].FullName
  }
  "Files in the installer: $((Get-ChildItem -Recurse -File $extracted | ForEach-Object { $_.FullName.Substring($extracted.Length + 1) }) -join ', ')"
} else {
  "7-Zip could not extract one devhq.exe (exit $LASTEXITCODE, found $($extractedExe.Count)): the installed hash is only predicted."
}

$description = [ordered]@{
  label = $Label
  commit = $head
  version = $version
  installer = [ordered]@{ file = $name; sha256 = Get-Sha256 $installer; bytes = (Get-Item $installer).Length }
  unpacked = [ordered]@{ file = Split-Path -Leaf $unpacked; sha256 = Get-Sha256 $unpacked; markerOffsets = @($markers) }
  packedPredicted = $predicted
  packedInInstaller = $fromInstaller
  tauriCrate = (Select-String -Path src-tauri/Cargo.lock -Pattern '^name = "tauri"$' -Context 0, 1).Context.PostContext[0]
}
Save-Json $description (Join-Path $Out "$Label.json")
$description | ConvertTo-Json -Depth 10
if ($fromInstaller) {
  if ($fromInstaller.sha256 -ne $predicted) { throw 'The executable in the installer is not the unpacked one with its marker patched' }
  if ($fromInstaller.unpatchedMarkers -ne 0) { throw 'The executable in the installer still holds an unpatched marker' }
}
