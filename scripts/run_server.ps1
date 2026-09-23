$proc = Start-Process -FilePath "D:\StreamApp\target\release\aerostream.exe" -PassThru
Write-Host "Started process ID: $($proc.Id)"
Start-Sleep -Seconds 2
if ($proc.HasExited) {
    Write-Host "Process exited with code: $($proc.ExitCode)" -ForegroundColor Red
} else {
    Write-Host "Process is running! (Id: $($proc.Id))" -ForegroundColor Green
}
