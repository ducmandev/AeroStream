const http = require('http');

async function getStatus() {
  return new Promise((resolve, reject) => {
    http.get('http://127.0.0.1:8080/api/status', (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => resolve(JSON.parse(data)));
    }).on('error', reject);
  });
}

async function pairPin(pin) {
  return new Promise((resolve, reject) => {
    const payload = JSON.stringify({ pin, device: 'Phase 1 Verification Script' });
    const req = http.request('http://127.0.0.1:8080/api/auth/pair', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(payload)
      }
    }, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        resolve({ statusCode: res.statusCode, body: JSON.parse(data) });
      });
    });
    req.on('error', reject);
    req.write(payload);
    req.end();
  });
}

function connectWs(query) {
  return new Promise((resolve) => {
    const ws = new WebSocket(`ws://127.0.0.1:8080/ws?${query}`);
    ws.binaryType = 'arraybuffer';
    let opened = false;
    ws.onopen = () => {
      opened = true;
      resolve({ success: true, ws });
    };
    ws.onerror = (err) => {
      if (!opened) resolve({ success: false, error: err.message });
    };
    setTimeout(() => {
      if (!opened) resolve({ success: false, error: 'timeout' });
    }, 3000);
  });
}

async function runTests() {
  console.log('=== Starting Phase 1 Verification Suite ===');
  const status = await getStatus();
  const validPin = status.pin;
  console.log(`Server Online. Valid PIN: ${validPin}`);

  // 1. Test POST /api/auth/pair with wrong PIN
  console.log('\n[Test 1.2.A] Testing Pairing API with invalid PIN...');
  const failPair = await pairPin('000000');
  console.log(`  Invalid PIN status code: ${failPair.statusCode} (expected 401)`);
  console.log(`  Response body:`, failPair.body);
  if (failPair.statusCode !== 401) {
    throw new Error('Expected 401 for wrong PIN');
  }

  // 2. Test POST /api/auth/pair with valid PIN
  console.log('\n[Test 1.2.B] Testing Pairing API with valid PIN...');
  const successPair = await pairPin(validPin);
  console.log(`  Valid PIN status code: ${successPair.statusCode} (expected 200)`);
  console.log(`  Issued session token: ${successPair.body.token.substring(0, 16)}... (expires in: ${successPair.body.expires_in}s)`);
  if (!successPair.body.token) {
    throw new Error('No token returned from pairing endpoint');
  }
  const sessionToken = successPair.body.token;

  // 3. Connect WebSocket using Session Token (NO PIN in URL)
  console.log('\n[Test 1.2.C] Connecting WebSocket using Session Token (NO PIN IN URL)...');
  const tokenConn = await connectWs(`token=${encodeURIComponent(sessionToken)}&codec=h264`);
  if (!tokenConn.success) {
    throw new Error(`Failed to connect with session token: ${tokenConn.error}`);
  }
  console.log('  Successfully connected via Session Token!');

  let tokenFrames = 0;
  tokenConn.ws.onmessage = (event) => {
    if (event.data instanceof ArrayBuffer && event.data.byteLength >= 12) {
      tokenFrames++;
    }
  };

  await new Promise(r => setTimeout(r, 1000));
  console.log(`  Stream frames received via Token Auth: ${tokenFrames} frames`);
  tokenConn.ws.close();

  // 4. Test Backward Compatibility: connect with ?pin=...
  console.log('\n[Test 1.2.D] Testing Backward Compatibility: connect with ?pin=...');
  const pinConn = await connectWs(`pin=${encodeURIComponent(validPin)}&codec=h264`);
  if (!pinConn.success) {
    throw new Error(`Failed to connect with legacy PIN: ${pinConn.error}`);
  }
  console.log('  Successfully connected via legacy PIN query!');
  pinConn.ws.close();

  // 5. Test Invalid Token
  console.log('\n[Test 1.2.E] Testing Connection with Fake/Expired Token...');
  const fakeConn = await connectWs(`token=invalidtoken1234567890abcdef&codec=h264`);
  console.log(`  Invalid token connection success: ${fakeConn.success} (expected false)`);
  if (fakeConn.success) {
    throw new Error('Connection should be rejected with invalid token');
  }

  console.log('\n=== ALL PHASE 1.2 TOKEN PAIRING TESTS PASSED! ===');
}

runTests().catch(err => {
  console.error('Test failed:', err);
  process.exit(1);
});
