$pipeName = "aerostream-secure-agent"
try {
    $p = New-Object System.IO.Pipes.NamedPipeClientStream(".", $pipeName, [System.IO.Pipes.PipeDirection]::InOut)
    $p.Connect(800)
    Write-Host "Pipe '$pipeName' is CONNECTED and LISTENING!" -ForegroundColor Green
    $p.Dispose()
} catch {
    Write-Host "Pipe connect failed: $($_.Exception.Message)" -ForegroundColor Red
}
