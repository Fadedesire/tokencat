@echo off
chcp 65001 >nul
setlocal
title TokenCat — запуск для разработки
set "PATH=%USERPROFILE%\.cargo\bin;%ProgramFiles%\nodejs;%ProgramFiles%\Git\cmd;%PATH%"
set "ROOT=%~dp0"
where cargo >nul 2>&1 || (echo [!] Rust не найден. Запустите install-tools.bat & pause & exit /b 1)

echo Собираю TokenCat (отладочная сборка). Первый раз несколько минут, потом секунды...
echo.
taskkill /im tokencat.exe /f >nul 2>&1
cd /d "%ROOT%app\src-tauri"
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%tools\run-logged.ps1" -Log "%ROOT%logs\dev-build.log" -Cmd "cargo build --color never"
if errorlevel 1 (
  echo.
  echo [!] Сборка упала. Лог: logs\dev-build.log
  echo     Напишите Claude: "сборка упала".
  pause
  exit /b 1
)
echo.
echo Готово, открываю окно TokenCat...
start "" "%ROOT%app\src-tauri\target\debug\tokencat.exe"
