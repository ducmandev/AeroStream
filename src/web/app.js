(() => {
  // Elements
  const authModal = document.getElementById('auth-modal');
  const pinInput = document.getElementById('pin-input');
  const btnConnect = document.getElementById('btn-connect');
  const authError = document.getElementById('auth-error');
  const connectionOverlay = document.getElementById('connection-overlay');
  
  const canvas = document.getElementById('stream-canvas');
  const canvasWrapper = document.getElementById('canvas-wrapper');
  const webrtcVideo = document.getElementById('webrtc-video');
  const ctx = canvas.getContext('2d');

  // WebRTC Transport State (Phase 5)
  let peerConnection = null;
  let inputDataChannel = null;
  let isWebRtcActive = false;
  let webRtcStatsInterval = null;
  let webRtcConnectTimeout = null;

  function getVideoRect() {
    const isVideoShown = isWebRtcActive && webrtcVideo && webrtcVideo.style.display !== 'none';
    const targetElement = isVideoShown ? webrtcVideo : canvas;
    const rect = targetElement.getBoundingClientRect();
    const videoAspect = isVideoShown && webrtcVideo.videoWidth > 0 && webrtcVideo.videoHeight > 0
      ? (webrtcVideo.videoWidth / webrtcVideo.videoHeight)
      : ((canvas.width > 0 && canvas.height > 0) ? (canvas.width / canvas.height) : (16 / 9));
    const elementAspect = rect.width / (rect.height || 1);
    
    let renderWidth, renderHeight, offsetX, offsetY;
    if (elementAspect > videoAspect) {
      renderHeight = rect.height;
      renderWidth = renderHeight * videoAspect;
      offsetX = rect.left + (rect.width - renderWidth) / 2;
      offsetY = rect.top;
    } else {
      renderWidth = rect.width;
      renderHeight = renderWidth / videoAspect;
      offsetX = rect.left;
      offsetY = rect.top + (rect.height - renderHeight) / 2;
    }
    return { left: offsetX, top: offsetY, width: renderWidth, height: renderHeight };
  }
  
  const statLatency = document.getElementById('stat-latency');
  const statFps = document.getElementById('stat-fps');
  const statBitrate = document.getElementById('stat-bitrate');
  const hudResolution = document.getElementById('hud-resolution');
  
  const dynamicIsland = document.getElementById('dynamic-island');
  const shortcutsPanel = document.getElementById('shortcuts-panel');
  const btnShortcutsToggle = document.getElementById('btn-shortcuts-toggle');
  const btnCloseShortcuts = document.getElementById('btn-close-shortcuts');
  const btnTouchMode = document.getElementById('btn-touch-mode');
  const rememberPinCheck = document.getElementById('remember-pin');
  let isTouchDirectMode = true;
  let islandIdleTimer = null;

  const btnSettingsToggle = document.getElementById('btn-settings-toggle');
  const btnCloseSettings = document.getElementById('btn-close-settings');
  const settingsPanel = document.getElementById('settings-panel');
  const qualitySlider = document.getElementById('setting-quality');
  const qualityVal = document.getElementById('quality-val');
  const btnFullscreen = document.getElementById('btn-fullscreen');
  const hiddenInput = document.getElementById('hidden-keyboard-input');
  const btnAudioMute = document.getElementById('btn-audio-mute');
  const svgAudioOn = document.getElementById('svg-audio-on');
  const svgAudioOff = document.getElementById('svg-audio-off');

  // WebCodecs Audio Engine
  let audioCtx = null;
  let audioDecoder = null;
  let isAudioMuted = false;
  let nextAudioTime = 0;

  // Phase 4 Monotonic QPC Clock & A/V Sync Engine
  let serverClockOffset = 0;
  let estimatedRtt = 10;
  let lastAudioTimestamp = 0;
  let lastAudioCtxTime = 0;
  let pingInterval = null;

  function sendPing() {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'ping', client_time: Date.now() }));
    }
  }

  function handleTimeSync(serverTime) {
    if (serverTime > 0) {
      serverClockOffset = serverTime - Date.now();
      console.log(`[Phase 4 TimeSync] Server QPC clock synchronized, offset: ${serverClockOffset}ms`);
      sendPing();
    }
  }

  function handlePong(msg) {
    const now = Date.now();
    const sendTime = msg.client_time || msg.t || now;
    const rtt = Math.max(1, now - sendTime);
    estimatedRtt = rtt;
    if (msg.server_time) {
      const measuredOffset = (msg.server_time + Math.round(rtt / 2)) - now;
      serverClockOffset = serverClockOffset === 0 ? measuredOffset : Math.round(serverClockOffset * 0.7 + measuredOffset * 0.3);
    }
    if (statLatency) {
      statLatency.textContent = `${Math.round(rtt / 2)} ms`;
    }
  }

  function initAudio() {
    if (audioCtx && audioDecoder) return;
    try {
      const AudioCtxClass = window.AudioContext || window.webkitAudioContext;
      if (!AudioCtxClass) return;
      if (!audioCtx) {
        audioCtx = new AudioCtxClass({ sampleRate: 48000 });
      }

      if (typeof AudioDecoder !== 'undefined' && (!audioDecoder || audioDecoder.state === 'closed')) {
        audioDecoder = new AudioDecoder({
          output: (audioData) => {
            playAudioData(audioData);
          },
          error: (e) => {
            console.warn('WebCodecs AudioDecoder error:', e);
          }
        });
        audioDecoder.configure({
          codec: 'opus',
          sampleRate: 48000,
          numberOfChannels: 2
        });
        console.log('WebCodecs AudioDecoder initialized for Opus 48kHz stereo');
      }
    } catch (e) {
      console.warn('Failed to initialize AudioContext/AudioDecoder:', e);
    }
  }

  function playAudioData(audioData) {
    try {
      if (isAudioMuted || !audioCtx || audioCtx.state !== 'running') {
        audioData.close();
        return;
      }

      const channels = audioData.numberOfChannels;
      const frames = audioData.numberOfFrames;
      const sampleRate = audioData.sampleRate;

      const audioBuffer = audioCtx.createBuffer(channels, frames, sampleRate);
      for (let ch = 0; ch < channels; ch++) {
        const channelData = audioBuffer.getChannelData(ch);
        audioData.copyTo(channelData, { planeIndex: ch, format: 'f32' });
      }
      audioData.close();

      const source = audioCtx.createBufferSource();
      source.buffer = audioBuffer;
      source.connect(audioCtx.destination);

      const currentTime = audioCtx.currentTime;
      if (nextAudioTime < currentTime) {
        nextAudioTime = currentTime + 0.025;
      }
      source.start(nextAudioTime);
      nextAudioTime += audioBuffer.duration;
    } catch (e) {
      try { audioData.close(); } catch(err) {}
    }
  }

  function handleAudioPacket(buffer) {
    if (isWebRtcActive || isAudioMuted || buffer.byteLength < 10) return;

    if (!audioDecoder || audioDecoder.state !== 'configured') {
      initAudio();
      if (!audioDecoder || audioDecoder.state !== 'configured') return;
    }

    if (audioCtx && audioCtx.state === 'suspended') {
      audioCtx.resume().catch(() => {});
    }

    const view = new DataView(buffer);
    const timeMs = Number(view.getBigUint64(2));
    lastAudioTimestamp = timeMs;
    if (audioCtx) {
      lastAudioCtxTime = audioCtx.currentTime;
    }
    const opusData = new Uint8Array(buffer, 10);
    if (opusData.length === 0) return;

    try {
      const chunk = new EncodedAudioChunk({
        type: 'key',
        timestamp: timeMs * 1000,
        data: opusData
      });
      audioDecoder.decode(chunk);
    } catch (e) {
      // Decode error ignored
    }
  }

  function toggleAudioMute() {
    initAudio();
    isAudioMuted = !isAudioMuted;
    if (webrtcVideo) {
      webrtcVideo.muted = isAudioMuted;
    }
    if (btnAudioMute) {
      btnAudioMute.classList.toggle('muted', isAudioMuted);
    }
    if (svgAudioOn && svgAudioOff) {
      if (isAudioMuted) {
        svgAudioOn.classList.add('hidden');
        svgAudioOff.classList.remove('hidden');
      } else {
        svgAudioOn.classList.remove('hidden');
        svgAudioOff.classList.add('hidden');
        if (audioCtx && audioCtx.state === 'suspended') {
          audioCtx.resume().catch(() => {});
        }
      }
    }
  }

  // State
  let ws = null;
  let currentPin = '';
  let isConnected = false;

  // Metrics
  let frameCount = 0;
  let lastFpsTime = performance.now();
  let totalBytesReceived = 0;
  let lastBitrateTime = performance.now();

  let currentSessionToken = sessionStorage.getItem('aerostream_token');

  // 1. Initialization
  function init() {
    const params = new URLSearchParams(window.location.search);
    const pinFromUrl = params.get('pin');
    const savedPin = localStorage.getItem('aerostream_pin');

    if (pinFromUrl && pinFromUrl.length === 6) {
      pinInput.value = pinFromUrl;
      pairAndConnect(pinFromUrl);
    } else if (currentSessionToken) {
      connectWithToken(currentSessionToken);
    } else if (savedPin && savedPin.length === 6) {
      pinInput.value = savedPin;
      pairAndConnect(savedPin);
    }

    btnConnect.addEventListener('click', () => {
      initAudio();
      if (audioCtx && audioCtx.state === 'suspended') {
        audioCtx.resume().catch(() => {});
      }
      const pin = pinInput.value.trim();
      if (pin.length < 4) {
        authError.textContent = 'Please enter a valid 6-digit PIN';
        return;
      }
      pairAndConnect(pin);
    });

    pinInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') btnConnect.click();
    });

    setupControls();
    setupTouchTrackpad();
  }

  // WebCodecs H.264 Video Decoder
  let videoDecoder = null;
  let currentCodecWidth = 0;
  let currentCodecHeight = 0;
  let lastFrameLossReport = 0;
  let hasReceivedKeyframe = false;

  function reportFrameLoss() {
    const now = Date.now();
    if (now - lastFrameLossReport >= 200) {
      lastFrameLossReport = now;
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'frame_loss' }));
        console.log('[PLI] Reported frame loss to host -> requesting forced IDR keyframe');
      }
    }
  }

  function initVideoDecoder(w, h) {
    if (typeof VideoDecoder === 'undefined') return;
    hasReceivedKeyframe = false;
    try {
      if (videoDecoder && videoDecoder.state !== 'closed') {
        videoDecoder.close();
      }
      videoDecoder = new VideoDecoder({
        output: (frame) => {
          ctx.drawImage(frame, 0, 0);
          frame.close();
        },
        error: (e) => {
          console.warn('VideoDecoder hardware decode error:', e);
          hasReceivedKeyframe = false;
          reportFrameLoss();
        }
      });
      videoDecoder.configure({
        codec: h > 720 ? 'avc1.42002A' : 'avc1.420020', // Baseline Profile: Level 4.2 for 1080p, Level 3.2 for 720p
        optimizeForLatency: true,
      });
      currentCodecWidth = w;
      currentCodecHeight = h;
      console.log(`WebCodecs Hardware VideoDecoder initialized for ${w}x${h}`);
    } catch (e) {
      console.warn('WebCodecs VideoDecoder initialization failed:', e);
    }
  }

  // 2. Authentication & WebSocket Connection
  async function pairAndConnect(pin) {
    currentPin = pin;
    authError.textContent = '';
    connectionOverlay.classList.remove('hidden');

    try {
      const resp = await fetch('/api/auth/pair', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ pin, device: navigator.userAgent })
      });
      const data = await resp.json();
      if (resp.ok && data.status === 'ok' && data.token) {
        currentSessionToken = data.token;
        sessionStorage.setItem('aerostream_token', currentSessionToken);
        if (rememberPinCheck && rememberPinCheck.checked) {
          localStorage.setItem('aerostream_pin', pin);
        }
        // Cleanse PIN from URL address bar so it never leaks in history or screen recording (Phase 1.2)
        if (window.location.search) {
          window.history.replaceState({}, document.title, window.location.pathname);
        }
        connectWithToken(currentSessionToken);
      } else {
        connectionOverlay.classList.add('hidden');
        authError.textContent = data.message || 'Authentication failed. Please verify PIN.';
        authModal.classList.remove('hidden');
      }
    } catch (e) {
      console.warn('Pairing request failed, falling back to direct connection:', e);
      connect(pin);
    }
  }

  function connectWithToken(token) {
    authError.textContent = '';
    connectionOverlay.classList.remove('hidden');

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const supportsWebCodecs = typeof VideoDecoder !== 'undefined';
    const codecParam = supportsWebCodecs ? '&codec=h264' : '&codec=jpeg';
    const wsUrl = `${protocol}//${window.location.host}/ws?token=${encodeURIComponent(token)}${codecParam}`;
    openWebSocket(wsUrl);
  }

  function connect(pin) {
    currentPin = pin;
    authError.textContent = '';
    connectionOverlay.classList.remove('hidden');

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const supportsWebCodecs = typeof VideoDecoder !== 'undefined';
    const codecParam = supportsWebCodecs ? '&codec=h264' : '&codec=jpeg';
    const wsUrl = `${protocol}//${window.location.host}/ws?pin=${encodeURIComponent(pin)}${codecParam}`;
    openWebSocket(wsUrl);
  }

  function openWebSocket(wsUrl) {
    if (ws) {
      try { ws.close(); } catch(e) {}
    }

    ws = new WebSocket(wsUrl);
    ws.binaryType = 'arraybuffer';

    ws.onopen = () => {
      const supportsWebCodecs = typeof VideoDecoder !== 'undefined';
      console.log(`Connected to AeroStream Host (Requested Codec: ${supportsWebCodecs ? 'H.264' : 'JPEG'})`);
      isConnected = true;
      authModal.classList.add('hidden');
      connectionOverlay.classList.add('hidden');
      if (pingInterval) clearInterval(pingInterval);
      pingInterval = setInterval(sendPing, 4000);
      sendPing();

      // Initiate WebRTC peer connection (Phase 5)
      startWebRtc();
    };

    ws.onmessage = async (event) => {
      if (event.data instanceof ArrayBuffer) {
        if (isWebRtcActive) return; // Handled directly via WebRTC video/audio track!
        const bytes = new Uint8Array(event.data);
        if (bytes.length >= 10 && bytes[0] === 0xFA && bytes[1] === 0xFA) {
          handleAudioPacket(event.data);
        } else {
          handleBinaryFrame(event.data);
        }
      } else {
        try {
          const msg = JSON.parse(event.data);
          if (msg.type === 'signal') {
            handleSignalMessage(msg);
          } else if (msg.type === 'auth_failed') {
            sessionStorage.removeItem('aerostream_token');
            currentSessionToken = null;
            authError.textContent = msg.reason || 'Invalid PIN or session expired';
            authModal.classList.remove('hidden');
          } else if (msg.type === 'lock_status') {
            const lockBanner = document.getElementById('lock-banner');
            if (lockBanner) {
              if (msg.locked) {
                lockBanner.classList.remove('hidden');
              } else {
                lockBanner.classList.add('hidden');
              }
            }
          } else if (msg.type === 'time_sync') {
            handleTimeSync(msg.server_time);
          } else if (msg.type === 'pong') {
            handlePong(msg);
          } else if (msg.type === 'cursor') {
            const cursorOverlay = document.getElementById('cursor-overlay');
            if (cursorOverlay) {
              if (msg.visible) {
                cursorOverlay.classList.remove('hidden');
                const vRect = getVideoRect();
                const wrapperRect = canvasWrapper.getBoundingClientRect();
                const cursorLeft = (vRect.left - wrapperRect.left) + msg.x * vRect.width;
                const cursorTop = (vRect.top - wrapperRect.top) + msg.y * vRect.height;
                cursorOverlay.style.left = `${cursorLeft}px`;
                cursorOverlay.style.top = `${cursorTop}px`;
              } else {
                cursorOverlay.classList.add('hidden');
              }
            }
          }
        } catch (e) {}
      }
    };

    ws.onerror = (err) => {
      console.error('WebSocket error:', err);
      if (pingInterval) clearInterval(pingInterval);
      sessionStorage.removeItem('aerostream_token');
      currentSessionToken = null;
      authError.textContent = 'Unable to connect to host. Please verify PIN or re-authenticate.';
    };

    ws.onclose = () => {
      if (pingInterval) clearInterval(pingInterval);
      if (webRtcConnectTimeout) clearTimeout(webRtcConnectTimeout);
      if (webRtcStatsInterval) clearInterval(webRtcStatsInterval);
      if (peerConnection) {
        try { peerConnection.close(); } catch(e) {}
        peerConnection = null;
      }
      inputDataChannel = null;
      fallbackToWebSocket();
      isConnected = false;
      connectionOverlay.classList.remove('hidden');
      authModal.classList.remove('hidden');
      if (videoDecoder && videoDecoder.state !== 'closed') {
        try { videoDecoder.close(); } catch(e) {}
        videoDecoder = null;
      }
    };
  }

  // WebRTC Signaling & Transport Management (Phase 5)
  async function startWebRtc() {
    if (!window.RTCPeerConnection) {
      console.log('[WebRTC] RTCPeerConnection not supported by browser; remaining on WebSocket.');
      return;
    }

    try {
      if (peerConnection) {
        try { peerConnection.close(); } catch(e) {}
        peerConnection = null;
      }

      console.log('[WebRTC] Initiating PeerConnection negotiation with host...');
      const config = {
        iceServers: [
          { urls: 'stun:stun.l.google.com:19302' }
        ]
      };

      peerConnection = new RTCPeerConnection(config);

      // Fallback timer: if WebRTC cannot reach connected state in 4.5s, log and stay on WebSocket
      if (webRtcConnectTimeout) clearTimeout(webRtcConnectTimeout);
      webRtcConnectTimeout = setTimeout(() => {
        if (peerConnection && peerConnection.connectionState !== 'connected') {
          console.warn('[WebRTC] Connection negotiation timed out (4.5s); WebSocket fallback remains active.');
        }
      }, 4500);

      // Create unordered, zero-retransmit DataChannel for mouse and keyboard (zero HOL-blocking)
      inputDataChannel = peerConnection.createDataChannel('input', {
        ordered: false,
        maxRetransmits: 0
      });

      inputDataChannel.onopen = () => {
        console.log('[WebRTC DataChannel] Opened for zero-blocking input transmission');
      };
      inputDataChannel.onclose = () => {
        console.log('[WebRTC DataChannel] Closed');
      };
      inputDataChannel.onerror = (e) => {
        console.warn('[WebRTC DataChannel] Error:', e);
      };

      // Add transceivers for receiving video and audio
      peerConnection.addTransceiver('video', { direction: 'recvonly' });
      peerConnection.addTransceiver('audio', { direction: 'recvonly' });

      peerConnection.ontrack = (event) => {
        console.log('[WebRTC] Received remote track:', event.track.kind);
        if (event.track.kind === 'video') {
          if (webrtcVideo) {
            webrtcVideo.srcObject = event.streams[0] || new MediaStream([event.track]);
            webrtcVideo.style.display = 'block';
            canvas.style.opacity = '0';
            isWebRtcActive = true;
            webrtcVideo.play().catch(e => console.warn('[WebRTC Video] play() error:', e));
            console.log('[WebRTC Video] Video track active, rendering via hardware-accelerated <video>');
          }
        } else if (event.track.kind === 'audio') {
          if (webrtcVideo) {
            if (webrtcVideo.srcObject && event.streams[0]) {
              // Same stream
            } else if (webrtcVideo.srcObject) {
              webrtcVideo.srcObject.addTrack(event.track);
            } else {
              webrtcVideo.srcObject = new MediaStream([event.track]);
            }
          }
        }
      };

      peerConnection.onicecandidate = (event) => {
        if (event.candidate && ws && ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({
            type: 'signal',
            action: 'candidate',
            candidate: event.candidate.toJSON ? event.candidate.toJSON() : event.candidate
          }));
        }
      };

      peerConnection.onconnectionstatechange = () => {
        console.log('[WebRTC] State:', peerConnection.connectionState);
        if (peerConnection.connectionState === 'connected') {
          console.log('[WebRTC] Connection successfully established!');
          if (webRtcConnectTimeout) clearTimeout(webRtcConnectTimeout);
          if (webRtcStatsInterval) clearInterval(webRtcStatsInterval);
          webRtcStatsInterval = setInterval(updateWebRtcStats, 1000);
        } else if (peerConnection.connectionState === 'failed' || peerConnection.connectionState === 'disconnected') {
          console.warn('[WebRTC] Connection failed or disconnected; falling back to WebSocket.');
          fallbackToWebSocket();
        }
      };

      const offer = await peerConnection.createOffer();
      await peerConnection.setLocalDescription(offer);

      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({
          type: 'signal',
          action: 'offer',
          sdp: offer.sdp
        }));
        console.log('[WebRTC] Sent SDP Offer to host');
      }
    } catch (e) {
      console.warn('[WebRTC] Failed to initialize WebRTC:', e);
      fallbackToWebSocket();
    }
  }

  async function handleSignalMessage(msg) {
    if (!peerConnection) return;
    if (msg.action === 'answer') {
      try {
        await peerConnection.setRemoteDescription(new RTCSessionDescription({
          type: 'answer',
          sdp: msg.sdp
        }));
        console.log('[WebRTC] Received and applied SDP Answer from host');
      } catch (e) {
        console.error('[WebRTC] Failed to setRemoteDescription answer:', e);
      }
    } else if (msg.action === 'candidate') {
      try {
        let cand = msg.candidate;
        if (typeof cand === 'string') {
          cand = JSON.parse(cand);
        }
        await peerConnection.addIceCandidate(new RTCIceCandidate(cand));
        console.log('[WebRTC] Applied remote ICE candidate');
      } catch (e) {
        console.warn('[WebRTC] addIceCandidate error:', e);
      }
    }
  }

  function fallbackToWebSocket() {
    isWebRtcActive = false;
    if (webrtcVideo) {
      webrtcVideo.style.display = 'none';
      webrtcVideo.srcObject = null;
    }
    canvas.style.opacity = '1';
    if (webRtcStatsInterval) {
      clearInterval(webRtcStatsInterval);
      webRtcStatsInterval = null;
    }
  }

  let lastRtcBytes = 0;
  let lastRtcFrames = 0;
  let lastRtcTime = 0;

  async function updateWebRtcStats() {
    if (!peerConnection || peerConnection.connectionState !== 'connected') return;
    try {
      const stats = await peerConnection.getStats();
      let currentBytes = 0;
      let currentFrames = 0;
      let rtt = null;

      stats.forEach(report => {
        if (report.type === 'inbound-rtp' && report.kind === 'video') {
          currentBytes += (report.bytesReceived || 0);
          currentFrames += (report.framesDecoded || 0);
        }
        if (report.type === 'candidate-pair' && report.nominated) {
          if (typeof report.currentRoundTripTime === 'number') {
            rtt = Math.round(report.currentRoundTripTime * 1000);
          }
        }
      });

      const now = performance.now();
      if (lastRtcTime > 0 && now > lastRtcTime) {
        const dt = (now - lastRtcTime) / 1000;
        const dBytes = currentBytes - lastRtcBytes;
        const dFrames = currentFrames - lastRtcFrames;
        if (dt > 0 && dBytes >= 0) {
          const mbps = ((dBytes * 8) / dt / 1_000_000).toFixed(1);
          const fps = Math.round(dFrames / dt);
          if (statBitrate) statBitrate.textContent = `${mbps} Mbps`;
          if (statFps) statFps.textContent = `${fps} FPS`;
          if (rtt !== null && statLatency) {
            statLatency.textContent = `${rtt} ms`;
          }
        }
      }
      lastRtcBytes = currentBytes;
      lastRtcFrames = currentFrames;
      lastRtcTime = now;
    } catch (e) {}
  }

  if (webrtcVideo) {
    webrtcVideo.addEventListener('loadedmetadata', () => {
      if (hudResolution && webrtcVideo.videoWidth > 0) {
        hudResolution.textContent = `${webrtcVideo.videoWidth}x${webrtcVideo.videoHeight}`;
      }
    });
    webrtcVideo.addEventListener('resize', () => {
      if (hudResolution && webrtcVideo.videoWidth > 0) {
        hudResolution.textContent = `${webrtcVideo.videoWidth}x${webrtcVideo.videoHeight}`;
      }
    });
  }

  // 3. Render Binary Frame (WebCodecs H.264 or Fallback JPEG) & Compute Metrics
  async function handleBinaryFrame(buffer) {
    if (isWebRtcActive || buffer.byteLength < 12) return;

    totalBytesReceived += buffer.byteLength;

    const view = new DataView(buffer);
    const timestamp = Number(view.getBigUint64(0));
    const width = view.getUint16(8);
    const height = view.getUint16(10);

    // Latency using synchronized monotonic QPC host clock
    const now = Date.now();
    const hostNow = now + serverClockOffset;
    if (timestamp > 0) {
      const oneWayLatency = Math.max(1, Math.min(999, hostNow - timestamp));
      statLatency.textContent = `${oneWayLatency} ms`;
    }

    // Canvas resize
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
      hudResolution.textContent = `${height}p`;
      initVideoDecoder(width, height);
    }

    const payloadBytes = new Uint8Array(buffer, 12);
    if (payloadBytes.length === 0) return;

    // Check payload type: JPEG starts with 0xFF 0xD8
    const isJpeg = payloadBytes[0] === 0xFF && payloadBytes[1] === 0xD8;

    if (isJpeg) {
      // JPEG decode path
      const blob = new Blob([payloadBytes], { type: 'image/jpeg' });
      try {
        const bitmap = await createImageBitmap(blob);
        ctx.drawImage(bitmap, 0, 0);
        bitmap.close();
      } catch (e) {}
    } else {
      // H.264 Annex-B bitstream via WebCodecs VideoDecoder
      if (!videoDecoder || videoDecoder.state === 'closed' || currentCodecWidth !== width || currentCodecHeight !== height) {
        initVideoDecoder(width, height);
      }

      if (videoDecoder && videoDecoder.state === 'configured') {
        // Detect keyframe (NAL type 5 IDR or type 7 SPS)
        let isKey = false;
        const scanLimit = Math.min(512, payloadBytes.length - 4);
        for (let i = 0; i < scanLimit; i++) {
          if (payloadBytes[i] === 0 && payloadBytes[i+1] === 0) {
            let nalOffset = -1;
            if (payloadBytes[i+2] === 1) {
              nalOffset = i + 3;
            } else if (payloadBytes[i+2] === 0 && payloadBytes[i+3] === 1) {
              nalOffset = i + 4;
            }
            if (nalOffset >= 0 && nalOffset < payloadBytes.length) {
              const nalType = payloadBytes[nalOffset] & 0x1F;
              if (nalType === 5 || nalType === 7) {
                isKey = true;
                break;
              }
            }
          }
        }
        if (!hasReceivedKeyframe) {
          if (!isKey) {
            reportFrameLoss();
            return;
          }
          hasReceivedKeyframe = true;
        }

        try {
          const chunk = new EncodedVideoChunk({
            type: isKey ? 'key' : 'delta',
            timestamp: timestamp * 1000,
            data: payloadBytes
          });
          videoDecoder.decode(chunk);
        } catch (e) {
          console.warn('WebCodecs decode exception, reporting frame loss:', e);
          hasReceivedKeyframe = false;
          reportFrameLoss();
        }
      }
    }

    // FPS
    frameCount++;
    const nowPerf = performance.now();
    if (nowPerf - lastFpsTime >= 1000) {
      statFps.textContent = `${frameCount} fps`;
      frameCount = 0;
      lastFpsTime = nowPerf;
    }

    // Bitrate
    if (nowPerf - lastBitrateTime >= 1000) {
      const mbps = ((totalBytesReceived * 8) / ((nowPerf - lastBitrateTime) / 1000) / 1_000_000).toFixed(1);
      statBitrate.textContent = `${mbps} Mbps`;
      totalBytesReceived = 0;
      lastBitrateTime = nowPerf;
    }
  }

  // 4. Send Input Command (Priority: WebRTC DataChannel with zero HOL-blocking, fallback to WebSocket)
  function sendInput(payload) {
    if (inputDataChannel && inputDataChannel.readyState === 'open') {
      inputDataChannel.send(JSON.stringify(payload));
      return;
    }
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(payload));
    }
  }

  // 5. Controls & Toolbar
  function setupControls() {
    function resetIslandTimer() {
      if (!dynamicIsland) return;
      dynamicIsland.classList.remove('island-collapsed');
      clearTimeout(islandIdleTimer);

      const isPanelOpen = (settingsPanel && !settingsPanel.classList.contains('hidden')) ||
                          (shortcutsPanel && !shortcutsPanel.classList.contains('hidden'));
      if (isPanelOpen) return;

      islandIdleTimer = setTimeout(() => {
        if (isConnected) {
          dynamicIsland.classList.add('island-collapsed');
        }
      }, 3500);
    }

    // Reveal island when mouse hovers near top edge
    window.addEventListener('mousemove', (e) => {
      if (e.clientY < 55) {
        if (dynamicIsland) dynamicIsland.classList.remove('island-collapsed');
      }
      resetIslandTimer();
    });

    window.addEventListener('touchstart', () => resetIslandTimer(), { passive: true });
    window.addEventListener('keydown', () => resetIslandTimer());

    // Settings Toggle
    btnSettingsToggle.addEventListener('click', (e) => {
      e.stopPropagation();
      settingsPanel.classList.toggle('hidden');
      if (shortcutsPanel) shortcutsPanel.classList.add('hidden');
      resetIslandTimer();
    });

    btnCloseSettings.addEventListener('click', () => {
      settingsPanel.classList.add('hidden');
      resetIslandTimer();
    });

    // Shortcuts Menu Toggle
    if (btnShortcutsToggle && shortcutsPanel) {
      btnShortcutsToggle.addEventListener('click', (e) => {
        e.stopPropagation();
        shortcutsPanel.classList.toggle('hidden');
        if (settingsPanel) settingsPanel.classList.add('hidden');
        resetIslandTimer();
      });
    }

    if (btnCloseShortcuts) {
      btnCloseShortcuts.addEventListener('click', () => {
        shortcutsPanel.classList.add('hidden');
        resetIslandTimer();
      });
    }

    // Touch / Trackpad Mode Toggle
    if (btnTouchMode) {
      btnTouchMode.addEventListener('click', () => {
        isTouchDirectMode = !isTouchDirectMode;
        btnTouchMode.classList.toggle('active', !isTouchDirectMode);
        btnTouchMode.title = isTouchDirectMode ? 'Touch Mode: Direct Tap' : 'Touch Mode: Virtual Trackpad';
        resetIslandTimer();
      });
    }

    // Close floating cards when clicking outside
    document.addEventListener('click', (e) => {
      if (shortcutsPanel && !shortcutsPanel.classList.contains('hidden')) {
        if (!shortcutsPanel.contains(e.target) && e.target !== btnShortcutsToggle && !btnShortcutsToggle.contains(e.target)) {
          shortcutsPanel.classList.add('hidden');
          resetIslandTimer();
        }
      }
      if (settingsPanel && !settingsPanel.classList.contains('hidden')) {
        if (!settingsPanel.contains(e.target) && e.target !== btnSettingsToggle && !btnSettingsToggle.contains(e.target)) {
          settingsPanel.classList.add('hidden');
          resetIslandTimer();
        }
      }
    });

    // Fullscreen Toggle
    btnFullscreen.addEventListener('click', () => {
      if (!document.fullscreenElement) {
        document.documentElement.requestFullscreen().catch(() => {});
      } else {
        document.exitFullscreen().catch(() => {});
      }
      resetIslandTimer();
    });

    // Audio Mute Toggle
    if (btnAudioMute) {
      btnAudioMute.addEventListener('click', () => {
        toggleAudioMute();
        resetIslandTimer();
      });
    }

    // Quick System Shortcuts
    const btnWin = document.getElementById('tb-win-key');
    if (btnWin) {
      btnWin.addEventListener('click', () => {
        sendInput({ type: 'key_click', key: 'win' });
      });
    }

    const btnEsc = document.getElementById('tb-esc-key');
    if (btnEsc) {
      btnEsc.addEventListener('click', () => {
        sendInput({ type: 'key_click', key: 'escape' });
      });
    }

    const btnCtrlAltDel = document.getElementById('btn-ctrl-alt-del');
    if (btnCtrlAltDel) {
      btnCtrlAltDel.addEventListener('click', () => {
        sendInput({ type: 'shortcut', keys: ['ctrl', 'alt', 'delete'] });
        if (shortcutsPanel) shortcutsPanel.classList.add('hidden');
      });
    }

    const btnAltTab = document.getElementById('btn-alt-tab');
    if (btnAltTab) {
      btnAltTab.addEventListener('click', () => {
        sendInput({ type: 'shortcut', keys: ['alt', 'tab'] });
      });
    }

    const btnTaskmgr = document.getElementById('btn-taskmgr');
    if (btnTaskmgr) {
      btnTaskmgr.addEventListener('click', () => {
        sendInput({ type: 'shortcut', keys: ['ctrl', 'shift', 'escape'] });
        if (shortcutsPanel) shortcutsPanel.classList.add('hidden');
      });
    }

    // Mouse Assist Buttons
    const btnLeftClick = document.getElementById('tb-left-click');
    if (btnLeftClick) {
      btnLeftClick.addEventListener('click', () => {
        sendInput({ type: 'mouse_click', button: 'left' });
      });
    }

    const btnRightClick = document.getElementById('tb-right-click');
    if (btnRightClick) {
      btnRightClick.addEventListener('click', () => {
        sendInput({ type: 'mouse_click', button: 'right' });
      });
    }

    const btnDoubleClick = document.getElementById('btn-double-click');
    if (btnDoubleClick) {
      btnDoubleClick.addEventListener('click', () => {
        sendInput({ type: 'mouse_double', button: 'left' });
      });
    }

    // Virtual Keyboard toggle
    const btnKeyboard = document.getElementById('tb-keyboard');
    if (btnKeyboard) {
      btnKeyboard.addEventListener('click', () => {
        hiddenInput.focus();
      });
    }

    hiddenInput.addEventListener('input', (e) => {
      if (e.data) {
        sendInput({ type: 'text', text: e.data });
      }
      hiddenInput.value = '';
    });

    hiddenInput.addEventListener('keydown', (e) => {
      if (e.key === 'Backspace') {
        sendInput({ type: 'key_click', key: 'backspace' });
      } else if (e.key === 'Enter') {
        sendInput({ type: 'key_click', key: 'enter' });
      }
    });

    // Preset mode selector
    document.querySelectorAll('#mode-pills .pill-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('#mode-pills .pill-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        const mode = btn.getAttribute('data-mode');
        sendInput({ type: 'set_mode', mode });
        if (mode === 'eco') {
          qualitySlider.value = 55;
          qualityVal.textContent = '55%';
        } else if (mode === 'balanced') {
          qualitySlider.value = 68;
          qualityVal.textContent = '68%';
        } else if (mode === 'quality') {
          qualitySlider.value = 80;
          qualityVal.textContent = '80%';
        }
      });
    });

    // Framerate selector
    document.querySelectorAll('#fps-pills .pill-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('#fps-pills .pill-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        const fps = parseInt(btn.getAttribute('data-fps'), 10);
        sendInput({ type: 'set_fps', fps });
      });
    });

    // Resolution selector
    document.querySelectorAll('#res-pills .pill-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('#res-pills .pill-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        const res = parseInt(btn.getAttribute('data-res'), 10);
        sendInput({ type: 'set_resolution', height: res });
      });
    });

    // Quality slider
    qualitySlider.addEventListener('input', (e) => {
      const q = parseInt(e.target.value, 10);
      qualityVal.textContent = `${q}%`;
      sendInput({ type: 'set_quality', quality: q });
    });
  }

  // 6. Smooth Mouse & Touch Trackpad Gestures
  function setupTouchTrackpad() {
    let touchStartX = 0;
    let touchStartY = 0;
    let touchStartTime = 0;
    let lastTouchX = 0;
    let lastTouchY = 0;
    let isDragging = false;
    let lastTapTime = 0;
    let isLeftDown = false;

    // Desktop mouse events
    canvas.addEventListener('mousemove', (e) => {
      const vRect = getVideoRect();
      const normX = Math.max(0, Math.min(1, (e.clientX - vRect.left) / vRect.width));
      const normY = Math.max(0, Math.min(1, (e.clientY - vRect.top) / vRect.height));
      sendInput({ type: 'mouse_move', x: normX, y: normY });
    });

    canvas.addEventListener('mousedown', (e) => {
      const vRect = getVideoRect();
      const normX = Math.max(0, Math.min(1, (e.clientX - vRect.left) / vRect.width));
      const normY = Math.max(0, Math.min(1, (e.clientY - vRect.top) / vRect.height));
      const btn = e.button === 2 ? 'right' : e.button === 1 ? 'middle' : 'left';
      sendInput({ type: 'mouse_down', button: btn, x: normX, y: normY });
    });

    canvas.addEventListener('mouseup', (e) => {
      const vRect = getVideoRect();
      const normX = Math.max(0, Math.min(1, (e.clientX - vRect.left) / vRect.width));
      const normY = Math.max(0, Math.min(1, (e.clientY - vRect.top) / vRect.height));
      const btn = e.button === 2 ? 'right' : e.button === 1 ? 'middle' : 'left';
      sendInput({ type: 'mouse_up', button: btn, x: normX, y: normY });
    });

    canvas.addEventListener('dblclick', (e) => {
      const vRect = getVideoRect();
      const normX = Math.max(0, Math.min(1, (e.clientX - vRect.left) / vRect.width));
      const normY = Math.max(0, Math.min(1, (e.clientY - vRect.top) / vRect.height));
      sendInput({ type: 'mouse_double', button: 'left', x: normX, y: normY });
    });

    canvas.addEventListener('mouseleave', () => {
      const cursorOverlay = document.getElementById('cursor-overlay');
      if (cursorOverlay) cursorOverlay.classList.add('hidden');
    });

    canvas.addEventListener('contextmenu', (e) => e.preventDefault());

    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      sendInput({ type: 'mouse_wheel', delta_x: Math.round(e.deltaX), delta_y: -Math.round(e.deltaY) });
    }, { passive: false });

    // Touch events for Mobile / Tablet Trackpad & Direct Click
    canvas.addEventListener('touchstart', (e) => {
      if (e.touches.length === 1) {
        const t = e.touches[0];
        touchStartX = t.clientX;
        touchStartY = t.clientY;
        lastTouchX = t.clientX;
        lastTouchY = t.clientY;
        touchStartTime = performance.now();
        isDragging = false;

        // Double-tap to drag support
        const now = performance.now();
        if (now - lastTapTime < 280) {
          isLeftDown = true;
          const vRect = getVideoRect();
          const normX = Math.max(0, Math.min(1, (t.clientX - vRect.left) / vRect.width));
          const normY = Math.max(0, Math.min(1, (t.clientY - vRect.top) / vRect.height));
          sendInput({ type: 'mouse_down', button: 'left', x: normX, y: normY });
        }
        lastTapTime = now;
      } else if (e.touches.length === 2) {
        lastTouchY = (e.touches[0].clientY + e.touches[1].clientY) / 2;
      }
    }, { passive: true });

    canvas.addEventListener('touchmove', (e) => {
      if (e.touches.length === 1) {
        const t = e.touches[0];
        const dx = t.clientX - lastTouchX;
        const dy = t.clientY - lastTouchY;
        lastTouchX = t.clientX;
        lastTouchY = t.clientY;

        if (Math.abs(t.clientX - touchStartX) > 3 || Math.abs(t.clientY - touchStartY) > 3) {
          isDragging = true;
          if (isTouchDirectMode) {
            const vRect = getVideoRect();
            const normX = Math.max(0, Math.min(1, (t.clientX - vRect.left) / vRect.width));
            const normY = Math.max(0, Math.min(1, (t.clientY - vRect.top) / vRect.height));
            sendInput({ type: 'mouse_move', x: normX, y: normY });
          } else {
            sendInput({ type: 'mouse_delta', dx: Math.round(dx * 1.5), dy: Math.round(dy * 1.5) });
          }
        }
      } else if (e.touches.length === 2) {
        // Two-finger scroll
        const currentY = (e.touches[0].clientY + e.touches[1].clientY) / 2;
        const dy = currentY - lastTouchY;
        lastTouchY = currentY;
        sendInput({ type: 'mouse_wheel', delta_x: 0, delta_y: Math.round(dy * 3) });
      }
    }, { passive: true });

    canvas.addEventListener('touchend', (e) => {
      const vRect = getVideoRect();
      const normX = Math.max(0, Math.min(1, (touchStartX - vRect.left) / vRect.width));
      const normY = Math.max(0, Math.min(1, (touchStartY - vRect.top) / vRect.height));

      if (isLeftDown) {
        isLeftDown = false;
        sendInput({ type: 'mouse_up', button: 'left', x: normX, y: normY });
      }

      if (e.touches.length === 0 && !isDragging) {
        const elapsed = performance.now() - touchStartTime;
        if (elapsed < 300) {
          if (isTouchDirectMode) {
            sendInput({ type: 'mouse_click', button: 'left', x: normX, y: normY });
          } else {
            sendInput({ type: 'mouse_click', button: 'left' });
          }
        }
      } else if (e.touches.length === 1 && !isDragging) {
        if (isTouchDirectMode) {
          sendInput({ type: 'mouse_click', button: 'right', x: normX, y: normY });
        } else {
          sendInput({ type: 'mouse_click', button: 'right' });
        }
      }
    }, { passive: true });
  }

  init();
})();
