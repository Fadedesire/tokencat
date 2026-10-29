@echo off
cd /d "%~dp0"
if exist "dist\ClaudeLimits.exe" (start "" "dist\ClaudeLimits.exe" & exit /b 0)
py -m pip install --quiet cryptography curl_cffi
start "" pyw claude_limits.pyw
