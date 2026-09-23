# AeroStream M-SA.1 Secure Agent Verification Script
# Run this script in an ELEVATED (Run as Administrator) PowerShell window.

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  AeroStream Work Order #1 — M-SA.1 Acceptance Test      " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Check Elevation
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[FAIL] This verification script MUST be run as Administrator." -ForegroundColor Red
    Write-Host "Please reopen PowerShell via 'Run as Administrator'." -ForegroundColor Yellow
    exit 1
}
Write-Host "[OK] Running with elevated Administrator privileges." -ForegroundColor Green

# 2. Cleanup any previous proof file
$proofPath = "C:\Windows\Temp\aerostream-agent-proof.txt"
if (Test-Path $proofPath) {
    Remove-Item $proofPath -Force -ErrorAction SilentlyContinue
    Write-Host "[INFO] Cleaned previous proof file at $proofPath" -ForegroundColor Gray
}

# 3. Trigger Spawn
Write-Host "[ACTION] Executing: .\target\release\aerostream.exe --spawn-secure-agent" -ForegroundColor Yellow
$output = & ".\target\release\aerostream.exe" --spawn-secure-agent
Write-Host $output

# 4. Wait 3 seconds for service teardown & agent proof
Start-Sleep -Seconds 3

Write-Host "`n--- CHECKLIST VERIFICATION ---" -ForegroundColor Cyan

# Check A: sc query AeroStreamSecureHelper must report NOT EXISTS
Write-Host "[CHECK A] Verifying temporary service was deleted..." -NoNewline
$scOutput = sc.exe query AeroStreamSecureHelper 2>&1 | Out-String
if ($scOutput -match "FAILED 1060" -or $scOutput -match "does not exist") {
    Write-Host " [PASS]" -ForegroundColor Green
    Write-Host "  Service AeroStreamSecureHelper no longer exists in SCM (cleanly deleted)." -ForegroundColor Gray
} else {
    Write-Host " [FAIL]" -ForegroundColor Red
    Write-Host "  Service still exists: $scOutput" -ForegroundColor Red
}

# Check B: tasklist /V shows aerostream.exe with user = SYSTEM in session
Write-Host "[CHECK B] Verifying aerostream.exe running as SYSTEM in session..." -NoNewline
$tasklist = tasklist.exe /V /FO CSV | ConvertFrom-Csv | Where-Object { $_."Image Name" -eq "aerostream.exe" }
$systemAgent = $tasklist | Where-Object { $_."User Name" -match "SYSTEM" }
if ($systemAgent) {
    Write-Host " [PASS]" -ForegroundColor Green
    $systemAgent | ForEach-Object {
        Write-Host "  PID: $($_.PID) | User: $($_.`"User Name`") | Session: $($_.`"Session#`") | Mem: $($_.`"Mem Usage`")" -ForegroundColor Gray
    }
} else {
    Write-Host " [FAIL]" -ForegroundColor Red
    Write-Host "  No aerostream.exe running under SYSTEM found in tasklist /V." -ForegroundColor Red
}

# Check C: Proof file exists and verify contents
Write-Host "[CHECK C] Verifying C:\Windows\Temp\aerostream-agent-proof.txt..." -NoNewline
if (Test-Path $proofPath) {
    Write-Host " [PASS]" -ForegroundColor Green
    Write-Host "--- Proof File Contents ---" -ForegroundColor Yellow
    Get-Content $proofPath | ForEach-Object { Write-Host "  $_" -ForegroundColor White }
    Write-Host "---------------------------" -ForegroundColor Yellow
} else {
    Write-Host " [FAIL]" -ForegroundColor Red
    Write-Host "  Proof file was not found at $proofPath" -ForegroundColor Red
}

Write-Host "`nAcceptance test complete." -ForegroundColor Cyan
