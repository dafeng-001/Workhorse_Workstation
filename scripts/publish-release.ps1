# Publish local EXE as GitHub Release asset (overwrite same name).
# Uses git credential for github.com. ASCII-only source (PowerShell 5.1 safe).
# Usage: powershell -File scripts/publish-release.ps1 [-Tag v2026.09.22] [-Notes 'text']

param(
  [string]$Tag = ("v" + (Get-Date -Format 'yyyy.MM.dd')),
  [string]$Notes = '',
  [string]$Repo = 'dafeng-001/Workhorse_Workstation',
  [string]$ExePath = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $ExePath) {
  $ExePath = Join-Path $root ([string]([char]0x725B) + [char]0x9A6C + [char]0x5DE5 + [char]0x4F5C + [char]0x53F0 + '.exe')
  if (-not (Test-Path $ExePath)) {
    $ExePath = Join-Path $root 'personal-workbench\personal-workbench.exe'
  }
  if (-not (Test-Path $ExePath)) {
    $ExePath = Join-Path $root 'personal-workbench\src-tauri\target\release\personal-workbench.exe'
  }
}
$ExePath = (Resolve-Path $ExePath).Path
$AssetName = 'Workhorse_Workstation-win64.exe'

$credLines = "protocol=https`nhost=github.com`n`n" | git credential fill
$token = ($credLines | Where-Object { $_ -like 'password=*' }) -replace '^password=',''
if (-not $token) { throw 'No github.com password/token in git credential' }

$headers = @{
  Authorization = "Bearer $token"
  Accept = 'application/vnd.github+json'
  'X-GitHub-Api-Version' = '2022-11-28'
  'User-Agent' = 'workhorse-release'
}

if (-not $Notes) {
  $Notes = "Workhorse Workstation portable EXE ($Tag). Download $AssetName and run it in an empty folder. See CALC.md for metrics."
}

$rel = $null
try {
  $rel = Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/$Repo/releases/tags/$Tag"
} catch {
  $bodyObj = @{
    tag_name = $Tag
    name = "Workhorse Workstation $Tag"
    body = $Notes
    draft = $false
    prerelease = $false
  }
  $body = $bodyObj | ConvertTo-Json -Depth 4
  $rel = Invoke-RestMethod -Headers $headers -Method Post -Uri "https://api.github.com/repos/$Repo/releases" -Body $body -ContentType 'application/json'
}

$assets = Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/$Repo/releases/$($rel.id)/assets"
foreach ($a in $assets) {
  if ($a.name -eq $AssetName -or $a.name -like '*win64*' -or $a.name -like '*.exe') {
    Invoke-RestMethod -Headers $headers -Method Delete -Uri "https://api.github.com/repos/$Repo/releases/assets/$($a.id)" | Out-Null
  }
}

$uploadUrl = "https://uploads.github.com/repos/$Repo/releases/$($rel.id)/assets?name=$AssetName"
$up = & curl.exe -sS -X POST -H "Authorization: Bearer $token" -H "Content-Type: application/octet-stream" -H "User-Agent: workhorse-release" --data-binary "@$ExePath" $uploadUrl | ConvertFrom-Json
if ($up.id) {
  Write-Host "OK $($rel.html_url)"
  Write-Host $up.browser_download_url
} else {
  throw ("upload failed: " + ($up | ConvertTo-Json -Compress))
}
