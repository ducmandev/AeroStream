# Test IPC Bridge with dynamic PIN
$ErrorActionPreference = "Stop"

$status = Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/status"
$pin = $status.pin
Write-Host "Host PIN is: $pin" -ForegroundColor Yellow

$wsUri = [System.Uri]"ws://127.0.0.1:8080/ws?pin=$pin"
$ws = New-Object System.Net.WebSockets.ClientWebSocket
$cts = New-Object System.Threading.CancellationTokenSource

Write-Host "Connecting to WebSocket at $wsUri..." -ForegroundColor Cyan
$connectTask = $ws.ConnectAsync($wsUri, $cts.Token)
$connectTask.Wait(3000)

if ($ws.State -eq [System.Net.WebSockets.WebSocketState]::Open) {
    Write-Host "[+] Connected to WebSocket successfully!" -ForegroundColor Green

    # Send wake_lock_screen
    $msgWake = '{"type":"wake_lock_screen"}'
    $bytesWake = [System.Text.Encoding]::UTF8.GetBytes($msgWake)
    $segmentWake = New-Object System.ArraySegment[byte] -ArgumentList @($bytesWake, 0, $bytesWake.Length)
    $ws.SendAsync($segmentWake, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $cts.Token).Wait()
    Write-Host "[+] Sent: $msgWake" -ForegroundColor Green

    Start-Sleep -Milliseconds 500

    # Send text
    $msgText = '{"type":"text","text":"5368"}'
    $bytesText = [System.Text.Encoding]::UTF8.GetBytes($msgText)
    $segmentText = New-Object System.ArraySegment[byte] -ArgumentList @($bytesText, 0, $bytesText.Length)
    $ws.SendAsync($segmentText, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $cts.Token).Wait()
    Write-Host "[+] Sent: $msgText" -ForegroundColor Green

    # Send Enter
    $msgEnter = '{"type":"key_click","key":"Enter"}'
    $bytesEnter = [System.Text.Encoding]::UTF8.GetBytes($msgEnter)
    $segmentEnter = New-Object System.ArraySegment[byte] -ArgumentList @($bytesEnter, 0, $bytesEnter.Length)
    $ws.SendAsync($segmentEnter, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $cts.Token).Wait()
    Write-Host "[+] Sent: $msgEnter" -ForegroundColor Green

    Start-Sleep -Milliseconds 500
    $ws.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, "Done", $cts.Token).Wait()
    Write-Host "[+] WebSocket closed cleanly." -ForegroundColor Cyan
} else {
    Write-Host "[!] Failed to connect to WebSocket: State is $($ws.State)" -ForegroundColor Red
}
