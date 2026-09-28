# Get NiuMa machine id
$py = Join-Path $PSScriptRoot "wm-license.py"
$cands = @("python", "py", "$env:MIMO_PYTHON")
foreach ($c in $cands) {
  if ($c -and (Get-Command $c -ErrorAction SilentlyContinue)) {
    & $c $py machine
    exit 0
  }
}
Write-Host "Need Python to run wm-license.py"
