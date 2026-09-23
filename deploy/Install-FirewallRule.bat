@echo off
setlocal
title AeroStream - Firewall Configuration
echo ========================================================
echo        AeroStream Remote Desktop - Firewall Setup
echo ========================================================
echo.

:: Kiem tra quyen Administrator
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [!] Can quyen Administrator de mo cong tuong lua (Firewall).
    echo Dang yeu cau cap quyen Administrator (UAC)...
    powershell -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

echo [+] Dang mo cong 8080 TCP tren Windows Defender Firewall...
netsh advfirewall firewall delete rule name="AeroStream Remote Desktop (Port 8080)" >nul 2>&1
netsh advfirewall firewall add rule name="AeroStream Remote Desktop (Port 8080)" dir=in action=allow protocol=TCP localport=8080 profile=any >nul 2>&1

if %errorlevel% equ 0 (
    echo.
    echo [THANH CONG] Da mo cong 8080 TCP thanh cong!
    echo Cac thiet bi Android, iPad, iPhone, Laptop khac trong cung mang Wi-Fi/LAN da co the ket noi.
) else (
    echo.
    echo [THAT BAI] Khong the cau hinh Firewall. Vui long chay lai bang chuot phai 'Run as administrator'.
)

echo.
echo ========================================================
pause
