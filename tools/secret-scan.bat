@echo off
chcp 65001 >nul
title TokenCat — проверка истории git на секреты
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0secret-scan.ps1"
echo.
echo Отчёт: logs\secret-scan.log
timeout /t 8
