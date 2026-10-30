@echo off
chcp 65001 >nul
setlocal
title TokenCat — замеры лёгкости
set "PATH=%USERPROFILE%\.cargo\bin;%ProgramFiles%\nodejs;%ProgramFiles%\Git\cmd;%PATH%"
set "ROOT=%~dp0"
where cargo >nul 2>&1 || (echo [!] Rust не найден. Запустите install-tools.bat & pause & exit /b 1)
where npm >nul 2>&1 || (echo [!] Node.js не найден. Запустите install-tools.bat & pause & exit /b 1)

rem Релизная сборка (как build.bat, но без паузы), потом замеры. Результат: logs\perf.txt
taskkill /im tokencat.exe /f >nul 2>&1
cd /d "%ROOT%app"
if not exist node_modules\@tauri-apps\cli (
  echo Ставлю сборщик Tauri ^(один раз^)...
  powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%tools\run-logged.ps1" -Log "%ROOT%logs\npm-install.log" -Cmd "npm install --no-audit --no-fund"
  if errorlevel 1 (echo [!] npm install упал. Лог: logs\npm-install.log & pause & exit /b 1)
)
echo Собираю релиз и установщик. Несколько минут...
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%tools\run-logged.ps1" -Log "%ROOT%logs\build.log" -Cmd "npx tauri build"
if errorlevel 1 (
  echo.
  echo [!] Сборка упала. Лог: logs\build.log — напишите Claude: "сборка упала".
  pause
  exit /b 1
)
if not exist "%ROOT%dist" mkdir "%ROOT%dist"
copy /y "%ROOT%app\src-tauri\target\release\tokencat.exe" "%ROOT%dist\TokenCat-portable.exe" >nul
copy /y "%ROOT%app\src-tauri\target\release\bundle\nsis\*-setup.exe" "%ROOT%dist\" >nul

echo.
echo Замеры (около 2 минут, окно не закрывать)...
powershell -NoProfile -ExecutionPolicy Bypass -File "%ROOT%tools\perf.ps1" -Root "%ROOT%."
echo.
echo Готово. Результат: logs\perf.txt
timeout /t 8
