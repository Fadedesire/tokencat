@echo off
rem Снимки окон TokenCat через 6 секунд (успей открыть нужное окно). Итог: marketing\raw
if not "%~1"=="min" ( start "" /min "%~f0" min & exit /b )
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0shots.ps1" -Delay 6
exit
