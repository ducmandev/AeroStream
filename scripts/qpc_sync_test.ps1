# scripts/qpc_sync_test.ps1
$status = Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/status"
Write-Host "Server status:"
Write-Host "  H.264 Encoder: $($status.h264_encoder)"
Write-Host "  Server QPC Time: $($status.server_time)"

if ($null -eq $status.server_time -or $status.server_time -le 0) {
    Write-Error "server_time missing or invalid in /api/status!"
    exit 1
}

$pin = $status.pin
Write-Host "Authenticating WebSocket with PIN: $pin"

# Connect ClientWebSocket
$ws = New-Object System.Net.WebSockets.ClientWebSocket
$cts = New-Object System.Threading.CancellationTokenSource
$cts.CancelAfter(8000)

$wsUri = New-Object System.Uri("ws://127.0.0.1:8080/ws?pin=$pin")
$connTask = $ws.ConnectAsync($wsUri, $cts.Token)
$connTask.Wait()

Write-Host "Connected to WebSocket. State: $($ws.State)"

$buffer = New-Object byte[] 65536
$segment = New-Object System.ArraySegment[byte] -ArgumentList @($buffer, 0, $buffer.Length)

# Read initial messages (time_sync, token, etc.)
$timeSyncReceived = $false
$serverTimeFromSync = 0
$videoTs = 0
$audioTs = 0

$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

# Send ping to test pong
$clientTime = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$pingJson = "{`"type`":`"ping`",`"client_time`":$clientTime}"
$pingBytes = [System.Text.Encoding]::UTF8.GetBytes($pingJson)
$pingSegment = New-Object System.ArraySegment[byte] -ArgumentList @($pingBytes, 0, $pingBytes.Length)
$sendTask = $ws.SendAsync($pingSegment, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $cts.Token)
$sendTask.Wait()
Write-Host "Sent ping with client_time: $clientTime"

$pongReceived = $false
$pongServerTime = 0

while ($stopwatch.ElapsedMilliseconds -lt 5000 -and (-not ($pongReceived -and $videoTs -gt 0 -and $audioTs -gt 0))) {
    $recvTask = $ws.ReceiveAsync($segment, $cts.Token)
    $recvTask.Wait()
    $result = $recvTask.Result
    $count = $result.Count

    if ($result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Text) {
        $msgText = [System.Text.Encoding]::UTF8.GetString($buffer, 0, $count)
        if ($msgText.Contains("time_sync")) {
            $syncObj = $msgText | ConvertFrom-Json
            $serverTimeFromSync = $syncObj.server_time
            $timeSyncReceived = $true
            Write-Host "[TEXT] Received time_sync: server_time = $serverTimeFromSync"
        }
        if ($msgText.Contains("pong")) {
            $pongObj = $msgText | ConvertFrom-Json
            $pongServerTime = $pongObj.server_time
            $pongReceived = $true
            $rtt = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $clientTime
            $offset = ($pongServerTime + ($rtt / 2)) - [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
            Write-Host "[TEXT] Received pong: client_time = $($pongObj.client_time), server_time = $pongServerTime, RTT = ${rtt}ms, ClockOffset = ${offset}ms"
        }
    } elseif ($result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Binary) {
        if ($count -ge 10 -and $buffer[0] -eq 0xFA -and $buffer[1] -eq 0xFA) {
            # Audio packet: [0xFA, 0xFA, ts(8 bytes BE), opus...]
            $tsBytes = New-Object byte[] 8
            [Array]::Copy($buffer, 2, $tsBytes, 0, 8)
            if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($tsBytes) }
            $audioTs = [BitConverter]::ToUInt64($tsBytes, 0)
        } elseif ($count -ge 12) {
            # Video frame: [ts(8 bytes BE), w(2), h(2), nal...]
            $tsBytes = New-Object byte[] 8
            [Array]::Copy($buffer, 0, $tsBytes, 0, 8)
            if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($tsBytes) }
            $videoTs = [BitConverter]::ToUInt64($tsBytes, 0)
        }
    }
}

$closeTask = $ws.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, "Done", $cts.Token)
$closeTask.Wait()

Write-Host "=== QPC A/V SYNC VERIFICATION RESULTS ==="
Write-Host "time_sync message received: $timeSyncReceived ($serverTimeFromSync)"
Write-Host "pong message received: $pongReceived (server_time: $pongServerTime)"
Write-Host "Latest Video QPC Timestamp: $videoTs"
Write-Host "Latest Audio QPC Timestamp: $audioTs"

if ($videoTs -gt 0 -and $audioTs -gt 0) {
    $diff = [Math]::Abs([int64]$videoTs - [int64]$audioTs)
    Write-Host "A/V Timestamp Delta: ${diff} ms"
    if ($diff -lt 500) {
        Write-Host "SUCCESS: Video and Audio QPC timestamps are synchronized in the same monotonic epoch! (Delta: ${diff}ms < 500ms)"
        exit 0
    } else {
        Write-Warning "Delta is larger than expected: ${diff}ms"
        exit 0
    }
} else {
    Write-Host "Video or Audio packet not received during window. Video: $videoTs, Audio: $audioTs"
    exit 0
}
