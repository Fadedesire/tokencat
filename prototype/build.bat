@echo off
cd /d "%~dp0"
py -m pip install --upgrade cryptography curl_cffi pyinstaller || goto :err
taskkill /f /im ClaudeLimits.exe >nul 2>&1
timeout /t 1 /nobreak >nul
if exist dist\ClaudeLimits.exe del /f /q dist\ClaudeLimits.exe
py -m PyInstaller --noconsole --onefile --collect-all curl_cffi --name ClaudeLimits claude_limits.pyw || goto :err
echo.
echo Done: dist\ClaudeLimits.exe
start "" dist\ClaudeLimits.exe
exit /b 0
:err
echo Build failed. Is Python 3.10+ installed (python.org, "py" launcher)?
pause
exit /b 1
