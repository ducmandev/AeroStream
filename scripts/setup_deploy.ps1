# Setup Deploy Folder and Organize Builds
$ErrorActionPreference = "Stop"

$workspaceDir = "D:\StreamApp"
$deployDir = Join-Path $workspaceDir "deploy"

Write-Host "Creating deploy directory: $deployDir" -ForegroundColor Cyan
if (-not (Test-Path $deployDir)) {
    New-Item -ItemType Directory -Path $deployDir -Force | Out-Null
}

# 1. Copy latest compiled aerostream.exe from target\release
$releaseExe = Join-Path $workspaceDir "target\release\aerostream.exe"
if (Test-Path $releaseExe) {
    Copy-Item $releaseExe (Join-Path $deployDir "aerostream.exe") -Force
    Write-Host "[+] Copied aerostream.exe ($((Get-Item $releaseExe).Length) bytes)" -ForegroundColor Green
}

# 2. Copy manifest
$manifest = Join-Path $workspaceDir "aerostream.exe.manifest"
if (Test-Path $manifest) {
    Copy-Item $manifest (Join-Path $deployDir "aerostream.exe.manifest") -Force
    Write-Host "[+] Copied aerostream.exe.manifest" -ForegroundColor Green
}

# 3. Copy latest Android APK
$apkSrc = Join-Path $workspaceDir "android_app\build\app\outputs\flutter-apk\app-release.apk"
if (-not (Test-Path $apkSrc)) {
    $apkSrc = Join-Path $workspaceDir "AeroStream-Android.apk"
}
if (Test-Path $apkSrc) {
    Copy-Item $apkSrc (Join-Path $deployDir "AeroStream-Android.apk") -Force
    Write-Host "[+] Copied AeroStream-Android.apk ($((Get-Item $apkSrc).Length) bytes)" -ForegroundColor Green
}

# 4. Copy TLS certificates if available
$certSrc = Join-Path $workspaceDir "cert.pem"
$keySrc = Join-Path $workspaceDir "key.pem"
if (Test-Path $certSrc) {
    Copy-Item $certSrc (Join-Path $deployDir "cert.pem") -Force
}
if (Test-Path $keySrc) {
    Copy-Item $keySrc (Join-Path $deployDir "key.pem") -Force
}

# 5. Copy useful helper scripts to deploy/
$scriptsToCopy = @(
    "Install-FirewallRule.bat",
    "Uninstall-FirewallRule.bat",
    "Setup-SessionUser.bat",
    "Unlock-UIPI.bat"
)
foreach ($s in $scriptsToCopy) {
    $srcPath = Join-Path $workspaceDir "scripts\$s"
    if (-not (Test-Path $srcPath)) {
        $srcPath = Join-Path $workspaceDir "AeroStream-Windows\$s"
    }
    if (Test-Path $srcPath) {
        Copy-Item $srcPath (Join-Path $deployDir $s) -Force
        Write-Host "[+] Copied script $s" -ForegroundColor Green
    }
}

Write-Host "`nContents of ${deployDir}:" -ForegroundColor Cyan
Get-ChildItem $deployDir | Select-Object Name, Length, LastWriteTime
