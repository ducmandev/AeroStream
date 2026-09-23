param([string]$EngineArgs = "")

$exePath = "D:\StreamApp\deploy\aerostream.exe"
if (-not (Test-Path $exePath)) {
    $exePath = "D:\StreamApp\target\release\aerostream.exe"
}

$result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
    CommandLine = "`"$exePath`" $EngineArgs".Trim()
    CurrentDirectory = (Split-Path $exePath)
}

if ($result.ReturnValue -eq 0) {
    Write-Host "Started aerostream_engine with PID: $($result.ProcessId) via WMI (detached)"
    Start-Sleep -Milliseconds 1500
    $p = Get-Process -Id $result.ProcessId -ErrorAction SilentlyContinue
    if ($p -and -not $p.HasExited) {
        Write-Host "Process is running healthy!"
    } else {
        Write-Host "Process may have exited prematurely."
    }
} else {
    Write-Host "Failed to start process via WMI, ReturnValue: $($result.ReturnValue)"
}
