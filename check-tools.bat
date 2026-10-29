@echo off
chcp 65001 >nul
setlocal
set "PATH=%USERPROFILE%\.cargo\bin;%ProgramFiles%\nodejs;%ProgramFiles%\Git\cmd;%PATH%"
set "OUT=%~dp0tools-report.txt"
> "%OUT%" echo TokenCat tools report %DATE% %TIME%
ver >>"%OUT%"
>>"%OUT%" echo --- rustc:
rustc -V >>"%OUT%" 2>&1 || >>"%OUT%" echo NOT FOUND
>>"%OUT%" echo --- cargo:
cargo -V >>"%OUT%" 2>&1 || >>"%OUT%" echo NOT FOUND
>>"%OUT%" echo --- rustup toolchain:
rustup show active-toolchain >>"%OUT%" 2>&1 || >>"%OUT%" echo NOT FOUND
>>"%OUT%" echo --- node:
node -v >>"%OUT%" 2>&1 || >>"%OUT%" echo NOT FOUND
>>"%OUT%" echo --- npm:
call npm -v >>"%OUT%" 2>&1 || >>"%OUT%" echo NOT FOUND
>>"%OUT%" echo --- git:
git --version >>"%OUT%" 2>&1 || >>"%OUT%" echo NOT FOUND
>>"%OUT%" echo --- MSVC C++ tools:
set "VSW=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "%VSW%" ("%VSW%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath >>"%OUT%" 2>&1) else (>>"%OUT%" echo NOT FOUND)
>>"%OUT%" echo --- WebView2:
reg query "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" /v pv >>"%OUT%" 2>&1
type "%OUT%"
echo.
echo Отчёт сохранён в tools-report.txt
pause
