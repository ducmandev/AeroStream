# ==============================================================================
# AeroStream - Automated Release Packaging & Distribution Script
# Usage: powershell -ExecutionPolicy Bypass -File scripts/build_bundle.ps1
# ==============================================================================

Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "       AeroStream Build & Distribution Packager         " -ForegroundColor Cyan
Write-Host "========================================================" -ForegroundColor Cyan

$WorkspaceDir = "D:\StreamApp"
$OutputDir = Join-Path $WorkspaceDir "AeroStream-Windows"
$ZipPackage = Join-Path $WorkspaceDir "AeroStream-Windows-v1.0.zip"
$TargetReleaseExe = Join-Path $WorkspaceDir "target\release\aerostream.exe"
$AndroidAppDir = Join-Path $WorkspaceDir "android_app"
$FlutterWindowsReleaseDir = Join-Path $AndroidAppDir "build\windows\x64\runner\Release"
$FlutterPath = "D:\flutter\bin\flutter.bat"
$JdkPath = "D:\Tools\jdk17"
$AndroidSdkPath = "D:\Android\Sdk"

# 1. Clean & Prepare Output Directory
Write-Host "`n[1/5] Preparing output directory: $OutputDir..." -ForegroundColor Yellow
if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
} else {
    # Remove stale logs
    Remove-Item (Join-Path $OutputDir "aerostream.log") -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $OutputDir "aerostream_debug.log") -Force -ErrorAction SilentlyContinue
}

# 2. Build Rust Engine (Release)
Write-Host "`n[2/5] Compiling Rust High-Performance Engine (Release)..." -ForegroundColor Yellow
Set-Location $WorkspaceDir
cargo build --release --bin aerostream
if ($LASTEXITCODE -ne 0) {
    Write-Host "[!] Cargo build failed with exit code $LASTEXITCODE" -ForegroundColor Red
    Exit 1
}
Write-Host "[+] Rust Engine compiled successfully." -ForegroundColor Green

# 3. Build Flutter Windows GUI (Release)
Write-Host "`n[3/5] Compiling Flutter Windows Desktop GUI (Release)..." -ForegroundColor Yellow
if (Test-Path $FlutterPath) {
    Set-Location $AndroidAppDir
    & $FlutterPath build windows --release
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[!] Flutter Windows build failed with exit code $LASTEXITCODE" -ForegroundColor Red
        Exit 1
    }
    Write-Host "[+] Flutter Windows Desktop compiled successfully." -ForegroundColor Green
} else {
    Write-Host "[!] Flutter SDK not found at $FlutterPath" -ForegroundColor Red
    Exit 1
}

# 4. Assemble Windows Standalone Distribution Bundle
Write-Host "`n[4/5] Assembling Windows standalone package in $OutputDir..." -ForegroundColor Yellow

# Copy Flutter Windows Application binaries (AeroStream.exe is the GUI Dashboard)
Copy-Item -Path (Join-Path $FlutterWindowsReleaseDir "android_app.exe") -Destination (Join-Path $OutputDir "AeroStream.exe") -Force
Copy-Item -Path (Join-Path $FlutterWindowsReleaseDir "flutter_windows.dll") -Destination (Join-Path $OutputDir "flutter_windows.dll") -Force
if (Test-Path (Join-Path $OutputDir "data")) {
    Remove-Item -Path (Join-Path $OutputDir "data") -Recurse -Force
}
Copy-Item -Path (Join-Path $FlutterWindowsReleaseDir "data") -Destination (Join-Path $OutputDir "data") -Recurse -Force

# Copy Rust Engine (aerostream_engine.exe is the high-performance backend)
Copy-Item -Path $TargetReleaseExe -Destination (Join-Path $OutputDir "aerostream_engine.exe") -Force

# Clean up any leftover duplicate files
Remove-Item (Join-Path $OutputDir "AeroStream.apk") -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $OutputDir "android_app.exe") -Force -ErrorAction SilentlyContinue

# Copy persistent TLS certificates if present
if (Test-Path (Join-Path $WorkspaceDir "cert.pem")) {
    Copy-Item -Path (Join-Path $WorkspaceDir "cert.pem") -Destination (Join-Path $OutputDir "cert.pem") -Force
}
if (Test-Path (Join-Path $WorkspaceDir "key.pem")) {
    Copy-Item -Path (Join-Path $WorkspaceDir "key.pem") -Destination (Join-Path $OutputDir "key.pem") -Force
}

# Copy Android APK
$sourceApk = Join-Path $AndroidAppDir "build\app\outputs\flutter-apk\app-release.apk"
if (Test-Path $sourceApk) {
    Copy-Item -Path $sourceApk -Destination (Join-Path $OutputDir "AeroStream-Android.apk") -Force
    Copy-Item -Path $sourceApk -Destination (Join-Path $WorkspaceDir "AeroStream-Android.apk") -Force
}

# Create Launcher & Helper Scripts
$startBat = Join-Path $OutputDir "Start-AeroStream.bat"
@"
@echo off
cd /d "%~dp0"
title AeroStream Remote Desktop Launcher
echo Dang khoi chay AeroStream Remote Desktop...
if exist "AeroStream.exe" (
    start "" "AeroStream.exe"
) else if exist "aerostream_engine.exe" (
    start "" "aerostream_engine.exe"
) else (
    echo [ERROR] Khong tim thay AeroStream.exe!
    pause
)
"@ | Set-Content -Path $startBat -Encoding ASCII

