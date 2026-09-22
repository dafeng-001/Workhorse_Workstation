# 将本机 牛马工作台.exe 发布为 GitHub Release 资产（保留最近一版）。
# 依赖：git 已登录 github.com；与远程同源仓库 dafeng-001/Workhorse_Workstation
# 用法：powershell -File scripts/publish-release.ps1 [-Tag v2026.09.22] [-Notes '说明']

param(
  [string]$Tag = ("v" + (Get-Date -Format 'yyyy.MM.dd')),
  [string]$Notes = '',
  [string]$Repo = 'dafeng-001/Workhorse_Workstation',
  [string]$ExePath = (Join-Path $PSScriptRoot '..\牛马工作台.exe')
)

$ErrorActionPreference = 'Stop'
$ExePath = (Resolve-Path $ExePath).Path
$AssetName = 'Workhorse_Workstation-win64.exe'

$credLines = "protocol=https`nhost=github.com`n`n" | git credential fill
$token = ($credLines | Where-Object { $_ -like 'password=*' }) -replace '^password=',''
if (-not $token) { throw 'git credential 里没有 github.com 密码/令牌' }

$headers = @{
  Authorization = "Bearer $token"
  Accept = 'application/vnd.github+json'
  'X-GitHub-Api-Version' = '2022-11-28'
  'User-Agent' = 'workhorse-release'
}

if (-not $Notes) {
  $Notes = "牛马工作台 Windows 便携版（$Tag）`n`n下载 **$AssetName** 放到空目录双击运行。`n指标口径见 CALC.md。"
}

# 若同 tag 已存在则复用
$rel = $null
try {
  $rel = Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/$Repo/releases/tags/$Tag"
} catch {
  $body = @{
    tag_name = $Tag
    name = "牛马工作台 $Tag"
    body = $Notes
    draft = $false
    prerelease = $false
  } | ConvertTo-Json -Depth 4
  $rel = Invoke-RestMethod -Headers $headers -Method Post -Uri "https://api.github.com/repos/$Repo/releases" -Body $body -ContentType 'application/json'
}

# 覆盖同名资产
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
  throw "upload failed: $($up | ConvertTo-Json -Compress)"
}
