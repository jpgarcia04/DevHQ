# What the upgrade rehearsal's build and its phases share (.github/workflows/upgrade-rehearsal.yml):
# file hashes and the bundle marker Tauri's bundler patches in the executable it packs.
#
# The marker: an executable that was never packed holds `__TAURI_BUNDLE_TYPE_VAR_UNK` once. Before
# packing it into the NSIS installer, the bundler rewrites those last three bytes to `NSS`, which
# `tauri::utils::platform::bundle_type` reads as NSIS, so src-tauri/src/identity.rs gives it the
# installed DevHQ's data. The executable left in target/ keeps `UNK`: its hash is never the
# installed executable's. The other `__TAURI_BUNDLE_TYPE_VAR_` strings are what `bundle_type`
# compares against and are in every executable, so only the count of `UNK` tells them apart.

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;

public static class RehearsalBytes {
    public static List<int> Find(byte[] haystack, byte[] needle) {
        var found = new List<int>();
        for (int at = 0; at <= haystack.Length - needle.Length; at++) {
            int i = 0;
            while (i < needle.Length && haystack[at + i] == needle[i]) i++;
            if (i == needle.Length) found.Add(at);
        }
        return found;
    }

    /// The offsets where two files of the same length differ, at most `limit` of them.
    public static List<int> Differences(byte[] a, byte[] b, int limit) {
        var found = new List<int>();
        if (a.Length != b.Length) return null;
        for (int at = 0; at < a.Length && found.Count < limit; at++)
            if (a[at] != b[at]) found.Add(at);
        return found;
    }
}
'@

$script:UnpackedMarker = [Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_UNK')

function Get-Sha256([string] $Path) {
  (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-Sha256OfBytes([byte[]] $Bytes) {
  [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($Bytes)).ToLowerInvariant()
}

# Where the unpatched marker is in an executable: one offset for an unpacked build, none for the
# one an installer packed.
function Get-UnpackedMarkers([string] $Path) {
  , [RehearsalBytes]::Find([IO.File]::ReadAllBytes($Path), $script:UnpackedMarker).ToArray()
}

# The SHA-256 the executable at $Path would have once packed: its one marker patched as the
# bundler patches it. Null when it does not hold exactly one.
function Get-PackedSha256([string] $Path) {
  $bytes = [IO.File]::ReadAllBytes($Path)
  $at = [RehearsalBytes]::Find($bytes, $script:UnpackedMarker)
  if ($at.Count -ne 1) { return $null }
  $patch = [Text.Encoding]::ASCII.GetBytes('NSS')
  [Array]::Copy($patch, 0, $bytes, $at[0] + $script:UnpackedMarker.Length - 3, 3)
  Get-Sha256OfBytes $bytes
}

# Where two executables differ, and what each holds there: for the unpacked and the packed build
# of one commit it must be the marker's last three bytes and nothing else.
function Compare-Executables([string] $Unpacked, [string] $Packed) {
  $a = [IO.File]::ReadAllBytes($Unpacked)
  $b = [IO.File]::ReadAllBytes($Packed)
  $offsets = [RehearsalBytes]::Differences($a, $b, 64)
  if ($null -eq $offsets) {
    return [ordered]@{ sameLength = $false; unpackedBytes = $a.Length; packedBytes = $b.Length }
  }
  [ordered]@{
    sameLength = $true
    differingOffsets = @($offsets)
    unpacked = [Text.Encoding]::ASCII.GetString([byte[]] @($offsets | ForEach-Object { $a[$_] }))
    packed = [Text.Encoding]::ASCII.GetString([byte[]] @($offsets | ForEach-Object { $b[$_] }))
  }
}

function Save-Json($Value, [string] $Path) {
  $Value | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $Path -Encoding utf8NoBOM
}

function Read-Json([string] $Path) {
  Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -Depth 30
}
