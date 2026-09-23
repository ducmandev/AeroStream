# AeroStream Work Order #2 - M-SA.2 Secure Agent Worker Test Script
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\test_secure_agent_worker.ps1 [-SendText "Password123"] [-Wake]

param(
    [string]$SendText = "",
    [switch]$Wake
)

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  AeroStream Work Order #2 - M-SA.2 Worker Verification   " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

$pipePath = "\\.\pipe\aerostream-secure-agent"
$framePath = "C:\Windows\Temp\aerostream-secure-frame.jpg"
$agentLogPath = "C:\Windows\Temp\aerostream-secure-agent.log"
$proofPath = "C:\Windows\Temp\aerostream-agent-proof.txt"

$exePath = Join-Path $PSScriptRoot "..\target\release\aerostream.exe"
if (-not (Test-Path $exePath)) {
    $exePath = ".\target\release\aerostream.exe"
}

# 1. Check if agent process is currently running
Write-Host "`n[STEP 1] Checking Secure Agent process..." -NoNewline
$aerostreamProcs = Get-Process aerostream -ErrorAction SilentlyContinue

if (-not $aerostreamProcs) {
    Write-Host " [NOT RUNNING]" -ForegroundColor Yellow
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if ($isAdmin) {
        Write-Host "Elevated prompt detected. Spawning secure agent via --spawn-secure-agent..." -ForegroundColor Yellow
        $spawnOut = & $exePath --spawn-secure-agent
        Write-Host $spawnOut
        Start-Sleep -Seconds 2
        $aerostreamProcs = Get-Process aerostream -ErrorAction SilentlyContinue
    } else {
        Write-Host "[WARN] Secure agent is not currently running." -ForegroundColor Yellow
        Write-Host "  To spawn the secure agent, please run once from an Administrator prompt:" -ForegroundColor Yellow
        Write-Host "    .\target\release\aerostream.exe --spawn-secure-agent" -ForegroundColor White
        Write-Host "  Or run: .\scripts\spawn_agent.bat (prompts UAC automatically)" -ForegroundColor White
    }
}

if ($aerostreamProcs) {
    Write-Host " [FOUND]" -ForegroundColor Green
    foreach ($p in $aerostreamProcs) {
        Write-Host "  PID: $($p.Id) | Session: $($p.SessionId)" -ForegroundColor Gray
    }
} else {
    Write-Host " [FAIL] Could not locate aerostream.exe process." -ForegroundColor Red
}

# 2. Check GDI Capture Frame
$mirrorPath = Join-Path $PSScriptRoot "..\target\aerostream-secure-frame.jpg"
$activeFrame = $null
if (Test-Path $framePath -ErrorAction SilentlyContinue) {
    $activeFrame = $framePath
} elseif (Test-Path $mirrorPath -ErrorAction SilentlyContinue) {
    $activeFrame = $mirrorPath
}

Write-Host "`n[STEP 2] Verifying GDI capture output ($framePath)..." -NoNewline
if ($activeFrame) {
    $item1 = Get-Item $activeFrame -ErrorAction SilentlyContinue
    $time1 = $item1.LastWriteTime
    $size1 = $item1.Length
    Write-Host " [EXISTS]" -ForegroundColor Green
    Write-Host "  Frame size: $size1 bytes | Timestamp: $time1" -ForegroundColor Gray
    
    # Wait 1s and check for refresh
    Start-Sleep -Milliseconds 1200
    $item2 = Get-Item $activeFrame -ErrorAction SilentlyContinue
    $time2 = $item2.LastWriteTime
    if ($time2 -gt $time1) {
        Write-Host "  Active update confirmed: frame refreshed to $time2" -ForegroundColor Green
    } else {
        Write-Host "  Frame exists (last written $time1)" -ForegroundColor Gray
    }
} else {
    Write-Host " [PENDING]" -ForegroundColor Yellow
    Write-Host "  Frame file not created yet; waiting 2s..." -ForegroundColor Gray
    Start-Sleep -Seconds 2
    if (Test-Path $framePath -ErrorAction SilentlyContinue) {
        $item = Get-Item $framePath -ErrorAction SilentlyContinue
        Write-Host "  [PASS] Frame created! Size: $($item.Length) bytes" -ForegroundColor Green
    } elseif (Test-Path $mirrorPath -ErrorAction SilentlyContinue) {
        $item = Get-Item $mirrorPath -ErrorAction SilentlyContinue
        Write-Host "  [PASS] Frame created in target! Size: $($item.Length) bytes" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] Frame file not found yet." -ForegroundColor Red
    }
}

