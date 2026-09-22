# Bump app version: last segment +1 (0.0.1 -> 0.0.2). ASCII-only source.
# Syncs Cargo.toml package version + tauri.conf.json version.
# Usage: powershell -File scripts/bump-version.ps1 [-To 0.0.2]
# Do NOT rewrite README when bumping for a commit/release.

param([string]$To = '')

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$cargo = Join-Path $root 'personal-workbench\src-tauri\Cargo.toml'
$conf  = Join-Path $root 'personal-workbench\src-tauri\tauri.conf.json'

$cargoText = Get-Content -Raw -Encoding UTF8 $cargo
if ($cargoText -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'version not found in Cargo.toml' }
$current = $Matches[1]

if (-not $To) {
  $parts = $current -split '\.'
  $last = [int]$parts[-1]
  $parts[-1] = [string]($last + 1)
  $To = ($parts -join '.')
}

$oldLine = 'version = "' + $current + '"'
$newLine = 'version = "' + $To + '"'
$idx = $cargoText.IndexOf($oldLine)
if ($idx -lt 0) { throw ('Cargo.toml missing ' + $oldLine) }
$cargoText = $cargoText.Remove($idx, $oldLine.Length).Insert($idx, $newLine)
Set-Content -Path $cargo -Value $cargoText -Encoding UTF8 -NoNewline

$confRaw = Get-Content -Raw -Encoding UTF8 $conf
$confRaw = $confRaw -replace '"version"\s*:\s*"[^"]+"', ('"version": "' + $To + '"')
Set-Content -Path $conf -Value $confRaw -Encoding UTF8 -NoNewline

Write-Host ("version " + $current + " -> " + $To)
Write-Host ("release tag: v" + $To)
