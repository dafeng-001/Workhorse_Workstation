# Generate WM1 offline license. ASCII-only.
# Usage: powershell -File scripts\make-license.ps1 [-Machine <id>] [-Exp 2027-12-31] [-Tier pro]
# Machine empty = any machine. Machine id shown in app Settings.
param([string]$Machine = '', [string]$Exp = '', [string]$Tier = 'pro')

function Get-Fnv($bytes) {
  $h = [UInt64]0xcbf29ce484222325
  foreach ($b in $bytes) {
    $h = $h -bxor [UInt64]$b
    $h = [UInt64]($h * [UInt64]0x100000001b3)
  }
  $h
}
$Secret = [Text.Encoding]::UTF8.GetBytes('wm-workhorse-license-v1-2026')
$payload = '{"m":"' + $Machine + '","exp":"' + $Exp + '","tier":"' + $Tier + '"}'
$pb = [Text.Encoding]::UTF8.GetBytes($payload)
$b64 = [Convert]::ToBase64String($pb)
function Mix($a,$b) {
  $buf = New-Object Collections.Generic.List[byte]
  $buf.AddRange($a); $buf.AddRange($b)
  (Get-Fnv $buf.ToArray())
}
$a = Mix $Secret $pb
$b2 = Mix $pb $Secret
$sig = ('{0:x16}{1:x16}' -f $a, $b2)
Write-Output ('WM1.' + $b64 + '.' + $sig)
