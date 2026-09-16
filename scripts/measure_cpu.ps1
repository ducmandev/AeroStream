$p = Get-Process -Name aerostream*, *aerostream* -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) {
    Write-Host "Process aerostream not found"
    exit 1
}
$c1 = $p.TotalProcessorTime.TotalMilliseconds
Start-Sleep -Seconds 2
$p.Refresh()
$c2 = $p.TotalProcessorTime.TotalMilliseconds
$cpu = [math]::Round(($c2 - $c1) / (20 * [Environment]::ProcessorCount), 2)
Write-Host "Idle CPU Usage of aerostream.exe: $cpu%"
