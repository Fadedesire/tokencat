@echo off
chcp 65001 >nul
setlocal
title TokenCat — сохранить в git
set "PATH=%ProgramFiles%\Git\cmd;%PATH%"
set "ROOT=%~dp0"
cd /d "%ROOT%"
set "OUT=%ROOT%logs\git-sync.log"
if not exist logs mkdir logs
where git >nul 2>&1 || (echo [!] Git не найден. Запустите install-tools.bat & pause & exit /b 1)

> "%OUT%" echo TokenCat git-sync %DATE% %TIME%

rem 1. Репозиторий (один раз)
if not exist ".git" (
  git init -b main >>"%OUT%" 2>&1
)
rem Имя и почта для коммитов: если глобально не заданы, ставим для этого проекта
git config user.name >nul 2>&1 || git config user.name "Roma" >>"%OUT%" 2>&1
git config user.email >nul 2>&1 || git config user.email "roma@users.noreply.github.com" >>"%OUT%" 2>&1
git config core.autocrlf false
rem Если F: не NTFS, git считает папку чужой - разрешаем её явно
git rev-parse --is-inside-work-tree >nul 2>&1 || git config --global --add safe.directory "%CD:\=/%" >>"%OUT%" 2>&1

rem 2. Адрес GitHub-репозитория берём из tools\remote.txt (одна строка)
if exist "tools\remote.txt" (
  set /p REMOTE=<"tools\remote.txt"
)
if defined REMOTE (
  git remote get-url origin >nul 2>&1 && git remote set-url origin "%REMOTE%" >>"%OUT%" 2>&1 || git remote add origin "%REMOTE%" >>"%OUT%" 2>&1
)

rem 3. Коммит (текст из tools\commit-msg.txt, иначе стандартный)
git add -A >>"%OUT%" 2>&1
git diff --cached --quiet
if errorlevel 1 (
  if exist "tools\commit-msg.txt" (
    git commit -F "tools\commit-msg.txt" >>"%OUT%" 2>&1
  ) else (
    git commit -m "Сохранение %DATE% %TIME%" >>"%OUT%" 2>&1
  )
) else (
  >>"%OUT%" echo Нечего коммитить.
)

rem 4. Push, если есть удалённый репозиторий. В первый раз Git откроет окно входа в GitHub.
git remote get-url origin >nul 2>&1
if not errorlevel 1 (
  echo Отправляю на GitHub. Если откроется окно входа — войдите в GitHub.
  git push -u origin main >>"%OUT%" 2>&1
  if errorlevel 1 (>>"%OUT%" echo PUSH FAILED) else (>>"%OUT%" echo PUSH OK)
) else (
  >>"%OUT%" echo Нет origin: push пропущен.
)
>>"%OUT%" echo --- status:
git status --short --branch >>"%OUT%" 2>&1
>>"%OUT%" echo --- log:
git log --oneline -5 >>"%OUT%" 2>&1
type "%OUT%"
echo.
echo Отчёт: logs\git-sync.log
timeout /t 8
