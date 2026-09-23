@echo off
setlocal
title AeroStream - Spawn Secure Agent
cd /d "%~dp0"

:: Check for Administrator privileges
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [!] Requesting Administrator elevation to spawn SYSTEM Secure Agent...
    powershell -NoProfile -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

echo ========================================================
echo        AeroStream - Spawn SYSTEM Secure Agent
echo ========================================================
echo.
echo Spawning agent into active interactive session...
"%~dp0aerostream.exe" --spawn-secure-agent
echo.
echo Checking running agent process:
powershell -NoProfile -ExecutionPolicy Bypass -Command "& { $procs = Get-Process aerostream -ErrorAction SilentlyContinue; if ($procs) { Write-Host 'Secure Agent is RUNNING (PIDs: ' ($procs.Id -join ', ') ')' -ForegroundColor Green } else { Write-Host 'Secure Agent is NOT running.' -ForegroundColor Red } }"
echo.
pause
