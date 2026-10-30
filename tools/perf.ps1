# TokenCat — замеры лёгкости (B3 п. 2). Запускается из perf.bat после релизной сборки.
# Пишет результат в logs\perf.txt (UTF-8). Ничего не отправляет в сеть сам.
param([string]$Root = (Split-Path $PSScriptRoot -Parent), [int]$CpuSeconds = 60)
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8

$out = Join-Path $Root 'logs\perf.txt'
New-Item -ItemType Directory -Force -Path (Split-Path $out) | Out-Null
$w = New-Object IO.StreamWriter($out, $false, (New-Object Text.UTF8Encoding($false)))
$w.AutoFlush = $true
function Say($s) { Write-Host $s; $w.WriteLine($s) }
function MB($b) { '{0:N2} МБ' -f ($b / 1MB) }

Say "TokenCat perf $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Say "ПК: $((Get-CimInstance Win32_OperatingSystem).Caption), ядер $([Environment]::ProcessorCount)"

# --- 1. Размеры файлов ---
$exe = Join-Path $Root 'app\src-tauri\target\release\tokencat.exe'
$setup = Get-ChildItem (Join-Path $Root 'app\src-tauri\target\release\bundle\nsis') -Filter '*-setup.exe' -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending | Select-Object -First 1
Say ''
Say '[размер]'
if (Test-Path $exe) { Say "portable exe: $(MB (Get-Item $exe).Length)" } else { Say 'portable exe: НЕТ (сборка не прошла?)'; $w.Close(); exit 1 }
if ($setup) { Say "установщик:   $(MB $setup.Length)  ($($setup.Name))" } else { Say 'установщик: НЕТ' }

# --- помощники ---
# Лог программы: берём самый свежий tokencat.log (у профилей могут быть свои папки)
function LogFiles {
  Get-ChildItem (Join-Path $env:LOCALAPPDATA 'app.tokencat.desktop') -Recurse -Filter 'tokencat.log' -ErrorAction SilentlyContinue
}
function PerfLines($since) {
  $(foreach ($f in LogFiles) {
    Get-Content $f.FullName -Encoding UTF8 -ErrorAction SilentlyContinue | Where-Object { $_ -match 'perf: ' } |
      ForEach-Object { [pscustomobject]@{ Line = $_; File = $f.FullName } }
  }) | Where-Object { $_.Line -ge $since }
}
# Процесс TokenCat + все его потомки (WebView2 запускает целое дерево msedgewebview2)
function Tree($rootPid) {
  $all = Get-CimInstance Win32_Process
  $ids = @($rootPid); $added = $true
  while ($added) {
    $added = $false
    foreach ($p in $all) {
      if (($ids -contains $p.ParentProcessId) -and -not ($ids -contains $p.ProcessId)) { $ids += $p.ProcessId; $added = $true }
    }
  }
  Get-Process -Id $ids -ErrorAction SilentlyContinue
}
function StopAll { Get-Process tokencat -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 800 }

# Один запуск: время до первых цифр (по строке в логе программы)
function Launch($label) {
  StopAll
  # Метка времени в том же формате, что пишет applog (строки сравниваются как текст)
  $since = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd HH:mm:ss')
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $p = Start-Process $exe -PassThru
  $line = $null
  while ($sw.Elapsed.TotalSeconds -lt 40 -and -not $line) {
    Start-Sleep -Milliseconds 100
    $line = PerfLines $since | Where-Object { $_.Line -match 'первые цифры' } | Select-Object -Last 1
  }
  Start-Sleep -Milliseconds 300
  foreach ($c in @(PerfLines $since | Where-Object { $_.Line -match 'кеша' })) { Say "$label`: $($c.Line -replace '^.*perf: ', '')" }
  if ($line) {
    $ms = [regex]::Match($line.Line, 'через (\d+) мс').Groups[1].Value
    Say "$label`: первые цифры через $ms мс (по логу программы)"
  } else {
    Say "$label`: строки «первые цифры» нет за 40 с (не вошли? нет VPN?)"
  }
  return $p
}

# --- 2. Старт ---
Say ''
Say '[старт] (первый запуск после сборки ≈ холодный, второй — тёплый; сеть входит в цифру)'
$null = Launch 'запуск 1'
$p = Launch 'запуск 2'

# --- 3. Память и CPU в простое ---
Say ''
Say '[простой] жду 15 с, пока всё успокоится...'
Start-Sleep 15
$procs = Tree $p.Id
$ws = ($procs | Measure-Object WorkingSet64 -Sum).Sum
$pv = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum
Say "процессов: $($procs.Count) ($(($procs | Group-Object ProcessName | ForEach-Object { "$($_.Name)×$($_.Count)" }) -join ', '))"
Say "RAM (рабочий набор, сумма): $(MB $ws)"
Say "RAM (частная память, сумма): $(MB $pv)"
foreach ($x in $procs | Sort-Object WorkingSet64 -Descending) { Say ("  {0,-16} pid {1,-6} {2}" -f $x.ProcessName, $x.Id, (MB $x.WorkingSet64)) }

$t0 = @{}; foreach ($x in $procs) { $t0[$x.Id] = $x.TotalProcessorTime.TotalMilliseconds }
Say "CPU: меряю $CpuSeconds с (панель как есть, котик идёт)..."
Start-Sleep $CpuSeconds
$cpu = 0
foreach ($x in Tree $p.Id) { if ($t0.ContainsKey($x.Id)) { $x.Refresh(); $cpu += $x.TotalProcessorTime.TotalMilliseconds - $t0[$x.Id] } }
$pct = $cpu / ($CpuSeconds * 1000) / [Environment]::ProcessorCount * 100
Say ("CPU в среднем: {0:N2} % от всего процессора ({1:N0} мс процессорного времени за {2} с)" -f $pct, $cpu, $CpuSeconds)

Say ''
Say 'Готово. TokenCat (релиз) оставлен запущенным.'
$w.Close()
