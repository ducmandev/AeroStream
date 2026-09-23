@echo off
echo ========================================================
echo Spawning AeroStream Secure Agent as SYSTEM in session...
echo ========================================================
powershell -NoProfile -Command "Start-Process -FilePath '%~dp0..\target\release\aerostream.exe' -ArgumentList '--spawn-secure-agent' -Verb RunAs -Wait"
echo.
echo Process check:
powershell -NoProfile -ExecutionPolicy Bypass -Command "& { $procs = Get-Process aerostream -ErrorAction SilentlyContinue; if ($procs) { Write-Host 'Secure Agent is RUNNING (PIDs: ' ($procs.Id -join ', ') ')' -ForegroundColor Green } else { Write-Host 'Secure Agent is NOT running.' -ForegroundColor Red } }"
echo.
pause
