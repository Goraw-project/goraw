@echo off
setlocal
where.exe pwsh.exe >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    pwsh.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0box.ps1" %*
    exit /b %ERRORLEVEL%
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0box.ps1" %*
exit /b %ERRORLEVEL%
