@echo off
chcp 65001 >nul
setlocal
title TokenCat — спайк S02
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0src-tauri"
where cargo >nul 2>&1 || (echo [!] Rust не найден. Сначала запустите F:\LimitCat\install-tools.bat & pause & exit /b 1)

echo Собираю спайк. Первый раз 3-10 минут (качаются библиотеки), потом секунды...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -Command "cmd /c 'cargo build --color never 2>&1' | Tee-Object -FilePath '%~dp0build-log.txt'; exit $LASTEXITCODE"
if errorlevel 1 (
  echo.
  echo [!] Сборка упала. Лог сохранён в app\spike\build-log.txt
  echo     Просто напишите Claude: "сборка упала".
  pause
  exit /b 1
)
echo.
echo Готово, запускаю окно спайка...
start "" "%~dp0src-tauri\target\debug\tokencat-spike.exe"
