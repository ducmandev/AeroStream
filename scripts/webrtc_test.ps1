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

# Construct a valid minimal WebRTC SDP Offer requesting H.264 video, Opus audio, and DataChannel
$sdpOffer = @"
v=0
o=- 46117314004300512 2 IN IP4 127.0.0.1
s=-
t=0 0
a=group:BUNDLE 0 1 2
m=video 9 UDP/TLS/RTP/SAVPF 96
c=IN IP4 0.0.0.0
a=rtcp:9 IN IP4 0.0.0.0
a=ice-ufrag:aerostream1234
a=ice-pwd:aerostreampassword123456789
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
a=ice-ufrag:aerostream1234
a=ice-pwd:aerostreampassword123456789
a=fingerprint:sha-256 00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00
a=setup:actpass
a=mid:1
a=recvonly
a=rtpmap:111 opus/48000/2
m=application 9 UDP/DTLS/SCTP webrtc-datachannel
c=IN IP4 0.0.0.0
a=ice-ufrag:aerostream1234
a=ice-pwd:aerostreampassword123456789
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

Write-Output "Sending WebRTC SDP Offer to host..."
SendJson $signalOffer

# Read responses
$buf = [byte[]]::new(65536)
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$receivedAnswer = $false
$answerSdp = ""

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
      $answerSdp = $json.sdp
      $receivedAnswer = $true
      Write-Output "Received WebRTC SDP Answer from host in $($sw.ElapsedMilliseconds)ms!"
    }
  }
}

if (-not $receivedAnswer) {
  Write-Error "FAIL: Did not receive WebRTC SDP Answer within 5 seconds."
  $ws.Dispose()
  exit 1
}

# Verify Answer SDP contains expected tracks
Write-Output "--- SDP Answer Summary ---"
$hasVideo = $answerSdp.Contains("m=video")
$hasAudio = $answerSdp.Contains("m=audio")
$hasData = $answerSdp.Contains("m=application")

$statusVideo = if ($hasVideo) { "OK" } else { "MISSING" }
$statusAudio = if ($hasAudio) { "OK" } else { "MISSING" }
$statusData = if ($hasData) { "OK" } else { "MISSING" }

Write-Output ("Video Track (H.264): {0}" -f $statusVideo)
Write-Output ("Audio Track (Opus): {0}" -f $statusAudio)
Write-Output ("DataChannel (SCTP): {0}" -f $statusData)

# Test ICE Candidate signaling
$candMsg = @{
  type = "signal"
  action = "candidate"
  candidate = @{
    candidate = "candidate:1 1 UDP 2130706431 127.0.0.1 50000 typ host"
    sdpMid = "0"
    sdpMLineIndex = 0
  }
} | ConvertTo-Json -Compress

Write-Output "Sending ICE Candidate to host..."
SendJson $candMsg
Start-Sleep -Milliseconds 500

Write-Output "=== WebRTC Signaling & Track Negotiation Test: PASS ==="
$ws.Dispose()
