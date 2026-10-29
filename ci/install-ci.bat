@echo off
chcp 65001 >nul
rem Кладёт настройку сборки GitHub Actions (ci\build.yml) в .github\workflows\.
rem Claude не может сам писать в папку .github (она защищена), поэтому копирует пользователь.
cd /d "%~dp0.."
if not exist ".github\workflows" mkdir ".github\workflows"
copy /y "ci\build.yml" ".github\workflows\build.yml" >nul && echo Готово: .github\workflows\build.yml обновлён. || echo [!] Не удалось скопировать.
timeout /t 5
