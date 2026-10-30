@echo off
chcp 65001 >nul
title TokenCat — выпустить версию (тег)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0release.ps1"
echo.
echo Отчёт: logs\release.log
timeout /t 10
