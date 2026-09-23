# Cleanup redundant files from root
$ErrorActionPreference = "SilentlyContinue"

$workspace = "D:\StreamApp"
$filesToRemove = @(
    "query",
    "AeroStream-Windows.exe",
    "AeroStream-Windows.exe.manifest",
    "AeroStream-Windows-v1.0.zip",
    "aerostream_engine.exe",
    "aerostream.exe",
    "aerostream.exe.manifest",
    "AeroStream-Android.apk",
    "aerostream.log",
    "aerostream_debug.log"
)

foreach ($f in $filesToRemove) {
    $p = Join-Path $workspace $f
    if (Test-Path $p) {
        Remove-Item $p -Force -Recurse -ErrorAction SilentlyContinue
        if (-not (Test-Path $p)) {
            Write-Host "[REMOVED] $f" -ForegroundColor Green
        } else {
            Write-Host "[LOCKED/FAILED] $f" -ForegroundColor Yellow
        }
    }
}

# Remove old AeroStream-Windows directory
$oldFolder = Join-Path $workspace "AeroStream-Windows"
if (Test-Path $oldFolder) {
    Remove-Item $oldFolder -Force -Recurse -ErrorAction SilentlyContinue
    if (-not (Test-Path $oldFolder)) {
        Write-Host "[REMOVED FOLDER] AeroStream-Windows" -ForegroundColor Green
    } else {
        Write-Host "[LOCKED/FAILED FOLDER] AeroStream-Windows" -ForegroundColor Yellow
    }
}

Write-Host "`nClean Root Directory Structure:" -ForegroundColor Cyan
Get-ChildItem $workspace | Select-Object Name, Mode, LastWriteTime
