# Ищет в ВСЕЙ истории git то, что нельзя показывать публично: ключи сессии claude.ai,
# почту, UUID организаций. Значения в отчёте маскируются. Отчёт: logs\secret-scan.log
param([string]$Root = (Resolve-Path "$PSScriptRoot\.."))
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$env:PATH = "$env:ProgramFiles\Git\cmd;$env:PATH"
Set-Location $Root
$log = Join-Path $Root 'logs\secret-scan.log'
New-Item -ItemType Directory -Force -Path (Split-Path $log) | Out-Null
$out = New-Object Collections.Generic.List[string]
$patterns = [ordered]@{
  'sk-ant'     = 'sk-ant-[A-Za-z0-9_\-]{8,}'
  'sessionKey' = 'sessionKey["'']?\s*[=:]\s*["'']?[A-Za-z0-9_\-]{16,}'
  'email'      = '[A-Za-z0-9._%+\-]+@(gmail|mail|yandex|outlook|hotmail|icloud)\.[a-z]{2,}'
  'uuid'       = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'
  'cf_clear'   = 'cf_clearance=[A-Za-z0-9_\-\.]{10,}'
}
function Mask([string]$s) { if ($s.Length -le 8) { return '****' } ; return $s.Substring(0,6) + '…' + $s.Substring($s.Length-3) }
$commit = ''; $file = ''; $hits = 0
git log -p --all --no-color --no-ext-diff 2>$null | ForEach-Object {
  $line = $_
  if ($line -like 'commit *') { $commit = $line.Substring(7, 8); return }
  if ($line -like '+++ b/*') { $file = $line.Substring(6); return }
  if (-not $line.StartsWith('+')) { return }
  foreach ($k in $patterns.Keys) {
    foreach ($m in [regex]::Matches($line, $patterns[$k])) {
      $hits++
      $out.Add(("{0} {1} {2}: {3}" -f $commit, $k, $file, (Mask $m.Value)))
    }
  }
}
$out.Insert(0, "secret-scan $(Get-Date -Format s) commits=$((git rev-list --all | Measure-Object).Count) hits=$hits")
$out.Add('--- tracked files > 1 MB:')
git ls-files -z | ForEach-Object { $_ -split "`0" } | Where-Object { $_ -and (Test-Path $_) -and ((Get-Item $_).Length -gt 1MB) } | ForEach-Object { $out.Add($_) }
$out.Add('--- all tracked files:')
git ls-files | ForEach-Object { $out.Add($_) }
[IO.File]::WriteAllLines($log, $out, (New-Object Text.UTF8Encoding($false)))
$out | Select-Object -First 40 | ForEach-Object { Write-Host $_ }
