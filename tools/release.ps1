# Ставит тег v<версия> и отправляет его на GitHub → запускается ci/release.yml (черновик релиза).
# Версию берёт из app/src-tauri/tauri.conf.json. Отчёт: logs\release.log
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$env:PATH = "$env:ProgramFiles\Git\cmd;$env:PATH"
$root = Resolve-Path "$PSScriptRoot\.."
Set-Location $root
$log = Join-Path $root 'logs\release.log'
New-Item -ItemType Directory -Force -Path (Split-Path $log) | Out-Null
function Say($t) { Write-Host $t; Add-Content -Path $log -Value $t -Encoding UTF8 }
Set-Content -Path $log -Value "release $(Get-Date -Format s)" -Encoding UTF8
$v = (Get-Content app/src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version
$tag = "v$v"
Say "Версия: $v"
$dirty = git status --porcelain
if ($dirty) { Say "[!] Есть несохранённые изменения. Сначала git-sync.bat."; Say ($dirty -join "`n"); exit 1 }
git fetch --tags origin 2>&1 | Out-Null
if (git tag --list $tag) { Say "[!] Тег $tag уже есть. Нужна новая версия."; exit 1 }
git tag -a $tag -m "TokenCat $tag" 2>&1 | ForEach-Object { Say $_ }
git push origin $tag 2>&1 | ForEach-Object { Say $_ }
if ($LASTEXITCODE -ne 0) { Say "PUSH FAILED"; exit 1 }
Say "OK: $tag отправлен. Сборка: https://github.com/Fadedesire/tokencat/actions"
Start-Process "https://github.com/Fadedesire/tokencat/actions/workflows/release.yml"
