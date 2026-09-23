@echo off
setlocal
title AeroStream Host Server
cd /d "%~dp0"

:: Check for Administrator privileges
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [!] Requesting Administrator elevation to run AeroStream Host...
    powershell -NoProfile -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

echo ========================================================
echo               AeroStream Host Server
echo ========================================================
echo.
echo Host Directory: %~dp0
echo Starting aerostream.exe on port 8080...
echo.

"%~dp0aerostream.exe"

pause
