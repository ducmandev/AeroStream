@echo off
setlocal
title AeroStream - Remove Firewall Rule
echo Kiem tra quyen Administrator...
net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)
echo Dang xoa luat Firewall port 8080...
netsh advfirewall firewall delete rule name="AeroStream Remote Desktop (Port 8080)" >nul 2>&1
echo [XONG] Da xoa luat tuong lua.
pause
