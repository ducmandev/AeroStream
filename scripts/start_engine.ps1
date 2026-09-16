param([string]$EngineArgs = "")

$exePath = "D:\StreamApp\AeroStream-Windows\aerostream_engine.exe"
if (-not (Test-Path $exePath)) {
    $exePath = "D:\StreamApp\target\release\aerostream.exe"
}

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $exePath
$psi.Arguments = $EngineArgs
$psi.WorkingDirectory = (Split-Path $exePath)
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true

$p = [System.Diagnostics.Process]::Start($psi)
if ($p) {
    Write-Host "Started $($p.ProcessName) with PID: $($p.Id) (Args: '$EngineArgs')"
    Start-Sleep -Milliseconds 1500
    if (-not $p.HasExited) {
        Write-Host "Process is running healthy!"
    } else {
        Write-Host "Process exited with code: $($p.ExitCode)"
    }
} else {
    Write-Host "Failed to start process."
}
