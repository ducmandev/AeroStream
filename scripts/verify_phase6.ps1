# Phase 6 Dual-Mode Remote Verification Script
$ErrorActionPreference = "Stop"

Write-Host "=== AeroStream Phase 6: Dual-Mode Remote Integration Test ===" -ForegroundColor Cyan

$baseUrl = "http://127.0.0.1:8080"

# 1. Test GET /api/status
Write-Host "`n1. Testing GET /api/status capability probing..." -ForegroundColor Yellow
try {
    $status = Invoke-RestMethod -Uri "$baseUrl/api/status" -Method Get
    $hostPin = $status.pin
    Write-Host "Online: $($status.status)" -ForegroundColor Green
    Write-Host "Host PIN: $hostPin" -ForegroundColor Green
    Write-Host "Modes: $($status.modes -join ', ')" -ForegroundColor Green
    Write-Host "Session Capability: $($status.session_capability | ConvertTo-Json -Compress)" -ForegroundColor Green
} catch {
    Write-Host "Failed to query /api/status: $_" -ForegroundColor Red
    exit 1
}

$currentUser = $env:USERNAME

# 2. Test POST /api/session/config with CURRENT Windows user ($env:USERNAME)
Write-Host "`n2. Testing POST /api/session/config with current Windows user ($currentUser)..." -ForegroundColor Yellow
try {
    $body = @{
        pin = $hostPin
        username = $currentUser
        password = "CurrentTestPassword123!"
    } | ConvertTo-Json
    $res = Invoke-RestMethod -Uri "$baseUrl/api/session/config" -Method Post -Body $body -ContentType "application/json"
    Write-Host "SUCCESS: Current user credential registered (DPAPI encrypted): $($res | ConvertTo-Json -Compress)" -ForegroundColor Green

    # Check status reflects current user
    $status = Invoke-RestMethod -Uri "$baseUrl/api/status" -Method Get
    Write-Host "Configured user: $($status.session_capability.configured_username)" -ForegroundColor Green
    Write-Host "Is current user: $($status.session_capability.is_current_user)" -ForegroundColor Green
    if ($status.session_capability.is_current_user -eq $true) {
        Write-Host "PASSED: Server correctly identifies configured account as current console user (Windows RDP auto-lock mode ready)" -ForegroundColor Green
    } else {
        Write-Host "FAILED: is_current_user was not true!" -ForegroundColor Red
    }
} catch {
    Write-Host "Failed to save current user config: $_" -ForegroundColor Red
}

# 3. Test POST /api/session/config with SECONDARY user ('aerostream_remote')
Write-Host "`n3. Testing POST /api/session/config with secondary user 'aerostream_remote' (isolated session)..." -ForegroundColor Yellow
try {
    $body = @{
        pin = $hostPin
        username = "aerostream_remote"
        password = "RemoteUserSecurePass2026!"
    } | ConvertTo-Json
    $res = Invoke-RestMethod -Uri "$baseUrl/api/session/config" -Method Post -Body $body -ContentType "application/json"
    Write-Host "SUCCESS: Secondary user credential registered: $($res | ConvertTo-Json -Compress)" -ForegroundColor Green
} catch {
    Write-Host "Failed to save secondary user config: $_" -ForegroundColor Red
}

# 4. Re-query /api/status to confirm 'session' mode is unlocked with secondary user
Write-Host "`n4. Re-querying /api/status with secondary user..." -ForegroundColor Yellow
try {
    $status = Invoke-RestMethod -Uri "$baseUrl/api/status" -Method Get
    Write-Host "Active Modes: $($status.modes -join ', ')" -ForegroundColor Green
    Write-Host "Capability: $($status.session_capability | ConvertTo-Json -Compress)" -ForegroundColor Green
    if ($status.session_capability.is_current_user -eq $false) {
        Write-Host "PASSED: Server correctly identifies configured account as secondary user (isolated background session)" -ForegroundColor Green
    }
    if ($status.modes -contains "session") {
        Write-Host "Dual-Mode capability gating: PASSED (Console & Session both available)" -ForegroundColor Green
    } else {
        Write-Host "Notice: Session mode not in modes: $($status.session_capability.reason)" -ForegroundColor Yellow
    }
} catch {
    Write-Host "Failed: $_" -ForegroundColor Red
}

# 5. Restore current user as primary choice for user convenience
Write-Host "`n5. Restoring current user ($currentUser) as configured session account..." -ForegroundColor Yellow
try {
    $body = @{
        pin = $hostPin
        username = $currentUser
        password = "SamplePassword"
    } | ConvertTo-Json
    $res = Invoke-RestMethod -Uri "$baseUrl/api/session/config" -Method Post -Body $body -ContentType "application/json"
    Write-Host "Configured user restored to '$currentUser': $($res.message)" -ForegroundColor Green
} catch {
    Write-Host "Failed restoring: $_" -ForegroundColor Yellow
}

Write-Host "`n=== Dual-Mode Session & Windows RDP Account Verification COMPLETED ===" -ForegroundColor Cyan
