@echo off
chcp 65001 >nul
title TokenCat — приватный ключ обновлений в буфер обмена
rem Кладёт приватный ключ в буфер обмена и открывает страницу секретов GitHub.
rem Там: Name = TAURI_SIGNING_PRIVATE_KEY, Secret = Ctrl+V, Add secret.
set "KEY=%USERPROFILE%\.tauri\tokencat-updater.key"
if not exist "%KEY%" (echo [!] Ключа нет. Сначала tools\updater-keys.bat & pause & exit /b 1)
type "%KEY%" | clip
echo Ключ скопирован в буфер обмена. Открываю GitHub...
start "" "https://github.com/Fadedesire/tokencat/settings/secrets/actions/new"
echo.
echo Name:   TAURI_SIGNING_PRIVATE_KEY
echo Secret: Ctrl+V
echo Потом нажмите Add secret. После этого буфер лучше очистить: скопируйте любой текст.
pause
