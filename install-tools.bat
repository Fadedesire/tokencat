@echo off
chcp 65001 >nul
title TokenCat — установка инструментов
echo ============================================================
echo  TokenCat: ставлю инструменты для сборки (один раз)
echo  Rust, Visual Studio Build Tools (C++), Node.js LTS, Git
echo.
echo  Windows будет спрашивать "Разрешить изменения?" — жмите "Да".
echo  Build Tools качаются долго (несколько ГБ), это нормально.
echo ============================================================
echo.
where winget >nul 2>&1 || (echo [!] Не найден winget. Обновите "Установщик приложений" в Microsoft Store и запустите снова. & pause & exit /b 1)

set "W=winget install -e --accept-source-agreements --accept-package-agreements --silent"

echo [1/4] Visual Studio Build Tools (C++)...
%W% --id Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

echo.
echo [2/4] Rust...
%W% --id Rustlang.Rustup
if exist "%USERPROFILE%\.cargo\bin\rustup.exe" "%USERPROFILE%\.cargo\bin\rustup.exe" default stable-msvc

echo.
echo [3/4] Node.js LTS...
%W% --id OpenJS.NodeJS.LTS

echo.
echo [4/4] Git...
%W% --id Git.Git

echo.
echo ============================================================
echo  Готово. Теперь дважды кликните check-tools.bat
echo  (если какой-то пункт выше написал "уже установлено" — это ок).
echo ============================================================
pause
