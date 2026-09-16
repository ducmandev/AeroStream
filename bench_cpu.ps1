param([string]$Pin = "", [int]$Seconds = 6)

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

Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.Form
$f.TopMost = $true; $f.Size = New-Object System.Drawing.Size(400,150); $f.StartPosition = "Manual"; $f.Location = New-Object System.Drawing.Point(50,50)
$l = New-Object System.Windows.Forms.Label
$l.Font = New-Object System.Drawing.Font("Consolas", 20); $l.Dock = "Fill"; $f.Controls.Add($l)
$t = New-Object System.Windows.Forms.Timer
$t.Interval = 16
$n = 0
$t.Add_Tick({ $n++; $l.Text = "AERO-BENCH $n $(Get-Date -Format fff)" })
$t.Start()
$f.Show()
Start-Sleep -Milliseconds 500

$p = Get-Process -Name aerostream*, *aerostream* -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) {
  Write-Error "aerostream process not found!"
  $t.Stop(); $f.Close()
  exit 1
}
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
$t.Stop(); $f.Close()
Write-Output ("BENCH 60Hz: frames={0} fps={1} avgKB={2}" -f $frames, [math]::Round($frames/$Seconds,1), [math]::Round($bytes/1024/[math]::Max($frames,1),1))
Write-Output ("CPU: " + [math]::Round(($p.CPU - $c0)/$Seconds*100,1) + "% of one core (baseline 3.1: 54.7% @ 50fps)")
