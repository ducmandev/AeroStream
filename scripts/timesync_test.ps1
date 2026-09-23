param([string]$Pin = "")
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?pin=$Pin", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
$buf = [byte[]]::new(4194304)
$deadline = [DateTime]::UtcNow.AddSeconds(4)
$timeSync = $null; $pong = $null; $frames = 0; $audio = 0
while ([DateTime]::UtcNow -lt $deadline) {
  $cts = [System.Threading.CancellationTokenSource]::new(800)
  try { $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult() } catch { break }
  if ($r.MessageType -eq "Text") {
    $txt = [System.Text.Encoding]::UTF8.GetString($buf, 0, $r.Count)
    if ($txt -match "time_sync") { $timeSync = $txt }
    if ($txt -match "pong") { $pong = $txt }
    if ($timeSync -and -not $pong) {
      # send a ping with client_time to trigger pong
      $bytes = [System.Text.Encoding]::UTF8.GetBytes('{"type":"ping","client_time":' + [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() + '}')
      $ws.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
    }
  } else {
    if ($buf[0] -eq 0xFA -and $buf[1] -eq 0xFA) { $audio++ } else { $frames++ }
  }
}
$ws.Dispose()
Write-Output "time_sync: $timeSync"
Write-Output "pong:      $pong"
Write-Output ("binary: frames={0} audio={1}" -f $frames, $audio)
