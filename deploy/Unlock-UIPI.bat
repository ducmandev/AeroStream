@echo off
setlocal
title AeroStream - Mo Khoa UIPI & Cho Phep Nhap Mat Khau Tu Xa
color 0B

echo ================================================================================
echo          AEROSTREAM - MO KHOA UIPI & CHO PHEP NHAP MAT KHAU TU XA
echo ================================================================================
echo.

:: 1. Kiem tra quyen Administrator
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo [THONG BAO] Dang yeu cau quyen Administrator de mo khoa UIPI...
    powershell -Command  Start-Process %~f0 -Verb RunAs
    exit /b
)

echo [*] Quyen Administrator: DA XAC THUC.
echo.
echo [1/3] Dang mo khoa UIPI tren Windows...
:: Cho phep cac ung dung remote dieu khien cua so dac quyen cao ma khong bi chan boi Secure Desktop
reg add HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System /v PromptOnSecureDesktop /t REG_DWORD /d 0 /f >nul 2>&1
if %errorlevel% equ 0 (
    echo     + PromptOnSecureDesktop = 0 [THANH CONG]: Hop thoai mat khau/UAC se khong bi dong bang man hinh.
) else (
    echo     ! PromptOnSecureDesktop [THAT BAI]
)

echo [2/4] Dang bat che do ho tro Accessibility Desktop va SendSAS...
reg add HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System /v EnableUIADesktopToggle /t REG_DWORD /d 1 /f >nul 2>&1
reg add HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System /v EnableSecureUIAPaths /t REG_DWORD /d 0 /f >nul 2>&1
reg add HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System /v SoftwareSASGeneration /t REG_DWORD /d 3 /f >nul 2>&1
if %errorlevel% equ 0 (
    echo     + EnableUIADesktopToggle = 1, EnableSecureUIAPaths = 0, SoftwareSASGeneration = 3 [THANH CONG]: Ho tro dieu khien va danh thuc man hinh khoa.
) else (
    echo     ! Accessibility Desktop [THAT BAI]
)

echo [3/3] Dang cap nhat quyen thuc thi Administrator cho AeroStream...
reg add HKCU\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers /v %~dp0aerostream_engine.exe /d ~ RUNASADMIN /f >nul 2>&1
if exist %~dp0AeroStream.exe (
    reg add HKCU\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers /v %~dp0AeroStream.exe /d ~ RUNASADMIN /f >nul 2>&1
)
echo     + Luon chay AeroStream bang quyen Administrator [THANH CONG].

echo.
echo ================================================================================
echo [HOAN TAT] DA MO KHOA UIPI VA CHO PHEP NHAP MAT KHAU TU XA THANH CONG!
echo.
echo Tu bay gio:
echo - Khi may host o man hinh khoa (Lock Screen) hoac hop thoai mat khau,
echo   ung dung AeroStream tren dien thoai co the nhap mat khau/PIN binh thuong.
echo - Cua so Task Manager va cac ung dung Administrator khong con bi chan chuot/phim.
echo ================================================================================
echo.
pause
