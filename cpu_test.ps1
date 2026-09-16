param([string]$Pin = "885095")
$proc = Get-Process aerostream -ErrorAction Stop
$cpu0 = $proc.CPU
$ws0 = $proc.WorkingSet64

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?pin=$Pin&codec=h264", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null

$buf = [byte[]]::new(4194304)
$frames = 0; $bytes = 0
$sw = [System.Diagnostics.Stopwatch]::StartNew()
# Generate screen activity by writing progress to console (drives change detection -> encode)
while ($sw.ElapsedMilliseconds -lt 6000) {
  $cts = [System.Threading.CancellationTokenSource]::new(800)
  try { $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult() } catch { break }
  if ($r.MessageType -eq "Binary") { $frames++; $bytes += $r.Count }
}
$ws.Dispose()

$proc.Refresh()
$cpu1 = $proc.CPU
$ws1 = $proc.WorkingSet64
$cpuDeltaSec = $cpu1 - $cpu0
$cpuPercent = ($cpuDeltaSec / 6.0) * 100
Write-Output ("h264 frames={0} avgKB={1} avgFPS={2}" -f $frames, [math]::Round($bytes/1024/[math]::Max($frames,1),1), [math]::Round($frames/6.0,1))
Write-Output ("process CPU during stream: {0:N1}% (of one core) | RAM: {1:N0} MB -> {2:N0} MB" -f $cpuPercent, $ws0/1MB, $ws1/1MB)
