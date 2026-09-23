param([string]$Pin = "469229", [int]$Count = 3)
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?pin=$Pin", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
$buf = [byte[]]::new(4194304)
$got = 0
$sw = [System.Diagnostics.Stopwatch]::StartNew()
while ($sw.ElapsedMilliseconds -lt 6000 -and $got -lt $Count) {
  $cts = [System.Threading.CancellationTokenSource]::new(1500)
  try { $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult() } catch { continue }
  if ($r.MessageType -eq "Binary" -and $r.Count -gt 100 -and -not ($buf[0] -eq 0xFA)) {
    $got++
    [System.IO.File]::WriteAllBytes("D:\StreamApp\lockframe_$got.jpg", $buf[12..($r.Count-1)])
  }
}
$ws.Dispose()
Write-Output "saved $got frames"