# 3. Check Named Pipe Connection & Dispatch Input
Write-Host "`n[STEP 3] Testing Named Pipe: $pipePath..." -NoNewline
try {
    $pipeClient = New-Object System.IO.Pipes.NamedPipeClientStream(".", "aerostream-secure-agent", [System.IO.Pipes.PipeDirection]::InOut)
    $pipeClient.Connect(2000)
    Write-Host " [CONNECTED]" -ForegroundColor Green
    
    $writer = New-Object System.IO.StreamWriter($pipeClient)
    $writer.AutoFlush = $true

    if ($Wake) {
        Write-Host "  Sending wake_lock_screen command..." -ForegroundColor Yellow
        $writer.WriteLine('{"type":"wake_lock_screen"}')
        Write-Host '  [SENT] {"type":"wake_lock_screen"}' -ForegroundColor Green
        Write-Host "  Waiting 1.2s for lock screen wallpaper to dismiss and LogonUI to focus password box..." -ForegroundColor Gray
        Start-Sleep -Milliseconds 1200
    }

    if ($SendText -ne "") {
        Write-Host "  Sending text injection: '$SendText'..." -ForegroundColor Yellow
        $jsonPayload = '{"type":"text","text":"' + $SendText + '"}'
        $writer.WriteLine($jsonPayload)
        Write-Host "  [SENT] $jsonPayload" -ForegroundColor Green
    }

    # If no flags passed, send a harmless test mouse delta nudge via pipe
    if (-not $Wake -and $SendText -eq "") {
        Write-Host "  Sending test mouse delta nudge via pipe..." -ForegroundColor Yellow
        $writer.WriteLine('{"type":"mouse_delta","dx":0,"dy":0}')
        Write-Host '  [SENT] {"type":"mouse_delta","dx":0,"dy":0}' -ForegroundColor Green
    }

    $pipeClient.Dispose()
    Write-Host "  [PASS] Pipe communication succeeded cleanly." -ForegroundColor Green
} catch {
    Write-Host " [FAILED]" -ForegroundColor Red
    Write-Host "  Error connecting or writing to pipe: $_" -ForegroundColor Red
}

Write-Host "`n[STEP 4] Recent Secure Agent Logs ($agentLogPath):" -ForegroundColor Cyan
if (Test-Path $agentLogPath -ErrorAction SilentlyContinue) {
    try {
        Get-Content $agentLogPath -Tail 15 -ErrorAction Stop | ForEach-Object {
            Write-Host "  $_" -ForegroundColor White
        }
    } catch {
        Write-Host "  Log exists but requires elevated privilege to read." -ForegroundColor Gray
    }
} else {
    Write-Host "  Log file not found." -ForegroundColor Gray
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host "  Verification complete. To test lock screen typing:     " -ForegroundColor Cyan
Write-Host "    1. Lock workstation (Win + L)                        " -ForegroundColor Yellow
Write-Host "    2. Run from remote/another session or script:        " -ForegroundColor Yellow
Write-Host '       .\scripts\test_secure_agent_worker.ps1 -Wake -SendText "Secret"' -ForegroundColor White
Write-Host "    3. Check C:\Windows\Temp\aerostream-secure-frame.jpg " -ForegroundColor Yellow
Write-Host "==========================================================" -ForegroundColor Cyan
