# WM license / update-pack tool (ASCII). No server required.
# Usage:
#   powershell -File scripts\wm-license.ps1 machine
#   powershell -File scripts\wm-license.ps1 license [-Machine <id>] [-Exp 2027-12-31] [-Tier pro]
#   powershell -File scripts\wm-license.ps1 key -Code WM1... -Machine <id>
#   powershell -File scripts\wm-license.ps1 pack -Code WM1... -Machine <id> [-Exe path] [-Out update.wmp]

param(
  [Parameter(Mandatory=$true, Position=0)][string]$Cmd,
  [string]$Machine = '',
  [string]$Exp = '',
  [string]$Tier = 'pro',
  [string]$Code = '',
  [string]$Exe = '',
  [string]$Out = ''
)
$ErrorActionPreference = 'Stop'
$Secret = [Text.Encoding]::UTF8.GetBytes('wm-workhorse-license-v1-2026')

function Get-Fnv([byte[]]$bytes) {
  $h = [UInt64]0xcbf29ce484222325
  foreach ($b in $bytes) { $h = $h -bxor [UInt64]$b; $h = [UInt64]($h * [UInt64]0x100000001b3) }
  $h
}
function HmacLike([byte[]]$msg) {
  $a = New-Object Collections.Generic.List[byte]; $a.AddRange($Secret); $a.AddRange($msg)
  $b = New-Object Collections.Generic.List[byte]; $b.AddRange($msg); $b.AddRange($Secret)
  return ('{0:x16}{1:x16}' -f (Get-Fnv $a.ToArray()), (Get-Fnv $b.ToArray()))
}
function Get-MachineId {
  $hostn = $env:COMPUTERNAME
  $user = $env:USERNAME
  $raw = (($hostn.ToLower().Trim()) + '|' + ($user.ToLower().Trim()) + '|wm')
  return ('{0:x16}' -f (Get-Fnv ([Text.Encoding]::UTF8.GetBytes($raw))))
}
function Get-UpdateKey([string]$licCode, [string]$machine) {
  $seed = New-Object Collections.Generic.List[byte]
  $seed.AddRange($Secret)
  $seed.AddRange([Text.Encoding]::UTF8.GetBytes($licCode))
  $seed.AddRange([Text.Encoding]::UTF8.GetBytes($machine))
  $key = New-Object byte[] 32
  for ($i=0; $i -lt 4; $i++) {
    $c = New-Object Collections.Generic.List[byte]
    $c.AddRange($seed.ToArray()); $c.Add([byte]$i)
    $h = HmacLike $c.ToArray()
    $hb = [Text.Encoding]::UTF8.GetBytes($h)
    for ($j=0; $j -lt 8; $j++) {
      $x = $hb[$j]
      $y = if ($j+8 -lt $hb.Length) { $hb[$j+8] } else { 0 }
      $key[$i*8+$j] = $x -bxor $y
    }
  }
  return $key
}

switch ($Cmd.ToLower()) {
  'machine' {
    Write-Output (Get-MachineId)
  }
  'license' {
    $m = $Machine
    $payload = '{"m":"' + $m + '","exp":"' + $Exp + '","tier":"' + $Tier + '"}'
    $pb = [Text.Encoding]::UTF8.GetBytes($payload)
    $b64 = [Convert]::ToBase64String($pb)
    $sig = HmacLike $pb
    Write-Output ('WM1.' + $b64 + '.' + $sig)
  }
  'key' {
    if (-not $Code) { throw 'need -Code' }
    if (-not $Machine) { $Machine = Get-MachineId }
    $k = Get-UpdateKey $Code $Machine
    Write-Output ((($k | ForEach-Object { $_.ToString('x2') }) -join ''))
  }
  'pack' {
    if (-not $Code) { throw 'need -Code' }
    if (-not $Machine) { $Machine = Get-MachineId }
    $root = Split-Path -Parent $PSScriptRoot
    if (-not $Exe) { $Exe = Join-Path $root 'personal-workbench\src-tauri\target\release\personal-workbench.exe' }
    if (-not $Out) { $Out = Join-Path $root 'update.wmp' }
    $Exe = (Resolve-Path $Exe).Path
    $key = Get-UpdateKey $Code $Machine
    $plain = [byte[]]([IO.File]::ReadAllBytes($Exe)).Clone()
    for ($i=0; $i -lt $plain.Length; $i++) {
      $k = $key[$i % 32] -bxor ([byte]([int](($i / 32)) * 31))
      $plain[$i] = $plain[$i] -bxor $k
    }
    $chk = New-Object Collections.Generic.List[byte]
    $chk.AddRange($key); $chk.AddRange($plain)
    $mac = (HmacLike $chk.ToArray()).Substring(0,16)
    $out = New-Object Collections.Generic.List[byte]
    $out.AddRange([Text.Encoding]::ASCII.GetBytes('WMP1'))
    $out.AddRange($plain)
    $out.AddRange([Text.Encoding]::ASCII.GetBytes($mac))
    [IO.File]::WriteAllBytes($Out, $out.ToArray())
    Write-Host "pack: $Out ($($out.Count) bytes)"
    Write-Host "machine: $Machine"
  }
  default {
    Write-Host 'cmds: machine | license | key | pack'
  }
}
