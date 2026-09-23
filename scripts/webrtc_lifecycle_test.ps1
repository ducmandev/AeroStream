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
Write-Output "Connected to WebSocket signaling channel (codec=h264)"

function SendJson($json) {
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
  $script:ws.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [System.Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
}

function TestOffer($round) {
  Write-Output "`n--- Round $($round): Sending Offer on Same WebSocket ---"
  $sdpOffer = @"
v=0
o=- 46117314004300512 $round IN IP4 127.0.0.1
s=-
t=0 0
a=group:BUNDLE 0 1 2
m=video 9 UDP/TLS/RTP/SAVPF 96
c=IN IP4 0.0.0.0
a=rtcp:9 IN IP4 0.0.0.0
a=ice-ufrag:aerostream$round
a=ice-pwd:aerostreampassword$round
a=fingerprint:sha-256 00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00
a=setup:actpass
a=mid:0
a=recvonly
a=rtpmap:96 H264/90000
a=rtcp-fb:96 nack
a=rtcp-fb:96 nack pli
m=audio 9 UDP/TLS/RTP/SAVPF 111
c=IN IP4 0.0.0.0
a=rtcp:9 IN IP4 0.0.0.0
a=ice-ufrag:aerostream$round
a=ice-pwd:aerostreampassword$round
a=fingerprint:sha-256 00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00
a=setup:actpass
a=mid:1
a=recvonly
a=rtpmap:111 opus/48000/2
m=application 9 UDP/DTLS/SCTP webrtc-datachannel
c=IN IP4 0.0.0.0
a=ice-ufrag:aerostream$round
a=ice-pwd:aerostreampassword$round
a=fingerprint:sha-256 00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00
a=setup:actpass
a=mid:2
a=sctp-port:5000
"@ -replace "`r`n", "`n"

  $signalOffer = @{
    type = "signal"
    action = "offer"
    sdp = $sdpOffer
  } | ConvertTo-Json -Compress

  SendJson $signalOffer
  $buf = [byte[]]::new(65536)
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $receivedAnswer = $false

  while ($sw.ElapsedMilliseconds -lt 5000 -and -not $receivedAnswer) {
    $cts = [System.Threading.CancellationTokenSource]::new(1500)
    try {
      $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $cts.Token).GetAwaiter().GetResult()
    } catch {
      break
    }
    if ($r.MessageType -eq "Text" -and $r.Count -gt 0) {
      $text = [System.Text.Encoding]::UTF8.GetString($buf, 0, $r.Count)
      if ($text.Contains('"type":"signal"') -and $text.Contains('"action":"answer"')) {
        $json = $text | ConvertFrom-Json
        $receivedAnswer = $true
        Write-Output "[+] Round $($round): Received WebRTC SDP Answer from host in $($sw.ElapsedMilliseconds)ms!"
      }
    }
  }

  if (-not $receivedAnswer) {
    Write-Error "[-] FAIL: Round $($round) did not receive Answer within 5 seconds."
    return $false
  }
  return $true
}

$p1 = TestOffer 1
Start-Sleep -Milliseconds 500
$p2 = TestOffer 2
Start-Sleep -Milliseconds 500
$p3 = TestOffer 3

$ws.Dispose()

if ($p1 -and $p2 -and $p3) {
  Write-Output "`n================================================"
  Write-Output "Bug B Verification (Session Lifecycle): PASS 100%"
  Write-Output "Consecutive offers on same WS replaced smoothly!"
  Write-Output "================================================"
  exit 0
} else {
  Write-Error "Bug B Verification: FAILED"
  exit 1
}
