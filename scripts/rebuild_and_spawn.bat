@echo off
echo ========================================================
echo 1. Stopping any old aerostream.exe processes (UAC)...
echo ========================================================
powershell -NoProfile -Command "Start-Process cmd -ArgumentList '/c taskkill /F /IM aerostream.exe' -Verb RunAs -Wait"
timeout /t 2 /nobreak >nul

echo.
echo ========================================================
echo 2. Building fresh release with SendInput worker...
echo ========================================================
cd /d "%~dp0.."
cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Build failed! Check cargo errors above.
    pause
    exit /b 1
)

echo.
echo ========================================================
echo 3. Synchronizing release binaries to deploy/...
echo ========================================================
if not exist "%~dp0..\deploy" mkdir "%~dp0..\deploy"
copy /y "%~dp0..\target\release\aerostream.exe" "%~dp0..\deploy\aerostream.exe"
if exist "%~dp0..\aerostream.exe.manifest" copy /y "%~dp0..\aerostream.exe.manifest" "%~dp0..\deploy\aerostream.exe.manifest"

echo.
echo ========================================================
echo 4. Spawning new Secure Agent (SYSTEM) into session...
echo ========================================================
powershell -NoProfile -Command "Start-Process -FilePath '%~dp0..\target\release\aerostream.exe' -ArgumentList '--spawn-secure-agent' -Verb RunAs -Wait"
timeout /t 2 /nobreak >nul

echo.
echo ========================================================
echo 5. Verifying running Secure Agent...
echo ========================================================
powershell -ExecutionPolicy Bypass -File "%~dp0test_secure_agent_worker.ps1"
echo.
pause
