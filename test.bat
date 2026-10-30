@echo off
chcp 65001 >nul
setlocal
title TokenCat — тесты
set "PATH=%USERPROFILE%\.cargo\bin;%ProgramFiles%\nodejs;%PATH%"
set "ROOT=%~dp0"
where cargo >nul 2>&1 || (echo [!] Rust не найден. Запустите install-tools.bat & pause & exit /b 1)
cd /d "%ROOT%app\src-tauri"
echo Запускаю юнит-тесты TokenCat...
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%tools\run-logged.ps1" -Log "%ROOT%logs\test.log" -Cmd "cargo test --color never"
if errorlevel 1 (
  echo.
  echo [!] Тесты упали. Лог: logs\test.log — напишите Claude: "тесты упали".
  pause
  exit /b 1
)
rem Тесты расчётов интерфейса (B9) — на Node, если он установлен
where node >nul 2>&1 && (
  echo.
  echo Запускаю UI-тесты...
  powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%tools\run-logged.ps1" -Log "%ROOT%logs\test-ui.log" -Cmd "node ..\..\tools\test-ui.js"
  if errorlevel 1 (
    echo.
    echo [!] UI-тесты упали. Лог: logs\test-ui.log — напишите Claude: "тесты упали".
    pause
    exit /b 1
  )
)
echo.
echo Все тесты прошли. Лог: logs\test.log
timeout /t 8
