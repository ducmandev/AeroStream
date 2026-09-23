param(
  [string]$Pin = "",
  [string]$Token = "",
  [string]$BadToken = "deadbeefdeadbeefdeadbeefdeadbeef"
)

if ([string]::IsNullOrWhiteSpace($Token)) {
  if ([string]::IsNullOrWhiteSpace($Pin)) {
    $status = Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/status"
    $Pin = $status.pin
  }
  $pairResp = Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/auth/pair" -Method Post -ContentType "application/json" -Body (@{ pin = $Pin; device = "PowerShell-TokenTest" } | ConvertTo-Json)
  $Token = $pairResp.token
  Write-Output ("Obtained Token: {0}" -f $Token)
}
function TryWs($query) {
  $ws = [System.Net.WebSockets.ClientWebSocket]::new()
  try {
    $ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?$query", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult()
    # receive one message to confirm full flow
    $buf = [byte[]]::new(1048576)
    $cts = [System.Threading.CancellationTokenSource]::new(3000)
    $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult()
    $ws.Dispose()
    return ("CONNECTED (recv {0} bytes)" -f $r.Count)
  } catch { $ws.Dispose(); return ("REJECTED: " + $_.Exception.GetBaseException().Message) }
}
Write-Output ("token hop le  -> " + (TryWs "token=$Token"))
Write-Output ("token rac     -> " + (TryWs "token=$BadToken"))
