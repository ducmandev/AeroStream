async function run() {
  const pin = '885095';
  const url = `ws://127.0.0.1:8080/ws?pin=${pin}&codec=h264`;
  console.log(`Connecting to ${url}...`);

  const ws = new WebSocket(url);
  ws.binaryType = 'arraybuffer';

  let audioPackets = 0;
  let videoPackets = 0;
  let audioBytesTotal = 0;
  let firstAudioTime = 0;

  ws.onopen = () => {
    console.log('[+] WebSocket connected successfully!');
  };

  ws.onmessage = (event) => {
    if (event.data instanceof ArrayBuffer) {
      const buf = Buffer.from(event.data);
      if (buf.length >= 10 && buf[0] === 0xFA && buf[1] === 0xFA) {
        audioPackets++;
        audioBytesTotal += buf.length;
        if (!firstAudioTime) firstAudioTime = Date.now();
        const timestamp = buf.readBigUInt64BE(2);
        const payloadLen = buf.length - 10;
        if (audioPackets <= 5 || audioPackets % 50 === 0) {
          console.log(`[Audio Packet #${audioPackets}] Time: ${timestamp}, Payload Size: ${payloadLen} bytes`);
        }
      } else if (buf.length >= 12) {
        videoPackets++;
        if (videoPackets === 1 || videoPackets % 30 === 0) {
          const w = buf.readUInt16BE(8);
          const h = buf.readUInt16BE(10);
          console.log(`[Video Frame #${videoPackets}] Res: ${w}x${h}, Size: ${buf.length} bytes`);
        }
      }
    } else {
      console.log('[Text message]:', event.data);
    }
  };

  ws.onerror = (err) => {
    console.error('[-] WebSocket error:', err);
  };

  // Run test for 3.5 seconds
  await new Promise(r => setTimeout(r, 3500));
  ws.close();

  console.log('\n=== TEST RESULTS ===');
  console.log(`Total Video Frames: ${videoPackets}`);
  console.log(`Total Audio Packets (0xFA 0xFA): ${audioPackets}`);
  console.log(`Total Audio Bytes: ${audioBytesTotal} bytes`);
  if (audioPackets > 10) {
    const elapsedSec = (Date.now() - firstAudioTime) / 1000;
    const hz = audioPackets / elapsedSec;
    console.log(`Audio Packet Rate: ${hz.toFixed(1)} Hz (Expected ~50 Hz for 20ms frames)`);
    console.log('[+] Phase 2 Audio Stream Verification: SUCCESS!');
    process.exit(0);
  } else {
    console.error('[-] Phase 2 Audio Stream Verification: FAILED (Insufficient audio packets)');
    process.exit(1);
  }
}

run();
