use crate::capture_dxgi::{DxgiDuplicationCapture, SendWrapper};
use crate::codec_h264::H264EncoderWrapper;
use crate::config::DynamicStreamSettings;
use bytes::{BufMut, Bytes, BytesMut};
use jpeg_encoder::{ColorType, Encoder};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::info;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub enum FramePayload {
    GpuD3D11 {
        texture: SendWrapper<ID3D11Texture2D>,
        d3d_manager: SendWrapper<IMFDXGIDeviceManager>,
    },
    CpuBgra {
        pixels: Vec<u8>,
    },
}

pub struct RawFrame {
    pub payload: FramePayload,
    pub width: u32,
    pub height: u32,
    pub jpeg_bgra: Option<Vec<u8>>,
    pub _frame_id: u64,
}

pub struct CaptureEngine {
    settings: Arc<DynamicStreamSettings>,
    frame_sender: broadcast::Sender<Bytes>,
    h264_sender: broadcast::Sender<Bytes>,
    cursor_sender: broadcast::Sender<String>,
    current_fps: Arc<AtomicU32>,
    latest_frame: Arc<RwLock<Option<Bytes>>>,
    latest_h264_keyframe: Arc<RwLock<Option<Bytes>>>,
    request_h264_keyframe: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl CaptureEngine {
    pub fn new(
        settings: Arc<DynamicStreamSettings>,
        frame_sender: broadcast::Sender<Bytes>,
        h264_sender: broadcast::Sender<Bytes>,
        cursor_sender: broadcast::Sender<String>,
        current_fps: Arc<AtomicU32>,
        latest_frame: Arc<RwLock<Option<Bytes>>>,
        latest_h264_keyframe: Arc<RwLock<Option<Bytes>>>,
        request_h264_keyframe: Arc<AtomicBool>,
    ) -> Self {
        Self {
            settings,
            frame_sender,
            h264_sender,
            cursor_sender,
            current_fps,
            latest_frame,
            latest_h264_keyframe,
            request_h264_keyframe,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn start(self: Arc<Self>) {
        let (raw_tx, mut raw_rx) = tokio::sync::mpsc::channel::<RawFrame>(1);
        let (recycle_tx, recycle_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);

        // Pre-allocate 2 buffers to seed the zero-allocation pool
        let _ = recycle_tx.try_send(Vec::with_capacity(1920 * 1080 * 4));
        let _ = recycle_tx.try_send(Vec::with_capacity(1920 * 1080 * 4));

        // 1. Single Streamlined Capture Worker (DXGI GPU + GDI Fallback)
        let running = self.running.clone();
        let settings = self.settings.clone();
        let cursor_sender = self.cursor_sender.clone();
        let tx = raw_tx.clone();
        let rec_tx_for_worker = recycle_tx.clone();
        let frame_sender_for_cap = self.frame_sender.clone();
        let h264_sender_for_cap = self.h264_sender.clone();
        let request_h264_keyframe_for_cap = self.request_h264_keyframe.clone();

        std::thread::Builder::new()
            .name("screen-capture-worker".into())
            .spawn(move || {
                Self::run_capture_worker(
                    running,
                    settings,
                    cursor_sender,
                    tx,
                    recycle_rx,
                    rec_tx_for_worker,
                    frame_sender_for_cap,
                    h264_sender_for_cap,
                    request_h264_keyframe_for_cap,
                );
            })
            .expect("Failed to spawn screen capture worker");

        // 2. Dedicated Compression & Broadcast Worker (H.264 + JPEG Dual Pipeline)
        let running = self.running.clone();
        let settings = self.settings.clone();
        let frame_sender = self.frame_sender.clone();
        let h264_sender = self.h264_sender.clone();
        let current_fps = self.current_fps.clone();
        let latest_frame = self.latest_frame.clone();
        let latest_h264_keyframe = self.latest_h264_keyframe.clone();
        let request_h264_keyframe = self.request_h264_keyframe.clone();

        std::thread::Builder::new()
            .name("screen-encoder-worker".into())
            .spawn(move || {
                Self::run_encoder_worker(
                    running,
                    settings,
                    &mut raw_rx,
                    recycle_tx,
                    frame_sender,
                    h264_sender,
                    current_fps,
                    latest_frame,
                    latest_h264_keyframe,
                    request_h264_keyframe,
                );
            })
            .expect("Failed to spawn screen encoder worker");

        info!("Ultra-low-latency Hybrid DXGI/GDI Capture Engine started successfully.");
    }

    /// Dedicated capture worker with DXGI GPU capture & automatic GDI fallback
    fn run_capture_worker(
        running: Arc<AtomicBool>,
        settings: Arc<DynamicStreamSettings>,
        cursor_sender: broadcast::Sender<String>,
        tx: tokio::sync::mpsc::Sender<RawFrame>,
        mut recycle_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
        _recycle_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
        frame_sender: broadcast::Sender<Bytes>,
        h264_sender: broadcast::Sender<Bytes>,
        request_h264_keyframe: Arc<AtomicBool>,
    ) {
        unsafe {
            let input_desk = OpenInputDesktop(0, 0, 0x10000000);
            if !input_desk.is_null() {
                SetThreadDesktop(input_desk);
            }
        }

        // Try initializing Direct DXGI Duplication on current desktop thread
        let mut dxgi_cap: Option<DxgiDuplicationCapture> = match DxgiDuplicationCapture::new() {
            Ok(c) => {
                info!("DXGI Desktop Duplication API initialized successfully! (GPU Compositor Capture)");
                Some(c)
            }
            Err(e) => {
                info!("DXGI initialization failed ({:?}), using high-speed GDI DIBSection fallback", e);
                None
            }
        };

        let mut last_dxgi_retry = Instant::now();
        let mut last_cursor_x = -1.0f32;
        let mut last_cursor_y = -1.0f32;
        let mut last_lock_check = Instant::now();
        let mut last_lock_state = false;

        let mut current_target_w = 0u32;
        let mut current_target_h = 0u32;
        let mut hdc_screen: HDC = null_mut();
        let mut hdc_mem: HDC = null_mut();
        let mut hbm: HBITMAP = null_mut();
        let mut old_obj: HGDIOBJ = null_mut();
        let mut ppv_bits: *mut u8 = null_mut();
        let mut frame_seq = 0u64;
        let mut last_hash = 0u64;
        let mut last_send_time = Instant::now() - Duration::from_secs(1);
        let mut last_had_subscribers = false;

        while running.load(Ordering::Relaxed) {
            // Phase 3.1: Full Idle-Gate - If no clients are connected, throttle to 1fps keepalive
            let has_subscribers = frame_sender.receiver_count() > 0 || h264_sender.receiver_count() > 0;
            if !has_subscribers {
                last_had_subscribers = false;
                std::thread::sleep(Duration::from_millis(1000));
                continue;
            }

            if !last_had_subscribers {
                // Client newly connected: wake up immediately and request instant IDR keyframe
                last_had_subscribers = true;
                request_h264_keyframe.store(true, Ordering::SeqCst);
                info!("[Phase 3 Idle-Gate] Client connected -> Waking capture engine & requesting immediate IDR keyframe");
            }

            if settings.is_paused() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }

            let target_fps = settings.get_fps().max(1);
            let frame_interval = Duration::from_micros(1_000_000 / target_fps as u64);
            let frame_start = Instant::now();

            // 1. Separate Hardware Cursor Layer: Broadcast position over metadata channel
            unsafe {
                let src_w = GetSystemMetrics(SM_CXSCREEN);
                let src_h = GetSystemMetrics(SM_CYSCREEN);
                if src_w > 0 && src_h > 0 {
                    let mut ci: CURSORINFO = std::mem::zeroed();
                    ci.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
                    if GetCursorInfo(&mut ci) != 0 {
                        let is_showing = (ci.flags & CURSOR_SHOWING) != 0;
                        if is_showing {
                            let norm_x = (ci.ptScreenPos.x as f32) / (src_w as f32);
                            let norm_y = (ci.ptScreenPos.y as f32) / (src_h as f32);
                            if (norm_x - last_cursor_x).abs() > 0.0005 || (norm_y - last_cursor_y).abs() > 0.0005 {
                                last_cursor_x = norm_x;
                                last_cursor_y = norm_y;
                                let msg = format!("{{\"type\":\"cursor\",\"x\":{:.4},\"y\":{:.4},\"visible\":true}}", norm_x, norm_y);
                                let _ = cursor_sender.send(msg);
                            }
                        }
                    }
                }
            }

            // Check desktop lock status periodically (every 500ms)
            if last_lock_check.elapsed() >= Duration::from_millis(500) {
                last_lock_check = Instant::now();
                let is_locked = Self::check_is_desktop_locked();
                if is_locked != last_lock_state {
                    last_lock_state = is_locked;
                    let lock_msg = format!("{{\"type\":\"lock_status\",\"locked\":{}}}", is_locked);
                    let _ = cursor_sender.send(lock_msg);
                    if is_locked {
                        info!("Host desktop is locked (UIPI active). Remote mouse/keyboard inputs are blocked by Windows OS security.");
                    } else {
                        info!("Host desktop unlocked. Remote mouse/keyboard inputs enabled.");
                    }
                }
            }

            // Retry DXGI initialization periodically if it was dropped during lock screen
            if dxgi_cap.is_none() && last_dxgi_retry.elapsed() >= Duration::from_secs(5) {
                last_dxgi_retry = Instant::now();
                unsafe {
                    let input_desk = OpenInputDesktop(0, 0, 0x10000000);
                    if !input_desk.is_null() {
                        SetThreadDesktop(input_desk);
                    }
                }
                if let Ok(c) = DxgiDuplicationCapture::new() {
                    info!("DXGI GPU Desktop Duplication successfully restored!");
                    dxgi_cap = Some(c);
                }
            }

            // 2. Capture frame: Try Direct DXGI Duplication first, fallback to GDI DIBSection
            if let Some(ref mut cap) = dxgi_cap {
                match cap.acquire_frame(16) {
                    Ok(None) => {
                        // Static screen: 0 changes
                        let is_heartbeat = last_send_time.elapsed() >= Duration::from_millis(500);
                        if !is_heartbeat {
                            let elapsed = frame_start.elapsed();
                            if elapsed < frame_interval {
                                std::thread::sleep(frame_interval - elapsed);
                            }
                            continue;
                        }
                    }
                    Ok(Some(frame)) => {
                        let is_heartbeat = last_send_time.elapsed() >= Duration::from_millis(500);
                        if !frame.has_changed && !is_heartbeat {
                            // Màn tĩnh: 0 encode, 0 network overhead
                            let elapsed = frame_start.elapsed();
                            if elapsed < frame_interval {
                                std::thread::sleep(frame_interval - elapsed);
                            }
                            continue;
                        }

                        // Changed (caret/toast/motion) or heartbeat: Send frame!
                        last_send_time = Instant::now();
                        let need_jpeg = frame_sender.receiver_count() > 0;
                        let mut jpeg_bgra = None;
                        if need_jpeg {
                            let mut bgra = match recycle_rx.try_recv() {
                                Ok(b) => b,
                                Err(_) => Vec::new(),
                            };
                            if cap.read_cpu_pixels(&mut bgra).is_ok() {
                                jpeg_bgra = Some(bgra);
                            }
                        }

                        frame_seq += 1;
                        let raw_frame = RawFrame {
                            payload: FramePayload::GpuD3D11 {
                                texture: frame.texture,
                                d3d_manager: cap.d3d_manager(),
                            },
                            width: frame.width,
                            height: frame.height,
                            jpeg_bgra,
                            _frame_id: frame_seq,
                        };

                        let _ = tx.try_send(raw_frame);

                        let elapsed = frame_start.elapsed();
                        if elapsed < frame_interval {
                            std::thread::sleep(frame_interval - elapsed);
                        }
                        continue;
                    }
                    Err(e) => {
                        info!("DXGI capture encountered {:?}, falling back to GDI", e);
                        dxgi_cap = None;
                        last_dxgi_retry = Instant::now();
                    }
                }
            }

            // GDI Fallback capture (ONLY when DXGI is completely uninitialized or failed)
            let (target_w, target_h, raw_slice_opt) = {
                unsafe {
                    let src_w = GetSystemMetrics(SM_CXSCREEN);
                    let src_h = GetSystemMetrics(SM_CYSCREEN);

                    if src_w <= 0 || src_h <= 0 {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }

                    let req_h = settings.get_target_height();
                    let (target_w, target_h) = if req_h >= src_h as u32 {
                        (src_w as u32 & !1, src_h as u32 & !1)
                    } else {
                        let h = (req_h & !1).max(2);
                        let w = (((src_w as u32 * h) / src_h as u32) & !1).max(2);
                        (w, h)
                    };

                    if target_w != current_target_w || target_h != current_target_h || hdc_mem.is_null() {
                        if !hdc_mem.is_null() {
                            SelectObject(hdc_mem, old_obj);
                            DeleteObject(hbm);
                            DeleteDC(hdc_mem);
                            ReleaseDC(null_mut(), hdc_screen);
                        }

                        hdc_screen = GetDC(null_mut());
                        hdc_mem = CreateCompatibleDC(hdc_screen);
                        SetStretchBltMode(hdc_mem, COLORONCOLOR);

                        let bmi = BITMAPINFO {
                            bmiHeader: BITMAPINFOHEADER {
                                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                                biWidth: target_w as i32,
                                biHeight: -(target_h as i32),
                                biPlanes: 1,
                                biBitCount: 32,
                                biCompression: BI_RGB,
                                biSizeImage: 0,
                                biXPelsPerMeter: 0,
                                biYPelsPerMeter: 0,
                                biClrUsed: 0,
                                biClrImportant: 0,
                            },
                            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
                        };

                        hbm = CreateDIBSection(
                            hdc_mem,
                            &bmi,
                            DIB_RGB_COLORS,
                            &mut ppv_bits as *mut _ as *mut _,
                            null_mut(),
                            0,
                        );
                        old_obj = SelectObject(hdc_mem, hbm);

                        current_target_w = target_w;
                        current_target_h = target_h;
                    }

                    let blit_ok = if target_w == src_w as u32 && target_h == src_h as u32 {
                        BitBlt(hdc_mem, 0, 0, target_w as i32, target_h as i32, hdc_screen, 0, 0, SRCCOPY) != 0
                    } else {
                        StretchBlt(hdc_mem, 0, 0, target_w as i32, target_h as i32, hdc_screen, 0, 0, src_w, src_h, SRCCOPY) != 0
                    };

                    if !blit_ok || ppv_bits.is_null() {
                        if !hdc_mem.is_null() {
                            SelectObject(hdc_mem, old_obj);
                            DeleteObject(hbm);
                            DeleteDC(hdc_mem);
                            ReleaseDC(null_mut(), hdc_screen);
                            hdc_mem = null_mut();
                            hdc_screen = null_mut();
                            hbm = null_mut();
                            ppv_bits = null_mut();
                        }
                        current_target_w = 0;
                        current_target_h = 0;
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }

                    let byte_len = (target_w * target_h * 4) as usize;
                    let slice = std::slice::from_raw_parts(ppv_bits, byte_len);
                    (target_w, target_h, Some(slice))
                }
            };

            let raw_slice = match raw_slice_opt {
                Some(s) => s,
                None => {
                    let elapsed = frame_start.elapsed();
                    if elapsed < frame_interval {
                        std::thread::sleep(frame_interval - elapsed);
                    }
                    continue;
                }
            };

            // 3. Dense 5-Point / Block Change Detection: Checks desktop pixel changes (for GDI fallback)
            let current_hash = Self::calculate_grid_hash(raw_slice, target_w as usize, target_h as usize);
            let is_changed = current_hash != last_hash;
            let is_heartbeat = last_send_time.elapsed() >= Duration::from_millis(500);

            if !is_changed && !is_heartbeat {
                // Static screen: zero encode, zero network overhead
                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    std::thread::sleep(frame_interval - elapsed);
                }
                continue;
            }

            last_hash = current_hash;
            last_send_time = Instant::now();

            let byte_len = (target_w * target_h * 4) as usize;
            let mut buf = match recycle_rx.try_recv() {
                Ok(b) => b,
                Err(_) => Vec::with_capacity(byte_len),
            };
            buf.resize(byte_len, 0);
            buf.copy_from_slice(raw_slice);

            frame_seq += 1;
            let raw_frame = RawFrame {
                payload: FramePayload::CpuBgra { pixels: buf },
                width: target_w,
                height: target_h,
                jpeg_bgra: None,
                _frame_id: frame_seq,
            };

            let _ = tx.try_send(raw_frame);

            let elapsed = frame_start.elapsed();
            if elapsed < frame_interval {
                std::thread::sleep(frame_interval - elapsed);
            }
        }

        unsafe {
            if !hdc_mem.is_null() {
                SelectObject(hdc_mem, old_obj);
                DeleteObject(hbm);
                DeleteDC(hdc_mem);
                ReleaseDC(null_mut(), hdc_screen);
            }
        }
    }

    /// Dedicated compression worker running H.264 & JPEG dual pipeline
    fn run_encoder_worker(
        running: Arc<AtomicBool>,
        settings: Arc<DynamicStreamSettings>,
        rx: &mut tokio::sync::mpsc::Receiver<RawFrame>,
        recycle_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
        frame_sender: broadcast::Sender<Bytes>,
        h264_sender: broadcast::Sender<Bytes>,
        current_fps: Arc<AtomicU32>,
        latest_frame: Arc<RwLock<Option<Bytes>>>,
        latest_h264_keyframe: Arc<RwLock<Option<Bytes>>>,
        request_h264_keyframe: Arc<AtomicBool>,
    ) {
        let mut jpeg_buf = Vec::with_capacity(1280 * 720 / 2);
        let mut h264_encoder = H264EncoderWrapper::new();
        let mut fps_counter = 0u32;
        let mut last_fps_check = Instant::now();
        let mut last_forced_idr = Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now);

        while running.load(Ordering::Relaxed) {
            if let Some(frame) = rx.blocking_recv() {
                let now_ms = crate::clock::qpc_now_ms();

                let has_h264_subscribers = h264_sender.receiver_count() > 0;
                let need_jpeg = frame_sender.receiver_count() > 0
                    || latest_frame.read().map(|g| g.is_none()).unwrap_or(false);

                // If no subscribers at all, skip both encoders to save 100% CPU
                if !has_h264_subscribers && !need_jpeg {
                    if let Some(buf) = frame.jpeg_bgra {
                        let _ = recycle_tx.try_send(buf);
                    }
                    continue;
                }

                // 1. Force keyframe if requested by subscriber or frame_loss, with 200ms cooldown (P0.3)
                if request_h264_keyframe.load(Ordering::Relaxed) {
                    if last_forced_idr.elapsed() >= Duration::from_millis(200) {
                        request_h264_keyframe.store(false, Ordering::Relaxed);
                        last_forced_idr = Instant::now();
                        h264_encoder.force_intra_frame();
                        tracing::debug!("Forced H.264 IDR keyframe generated (cooldown 200ms)");
                    }
                }

                // 2. Encode H.264 if there are subscribers
                if has_h264_subscribers {
                    let encode_res = match frame.payload {
                        FramePayload::GpuD3D11 { ref texture, ref d3d_manager } => {
                            h264_encoder.set_d3d_manager(d3d_manager.0.clone());
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                h264_encoder.encode_texture(&texture.0, frame.width as usize, frame.height as usize)
                            }))
                        }
                        FramePayload::CpuBgra { ref pixels } => {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                h264_encoder.encode(pixels, frame.width as usize, frame.height as usize)
                            }))
                        }
                    };

                    match encode_res {
                        Ok(Some(h264_data)) => {
                            let is_key = Self::is_h264_keyframe(&h264_data);
                            let mut packet = BytesMut::with_capacity(12 + h264_data.len());
                            packet.put_u64(now_ms);
                            packet.put_u16(frame.width as u16);
                            packet.put_u16(frame.height as u16);
                            packet.extend_from_slice(&h264_data);
                            let bytes = packet.freeze();

                            if is_key {
                                if let Ok(mut guard) = latest_h264_keyframe.write() {
                                    *guard = Some(bytes.clone());
                                }
                            }
                            let _ = h264_sender.send(bytes);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            info!("H.264 encode panicked safely caught: {:?}", e);
                        }
                    }
                }

                // 3. Encode JPEG (standard fallback) - ONLY if needed
                if need_jpeg {
                    let bgra_slice: Option<&[u8]> = match frame.payload {
                        FramePayload::GpuD3D11 { .. } => frame.jpeg_bgra.as_deref(),
                        FramePayload::CpuBgra { ref pixels } => Some(pixels.as_slice()),
                    };

                    if let Some(bgra) = bgra_slice {
                        jpeg_buf.clear();
                        let quality = settings.get_quality();
                        let encoder = Encoder::new(&mut jpeg_buf, quality);

                        if let Ok(_) = encoder.encode(
                            bgra,
                            frame.width as u16,
                            frame.height as u16,
                            ColorType::Bgra,
                        ) {
                            let mut packet = BytesMut::with_capacity(12 + jpeg_buf.len());
                            packet.put_u64(now_ms);
                            packet.put_u16(frame.width as u16);
                            packet.put_u16(frame.height as u16);
                            packet.extend_from_slice(&jpeg_buf);

                            let bytes = packet.freeze();
                            if let Ok(mut guard) = latest_frame.write() {
                                *guard = Some(bytes.clone());
                            }
                            let _ = frame_sender.send(bytes);
                        }
                    }
                }

                // Recycle buffer back into zero-allocation pool
                if let Some(buf) = frame.jpeg_bgra {
                    let _ = recycle_tx.try_send(buf);
                } else if let FramePayload::CpuBgra { pixels } = frame.payload {
                    let _ = recycle_tx.try_send(pixels);
                }

                fps_counter += 1;
                if last_fps_check.elapsed() >= Duration::from_secs(1) {
                    current_fps.store(fps_counter, Ordering::Relaxed);
                    fps_counter = 0;
                    last_fps_check = Instant::now();
                }
            }
        }
    }

    /// Dense 5-point per block FNV-1a hash covering all sectors of the screen
    fn calculate_grid_hash(data: &[u8], width: usize, height: usize) -> u64 {
        if data.len() < width * height * 4 || width < 16 || height < 9 {
            return 0;
        }
        let cols = 16;
        let rows = 9;
        let block_w = width / cols;
        let block_h = height / rows;
        let mut hash: u64 = 0xcbf29ce484222325; // FNV offset

        let offsets = [
            (block_w / 2, block_h / 2),
            (block_w / 4, block_h / 4),
            (3 * block_w / 4, block_h / 4),
            (block_w / 4, 3 * block_h / 4),
            (3 * block_w / 4, 3 * block_h / 4),
        ];

        for r in 0..rows {
            let base_y = r * block_h;
            for c in 0..cols {
                let base_x = c * block_w;
                for &(ox, oy) in &offsets {
                    let cx = base_x + ox;
                    let cy = base_y + oy;
                    let idx = (cy * width + cx) * 4;
                    if idx + 3 < data.len() {
                        let pixel = u32::from_le_bytes([
                            data[idx],
                            data[idx + 1],
                            data[idx + 2],
                            data[idx + 3],
                        ]) as u64;
                        hash = (hash ^ pixel).wrapping_mul(0x100000001b3);
                    }
                }
            }
        }
        hash
    }

    /// Inspects H.264 Annex B stream to verify if it contains a true IDR (type 5) keyframe (P0.1)
    pub fn is_h264_keyframe(data: &[u8]) -> bool {
        let len = data.len();
        let mut i = 0;
        while i + 3 < len {
            if data[i] == 0 && data[i + 1] == 0 {
                let nal_header_idx = if data[i + 2] == 1 {
                    i + 3
                } else if i + 4 < len && data[i + 2] == 0 && data[i + 3] == 1 {
                    i + 4
                } else {
                    i += 1;
                    continue;
                };

                if nal_header_idx < len {
                    let nal_type = data[nal_header_idx] & 0x1F;
                    // 5: IDR slice, 7: SPS parameter set (Instantaneous Decoder Refresh access unit)
                    if nal_type == 5 || nal_type == 7 {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    /// Checks whether the interactive Windows desktop is currently locked (Winlogon / LogonUI active)
    pub fn check_is_desktop_locked() -> bool {
        unsafe {
            // DESKTOP_READOBJECTS = 0x0001
            let desk = OpenInputDesktop(0, 0, 0x0001);
            if desk.is_null() {
                return true;
            }
            CloseDesktop(desk);
            false
        }
    }
}
