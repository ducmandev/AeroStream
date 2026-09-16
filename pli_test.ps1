param([string]$Pin = "")

# Ensure engine is running
$proc = Get-Process -Name aerostream*, *aerostream* -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) {
  Write-Output "Engine not running, starting via scripts\start_engine.ps1..."
  & "D:\StreamApp\scripts\start_engine.ps1"
  Start-Sleep -Seconds 2
}

if ([string]::IsNullOrWhiteSpace($Pin)) {
  try {
    $status = Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/status" -TimeoutSec 5
    $Pin = $status.pin
    Write-Output "Auto-detected PIN: $Pin"
  } catch {
    Write-Error "Could not fetch PIN from http://127.0.0.1:8080/api/status: $_"
    exit 1
  }
}

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.ConnectAsync([Uri]"ws://127.0.0.1:8080/ws?pin=$Pin&codec=h264", [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
Write-Output "connected (codec=h264)"

function SendJson($json) {
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
  $script:ws.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
}

$buf = [byte[]]::new(4194304)
function ScanForIdr($data, $len) {
  # return true if an IDR NAL (type 5) exists in this frame
  for ($i = 12; $i -lt ($len - 4); $i++) {
    if ($data[$i] -eq 0 -and $data[$i+1] -eq 0) {
      $j = -1
      if ($data[$i+2] -eq 1) { $j = $i + 3 }
      elseif ($data[$i+2] -eq 0 -and $data[$i+3] -eq 1) { $j = $i + 4 }
      if ($j -ge 0 -and (($data[$j] -band 0x1F) -eq 5)) { return $true }
    }
  }
  return $false
}

# Phase 1: receive frames for 2s, note whether we see any IDR naturally
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$framesBefore = 0; $idrBefore = 0
while ($sw.ElapsedMilliseconds -lt 2000) {
  $cts = [System.Threading.CancellationTokenSource]::new(1000)
  try { $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult() } catch { break }
  if ($r.MessageType -ne "Binary" -or $r.Count -lt 12) { continue }
  $framesBefore++
  if (ScanForIdr $buf $r.Count) { $idrBefore++ }
}
Write-Output "before PLI: frames=$framesBefore idr=$idrBefore"

# Phase 2: send frame_loss (PLI), then count frames/IDRs for 2s
SendJson '{"type":"frame_loss"}'
$sw.Restart()
$framesAfter = 0; $idrAfter = 0; $idrLatency = -1
while ($sw.ElapsedMilliseconds -lt 2000) {
  $cts = [System.Threading.CancellationTokenSource]::new(1000)
  try { $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult() } catch { break }
  if ($r.MessageType -ne "Binary" -or $r.Count -lt 12) { continue }
  $framesAfter++
  if (ScanForIdr $buf $r.Count -and $idrLatency -lt 0) { $idrLatency = $sw.ElapsedMilliseconds }
  if (ScanForIdr $buf $r.Count) { $idrAfter++ }
}
Write-Output "after PLI: frames=$framesAfter idr=$idrAfter firstIdrLatency=${idrLatency}ms"
$ws.Dispose()