$firewallBat = Join-Path $OutputDir "Install-FirewallRule.bat"
@"
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
"@ | Set-Content -Path $firewallBat -Encoding ASCII

$uninstallFirewallBat = Join-Path $OutputDir "Uninstall-FirewallRule.bat"
@"
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
"@ | Set-Content -Path $uninstallFirewallBat -Encoding ASCII

# Create Instructions file (Vietnamese)
$docFile = Join-Path $OutputDir "HUONG_DAN_SU_DUNG.txt"
@"
================================================================================
           AEROSTREAM REMOTE DESKTOP - HUONG DAN SU DUNG CHO MAY KHAC
================================================================================

1. GIOI THIEU:
   AeroStream la giai phap dieu khien may tinh tu xa sieu nhanh (Ultra-low latency):
   - Truyen hinh anh bang phan cung GPU (Intel QSV / DXGI) do tre ~12ms.
   - Truyen am thanh thuc (WASAPI Loopback + Opus 48kHz Stereo) truc tiep.
   - Giao dien Fluent Windows 11 Light sang trong, Dynamic Island thong minh.
   - Ho tro day du chuot, ban phim, go tieng Viet IME, phong to/thu nho man hinh.

2. CACH CHAY TREN MAY TINH WINDOWS KHAC:
   - Buoc 1: Giai nen file 'AeroStream-Windows-v1.0.zip' vao bat ky thu muc nao
             (Vi du: C:\AeroStream hoac Desktop).
   - Buoc 2: Nhap chuot phai vao file 'Install-FirewallRule.bat' -> chon 'Run as administrator'
             (Chi can lam 1 lan dau tien de cho phep cac may khac trong LAN ket noi qua port 8080).
   - Buoc 3: Nhap dup chuot vao file 'AeroStream.exe' (hoac chay 'Start-AeroStream.bat').
             Man hinh Dashboard Windows se hien thi:
             + Dia chi IP noi bo (vi du: 192.168.1.15)
             + Port: 8080
             + Ma PIN 6 chu so bao mat (tu dong tao ngau nhien moi lan chay)
             + Ma QR Code de quet nhanh

3. KET NOI TU DIEN THOAI ANDROID:
   - Cai dat file 'AeroStream-Android.apk' (co san trong thu muc nay hoac chia se qua Zalo/Drive).
   - Mo ung dung AeroStream tren dien thoai.
   - Quet ma QR tren man hinh PC, hoac nhap IP va ma PIN roi bam 'Ket noi'.
   - Cac tinh nang tren dien thoai:
     + Thanh Dynamic Island: Cham de goi menu, hoac vuot cham nhe o mep tren/canh man hinh.
     + Trackpad chuot: Che do chuot sieu muot, ho tro chinh do nhay tu 1 - 100 trong Cài đặt.
     + Phim tat Windows: Win, Alt+Tab, Ctrl+C, Ctrl+V, Task Manager, Mui ten...
     + Phim IME: Cho phep chuyen doi bo go va nhap tieng Viet de dang.
     + Am thanh: Bat/tat truyen am thanh PC ve dien thoai tuc thi.
     + Zoom man hinh: Dung 2 ngon tay de phong to / thu nho / di chuyen man hinh.

4. KET NOI TU TRINH DUYET WEB (iPhone / iPad / Mac / PC khac):
   - Khong can cai dat bat ky ung dung nao!
   - Tren thiet bi khac cung mang Wi-Fi, mo Chrome, Safari hoac Edge.
   - Nhap dia chi: http://<IP-MAY-TINH>:8080/?pin=<MA-PIN>
     (Vi du: http://192.168.1.15:8080/?pin=123456)
   - Trinh duyet se tu dong stream man hinh 60fps va am thanh truc tiep.

5. XU LY SU CO (TROUBLESHOOTING):
   - Dien thoai bao khong ket noi duoc:
     + Kiem tra dien thoai va may tinh co dang ket noi chung 1 mang Wi-Fi hay khong.
     + Chay lai file 'Install-FirewallRule.bat' bang quyen Administrator.
   - Khong nghe thay tieng tren dien thoai / web:
     + Kiem tra may tinh dang phat nhac/video qua loa mac dinh.
     + Bam vao bieu tuong Loa tren thanh Dynamic Island de bat am thanh.
   - Muon tat may chu:
     + Dong cua so AeroStream tren may tinh.

================================================================================
Chuc ban co trai nghiem dieu khien may tinh tuyet voi cung AeroStream!
"@ | Set-Content -Path $docFile -Encoding UTF8

Write-Host "[+] Assembled release files in $OutputDir." -ForegroundColor Green

# 5. Compress to Standalone ZIP Package
Write-Host "`n[5/5] Compressing standalone distribution ZIP: $ZipPackage..." -ForegroundColor Yellow
if (Test-Path $ZipPackage) {
    Remove-Item $ZipPackage -Force
}

Compress-Archive -Path "$OutputDir\*" -DestinationPath $ZipPackage -CompressionLevel Optimal -Force
$zipInfo = Get-Item $ZipPackage
Write-Host "[+] Generated $ZipPackage ($([math]::Round($zipInfo.Length / 1MB, 2)) MB)" -ForegroundColor Green

Set-Location $WorkspaceDir
Write-Host "`n=== All Tasks Completed Successfully! ===" -ForegroundColor Cyan
Get-ChildItem $OutputDir | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
