# Encrypt portable exe for a customer: key = f(license code + machine id)
# Usage: powershell -File scripts\make-update-pack.ps1 -Code WM1.xxx.yyy -Machine <id> [-Exe path] [-Out update.wmp]
param([Parameter(Mandatory=$true)][string]$Code, [string]$Machine='', [string]$Exe='', [string]$Out='')
$ErrorActionPreference='Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $root 'personal-workbench\src-tauri\target\release\personal-workbench.exe' }
if (-not $Out) { $Out = Join-Path $root 'update.wmp' }
$Exe = (Resolve-Path $Exe).Path

function Get-Fnv([byte[]]$bytes) {
  $h = [UInt64]0xcbf29ce484222325
  foreach ($b in $bytes) { $h = $h -bxor [UInt64]$b; $h = [UInt64]($h * [UInt64]0x100000001b3) }
  $h
}
function HmacLike([byte[]]$msg) {
  $Secret = [Text.Encoding]::UTF8.GetBytes('wm-workhorse-license-v1-2026')
  $a = New-Object Collections.Generic.List[byte]; $a.AddRange($Secret); $a.AddRange($msg)
  $b = New-Object Collections.Generic.List[byte]; $b.AddRange($msg); $b.AddRange($Secret)
  return ('{0:x16}{1:x16}' -f (Get-Fnv $a.ToArray()), (Get-Fnv $b.ToArray()))
}
# parse machine from code if empty: leave empty -> still derive with empty machine like app's current machine?
# For bound pack: use provided machine (from app settings)
if (-not $Machine) { Write-Host 'Warning: Machine empty — pack only works if license machine_id is empty' }
$Secret = [Text.Encoding]::UTF8.GetBytes('wm-workhorse-license-v1-2026')
$seed = New-Object Collections.Generic.List[byte]
$seed.AddRange($Secret)
$seed.AddRange([Text.Encoding]::UTF8.GetBytes($Code))
$seed.AddRange([Text.Encoding]::UTF8.GetBytes($Machine))
$key = New-Object byte[] 32
for ($i=0; $i -lt 4; $i++) {
  $c = New-Object Collections.Generic.List[byte]
  $c.AddRange($seed.ToArray()); $c.Add([byte]$i)
  $h = HmacLike $c.ToArray()
  $hb = [Text.Encoding]::UTF8.GetBytes($h)
  for ($j=0; $j -lt 8; $j++) {
    $key[$i*8+$j] = $hb[$j] -bxor $(if ($j+8 -lt $hb.Length) { $hb[$j+8] } else { 0 })
  }
}
$data = [IO.File]::ReadAllBytes($Exe)
$plain = [byte[]]$data.Clone()
for ($i=0; $i -lt $plain.Length; $i++) {
  $k = $key[$i % 32] -bxor ([byte](($i / 32) * 31) )
  $plain[$i] = $plain[$i] -bxor $k
}
$chk = New-Object Collections.Generic.List[byte]
$chk.AddRange($key); $chk.AddRange($plain)
$mac = (HmacLike $chk.ToArray()).Substring(0,16)
$magic = [Text.Encoding]::ASCII.GetBytes('WMP1')
$out = New-Object Collections.Generic.List[byte]
$out.AddRange($magic); $out.AddRange($plain); $out.AddRange([Text.Encoding]::ASCII.GetBytes($mac))
[IO.File]::WriteAllBytes($Out, $out.ToArray())
Write-Host "Wrote $Out ($($out.Count) bytes)"
