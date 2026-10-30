@echo off
chcp 65001 >nul
rem Кладёт настройки GitHub Actions (ci\build.yml, ci\release.yml) в .github\workflows\.
rem Claude не может сам писать в папку .github (она защищена), поэтому копирует пользователь.
cd /d "%~dp0.."
if not exist ".github\workflows" mkdir ".github\workflows"
copy /y "ci\build.yml" ".github\workflows\build.yml" >nul && echo Готово: build.yml || echo [!] build.yml не скопирован.
copy /y "ci\release.yml" ".github\workflows\release.yml" >nul && echo Готово: release.yml || echo [!] release.yml не скопирован.
timeout /t 5
