param([string]$Pin = "")

# Ensure engine is running
$proc = Get-Process -Name aerostream*, *aerostream* -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) {
  Write-Output "Engine not running, starting via scripts\start_engine.ps1..."
  & "D:\StreamApp\scripts\start_engine.ps1"
  Start-Sleep -Seconds 2
}

if ([string]::IsNullOrWhiteSpace($Pin)) {
  $status = Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/status"
  $Pin = $status.pin
}

Add-Type -AssemblyName System.Windows.Forms
$p0 = [System.Windows.Forms.Cursor]::Position
Write-Output ("BEFORE: {0},{1}" -f $p0.X, $p0.Y)

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?pin=$Pin", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult()

function SendJson($json) {
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
  $script:ws.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
}

SendJson '{"type":"mouse_move","x":0.9,"y":0.1}'
Start-Sleep -Milliseconds 800
$p1 = [System.Windows.Forms.Cursor]::Position
Write-Output ("AFTER mouse_move(0.9,0.1): {0},{1}" -f $p1.X, $p1.Y)

SendJson '{"type":"mouse_delta","dx":50,"dy":-30}'
Start-Sleep -Milliseconds 800
$p2 = [System.Windows.Forms.Cursor]::Position
Write-Output ("AFTER mouse_delta(50,-30): {0},{1}" -f $p2.X, $p2.Y)

SendJson '{"type":"mouse_wheel","delta_x":0,"delta_y":2}'
SendJson '{"type":"key_click","key":"f13"}'
Start-Sleep -Milliseconds 500
Write-Output "sent wheel+key_click(F13 - harmless)"
$ws.Dispose()
