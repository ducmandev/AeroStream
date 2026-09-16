$processes = Get-Process -Name aerostream*, *aerostream* -ErrorAction SilentlyContinue
if (-not $processes) {
    Write-Host "No aerostream process running."
    exit 0
}

foreach ($p in $processes) {
    Write-Host "Stopping $($p.ProcessName) (PID: $($p.Id))..."
    $res = Invoke-CimMethod -Query "select * from Win32_Process where ProcessId = $($p.Id)" -MethodName Terminate -ErrorAction SilentlyContinue
    if ($res.ReturnValue -eq 0) {
        Write-Host "Process $($p.Id) terminated successfully via CIM."
    } else {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    }
}
