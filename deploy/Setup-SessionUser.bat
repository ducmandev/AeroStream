@echo off
setlocal
title AeroStream - Session Mode User Setup
echo ========================================================
echo        AeroStream - Session Mode Secondary User Setup
echo ========================================================
echo.

:: Kiem tra quyen Administrator
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [!] Can quyen Administrator de tao tai khoan phu Remote Desktop.
    echo Dang yeu cau cap quyen Administrator (UAC)...
    powershell -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

set USERNAME=aerostream_remote
set PASSWORD=AeroStream#2026

echo [+] Dang kiem tra tai khoan %USERNAME%...
net user %USERNAME% >nul 2>&1
if %errorlevel% neq 0 (
    echo [+] Dang tao tai khoan nguoi dung phu: %USERNAME%...
    net user %USERNAME% %PASSWORD% /add /comment:"AeroStream Isolated Session Mode User" /passwordchg:no
    if %errorlevel% neq 0 (
        echo [!] Khong the tao tai khoan %USERNAME%.
        pause
        exit /b 1
    )
    echo [OK] Da tao tai khoan %USERNAME% thanh cong.
) else (
    echo [OK] Tai khoan %USERNAME% da ton tai san.
)

echo.
echo [+] Dang cap quyen Remote Desktop cho %USERNAME%...
net localgroup "Remote Desktop Users" %USERNAME% /add >nul 2>&1
echo [OK] Da them %USERNAME% vao nhom 'Remote Desktop Users'.

echo.
echo [+] Dang kiem tra va kich hoat Remote Desktop Host tren may...
reg add "HKLM\SYSTEM\CurrentControlSet\Control\Terminal Server" /v fDenyTSConnections /t REG_DWORD /d 0 /f >nul 2>&1
sc config TermService start= auto >nul 2>&1
net start TermService >nul 2>&1

echo [+] Dang kiem tra Firewall cho cong RDP 3389...
netsh advfirewall firewall delete rule name="AeroStream Remote Desktop (Port 3389)" >nul 2>&1
netsh advfirewall firewall add rule name="AeroStream Remote Desktop (Port 3389)" dir=in action=allow protocol=TCP localport=3389 profile=any >nul 2>&1

echo.
echo ========================================================
echo [THANH CONG] Thiet lap Session Mode hoan tat!
echo.
echo Thong tin tai khoan phu:
echo   - Ten dang nhap: %USERNAME%
echo   - Mat khau:      %PASSWORD%
echo.
echo Ban co the su dung thong tin nay de luu vao AeroStream Engine
echo qua API POST /api/session/config hoac tren giao dien Web.
echo ========================================================
echo.
pause
