param([string]$Pin = "")
if ([string]::IsNullOrWhiteSpace($Pin)) {
  $status = Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/status"
  $Pin = $status.pin
}
function TryConnect($p) {
  $ws = [System.Net.WebSockets.ClientWebSocket]::new()
  try {
    $ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?pin=$p", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult()
    $ws.Dispose()
    return "OK(connected)"
  } catch [System.Net.WebSockets.WebSocketException] {
    $resp = $_.Exception.InnerException
    if ($resp -is [System.Net.HttpWebResponse]) { return "HTTP " + [int]$resp.StatusCode }
    return "WS-ERROR: " + $_.Exception.Message
  } catch { return "ERR: " + $_.Exception.Message }
}

for ($i = 1; $i -le 6; $i++) {
  Write-Output ("wrong-PIN attempt {0}: {1}" -f $i, (TryConnect "00000$i"))
}
Write-Output ("correct PIN while locked: " + (TryConnect $Pin))
Write-Output "waiting 62s for lockout expiry..."
Start-Sleep -Seconds 62
Write-Output ("correct PIN after expiry: " + (TryConnect $Pin))
