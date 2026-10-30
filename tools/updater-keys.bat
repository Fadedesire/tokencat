@echo off
chcp 65001 >nul
setlocal
title TokenCat — ключи подписи обновлений
rem Создаёт пару ключей для автообновления (B4). Приватный ключ остаётся ТОЛЬКО на этом ПК
rem (%USERPROFILE%\.tauri\tokencat-updater.key) и в GitHub Secrets; в репозиторий не попадает.
rem Публичный ключ копируется в tools\updater-pubkey.txt — его можно показывать всем.
set "PATH=%ProgramFiles%\nodejs;%PATH%"
set "ROOT=%~dp0.."
set "KEYDIR=%USERPROFILE%\.tauri"
set "KEY=%KEYDIR%\tokencat-updater.key"
if not exist "%ROOT%\logs" mkdir "%ROOT%\logs"
if exist "%KEY%" (
  echo Ключ уже есть: %KEY% — новый не создаю.
) else (
  if not exist "%KEYDIR%" mkdir "%KEYDIR%"
  cd /d "%ROOT%\app"
  call npx tauri signer generate --ci -w "%KEY%" > "%ROOT%\logs\updater-keys.log" 2>&1
)
if exist "%KEY%.pub" (
  copy /y "%KEY%.pub" "%ROOT%\tools\updater-pubkey.txt" >nul
  echo OK >> "%ROOT%\logs\updater-keys.log"
  echo Готово. Публичный ключ: tools\updater-pubkey.txt
) else (
  echo FAIL >> "%ROOT%\logs\updater-keys.log"
  echo [!] Не получилось. Лог: logs\updater-keys.log
)
timeout /t 8
