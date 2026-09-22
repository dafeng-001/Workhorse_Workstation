# Bump app version (last segment +1) in Cargo.toml + tauri.conf.json.
# ASCII-only source. Usage: powershell -File scripts/bump-version.ps1 [-To 2026.9.23]
# Without -To, reads current from Cargo.toml and adds 1 to the last numeric part.

param([string]$To = '')

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$cargo = Join-Path $root 'personal-workbench\src-tauri\Cargo.toml'
$conf  = Join-Path $root 'personal-workbench\src-tauri\tauri.conf.json'

$cargoText = Get-Content -Raw -Encoding UTF8 $cargo
if ($cargoText -notmatch 'version\s*=\s*"([^"]+)"') { throw 'version not found in Cargo.toml' }
$current = $Matches[1]

if (-not $To) {
  $parts = $current -split '\.'
  $last = [int]$parts[-1]
  $parts[-1] = [string]($last + 1)
  $To = ($parts -join '.')
}

$cargoText = $cargoText -replace 'version\s*=\s*"[^"]+"', ('version = "' + $To + '"', 1)
# only first version = under [package] — replace once
$cargoText = (Get-Content -Raw -Encoding UTF8 $cargo)
$idx = $cargoText.IndexOf('version = "')
if ($idx -lt 0) { throw 'Cargo.toml package version missing' }
$cargoText = $cargoText.Remove($idx, ('version = "' + $current + '"').Length)
$cargoText = $cargoText.Insert($idx, ('version = "' + $To + '"'))
Set-Content -Path $cargo -Value $cargoText -Encoding UTF8 -NoNewline

$confObj = Get-Content -Raw -Encoding UTF8 $conf | ConvertFrom-Json
$confObj.version = $To
$confObj | ConvertTo-Json -Depth 20 | Set-Content -Path $conf -Encoding UTF8

$tag = 'v' + $To
Write-Host "version $current -> $To"
Write-Host "release tag should be: $tag"
Write-Host "next: scripts/publish-release.ps1 -Tag $tag"
