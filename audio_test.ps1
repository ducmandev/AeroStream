param([string]$Pin = "")

if ([string]::IsNullOrWhiteSpace($Pin)) {
  try {
    $status = Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/status" -TimeoutSec 2
    $Pin = $status.pin
    Write-Output "Auto-detected PIN: $Pin"
  } catch {
    Write-Error "Could not fetch PIN from http://127.0.0.1:8080/api/status: $_"
    exit 1
  }
}

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?pin=$Pin", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
Write-Output "connected as JPEG viewer"

$buf = [byte[]]::new(4194304)
$audioCount = 0; $videoCount = 0; $otherCount = 0
$firstAudioTs = 0; $lastAudioTs = 0; $maxGapMs = 0
$sw = [System.Diagnostics.Stopwatch]::StartNew()
while ($sw.ElapsedMilliseconds -lt 3000) {
  $cts = [System.Threading.CancellationTokenSource]::new(1000)
  try { $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult() } catch { break }
  if ($r.MessageType -ne "Binary" -or $r.Count -lt 2) { $otherCount++; continue }
  if ($buf[0] -eq 0xFA -and $buf[1] -eq 0xFA -and $r.Count -ge 10) {
    $audioCount++
    $ts = 0L
    for ($i = 0; $i -lt 8; $i++) { $ts = ($ts -shl 8) -bor $buf[2 + $i] }
    if ($firstAudioTs -eq 0) { $firstAudioTs = $ts }
    if ($lastAudioTs -gt 0) {
      $gap = $ts - $lastAudioTs
      if ($gap -gt $maxGapMs) { $maxGapMs = $gap }
    }
    $lastAudioTs = $ts
    $avgBytes = if (-not $avgBytes) { $r.Count } else { $avgBytes }
  } elseif ($buf[0] -eq 0xFE -and $buf[1] -eq 0xFE) {
    $otherCount++
  } else {
    $videoCount++
  }
}
$ws.Dispose()
$duration = 3
Write-Output ("audio(0xFAFA): {0} packets in {1}s (~{2}/s, expected ~50/s) | maxGap={3}ms | video frames: {4}" -f $audioCount, $duration, [math]::Round($audioCount/$duration), $maxGapMs, $videoCount)
