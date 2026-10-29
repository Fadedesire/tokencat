# Запускает команду, показывает вывод в окне и пишет его в лог в UTF-8.
# (Tee-Object в PowerShell 5 пишет UTF-16 — см. CONTEXT.md, грабли №6.)
param([Parameter(Mandatory)] [string]$Log, [Parameter(Mandatory)] [string]$Cmd)
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
New-Item -ItemType Directory -Force -Path (Split-Path $Log) | Out-Null
$w = New-Object IO.StreamWriter($Log, $false, (New-Object Text.UTF8Encoding($false)))
$w.AutoFlush = $true
try {
  & cmd /c "$Cmd 2>&1" | ForEach-Object { Write-Host $_; $w.WriteLine($_) }
  $code = $LASTEXITCODE
} finally { $w.Close() }
exit $code
