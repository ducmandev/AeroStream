param([string]$Pin = "", [int]$Seconds = 8)
# Spawn a VISIBLE console printing rapidly = guaranteed screen activity
$worker = Start-Process powershell -ArgumentList "-NoProfile","-Command","1..200000 | ForEach-Object { Write-Host \`"ACTIVITY \$_ $(Get-Date -Format fff)\`" }" -PassThru -WindowStyle Normal
Start-Sleep -Milliseconds 800

$p = Get-Process aerostream
$c0 = $p.CPU
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?pin=$Pin&codec=h264", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
$buf = [byte[]]::new(4194304)
$frames = 0; $bytes = 0
$sw = [System.Diagnostics.Stopwatch]::StartNew()
while ($sw.ElapsedMilliseconds -lt ($Seconds * 1000)) {
  $cts = [System.Threading.CancellationTokenSource]::new(500)
  try { $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult() } catch { break }
  if ($r.MessageType -eq "Binary") { $frames++; $bytes += $r.Count }
}
$ws.Dispose()
$p.Refresh()
try { Stop-Process -Id $worker.Id -Force } catch {}
$fps = [math]::Round($frames/$Seconds,1)
$cpu = [math]::Round(($p.CPU - $c0)/$Seconds*100,1)
Write-Output ("BENCH active: frames={0} fps={1} avgKB={2} bitrate={3} Mbps" -f $frames, $fps, [math]::Round($bytes/1024/[math]::Max($frames,1),1), [math]::Round($bytes*8/$Seconds/1000000,2))
Write-Output ("CPU: {0}% of one core at {1} fps  (baseline 3.1: 54.7% @ 50.3fps)" -f $cpu, $fps)
Write-Output ("Per-frame: {0} %-core*ms (old: {1})" -f [math]::Round($cpu*1000/[math]::Max($frames,1),2), [math]::Round(54.7*1000/302,2))
