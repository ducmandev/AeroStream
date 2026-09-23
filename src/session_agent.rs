use bytes::{BufMut, Bytes, BytesMut};
use jpeg_encoder::{ColorType, Encoder};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;
use tracing::{error, info, warn};

use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::input::{InputManager, InputMessage};

const PACKET_TYPE_FRAME: u8 = 0x01;
#[allow(dead_code)]
const PACKET_TYPE_AUDIO: u8 = 0x02;
const PACKET_TYPE_INPUT: u8 = 0x10;
#[allow(dead_code)]
const PACKET_TYPE_HEARTBEAT: u8 = 0x20;

pub async fn run_session_agent(pipe_name: String) -> Result<(), Box<dyn std::error::Error>> {
    info!("=== AeroStream Session Agent Starting ===");
    let pipe_path = format!(r"\\.\pipe\{}", pipe_name);
    info!("[SessionAgent] Connecting to Broker Named Pipe: {}", pipe_path);

    // 1. Retry connecting to Broker Named Pipe
    let mut client = None;
    for attempt in 1..=30 {
        match ClientOptions::new().open(&pipe_path) {
            Ok(c) => {
                client = Some(c);
                info!("[SessionAgent] Connected to Broker on attempt {}", attempt);
                break;
            }
            Err(e) => {
                if attempt % 5 == 0 {
                    info!("[SessionAgent] Waiting for Named Pipe server... ({}/30): {:?}", attempt, e);
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    let client = match client {
        Some(c) => c,
        None => {
            error!("[SessionAgent] Failed to connect to Named Pipe '{}' after 30 attempts. Exiting.", pipe_path);
            return Err("Pipe connection timeout".into());
        }
    };

    let (mut reader, mut writer) = tokio::io::split(client);

    // 2. Query Session Virtual Display Metrics
    let (screen_w, screen_h) = unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN) as u32;
        let h = GetSystemMetrics(SM_CYSCREEN) as u32;
        (if w > 0 { w } else { 1280 }, if h > 0 { h } else { 720 })
    };
    info!("[SessionAgent] Session Desktop Resolution: {}x{}", screen_w, screen_h);

    // 3. Initialize InputManager inside Session Desktop
    let input_manager = match InputManager::new(screen_w, screen_h) {
        Ok(im) => Arc::new(im),
        Err(e) => {
            error!("[SessionAgent] Failed to init InputManager: {:?}", e);
            return Err(e.into());
        }
    };

    let running = Arc::new(AtomicBool::new(true));

    // 4. Background Task: Read Input Events from Broker
    let input_running = Arc::clone(&running);
    let input_im = Arc::clone(&input_manager);
    let input_task = tokio::spawn(async move {
        let mut len_buf = [0u8; 4];
        while input_running.load(Ordering::Relaxed) {
            if let Err(_) = reader.read_exact(&mut len_buf).await {
                warn!("[SessionAgent] Pipe closed by Broker (read length failed). Terminating.");
                input_running.store(false, Ordering::Relaxed);
                break;
            }
            let packet_len = u32::from_le_bytes(len_buf) as usize;
            if packet_len == 0 || packet_len > 1_000_000 {
                warn!("[SessionAgent] Invalid packet length {}; closing.", packet_len);
                input_running.store(false, Ordering::Relaxed);
                break;
            }

            let mut packet_data = vec![0u8; packet_len];
            if let Err(_) = reader.read_exact(&mut packet_data).await {
                warn!("[SessionAgent] Pipe closed by Broker (read data failed).");
                input_running.store(false, Ordering::Relaxed);
                break;
            }

            let packet_type = packet_data[0];
            if packet_type == PACKET_TYPE_INPUT && packet_data.len() > 1 {
                if let Ok(json_str) = std::str::from_utf8(&packet_data[1..]) {
                    if let Ok(msg) = serde_json::from_str::<InputMessage>(json_str) {
                        input_im.handle_input(msg);
                    }
                }
            }
        }
    });

    // 5. GDI Virtual Display Capture Loop (Session Mode)
    let capture_running = Arc::clone(&running);
    let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel::<Bytes>(2);

    let capture_thread = std::thread::spawn(move || {
        let hdc_screen = unsafe { GetDC(null_mut()) };
        let hdc_mem = unsafe { CreateCompatibleDC(hdc_screen) };

        let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = screen_w as i32;
        bmi.bmiHeader.biHeight = -(screen_h as i32); // Top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut p_bits: *mut u8 = null_mut();
        let hbitmap = unsafe {
            CreateDIBSection(
                hdc_mem,
                &bmi,
                DIB_RGB_COLORS,
                &mut p_bits as *mut _ as *mut _,
                null_mut(),
                0,
            )
        };
        let old_bmp = unsafe { SelectObject(hdc_mem, hbitmap) };

        let buf_size = (screen_w * screen_h * 4) as usize;
        let mut jpeg_buf = Vec::with_capacity(buf_size / 8);
        let mut frame_id = 0u64;

        let target_interval = Duration::from_millis(33); // ~30 FPS default for isolated RDP session

        while capture_running.load(Ordering::Relaxed) {
            let start_t = Instant::now();

            unsafe {
                BitBlt(
                    hdc_mem,
                    0,
                    0,
                    screen_w as i32,
                    screen_h as i32,
                    hdc_screen,
                    0,
                    0,
                    SRCCOPY | CAPTUREBLT,
                );
            }

            if !p_bits.is_null() {
                let raw_slice = unsafe { std::slice::from_raw_parts(p_bits, buf_size) };
                jpeg_buf.clear();

                let encoder = Encoder::new(&mut jpeg_buf, 70);
                if encoder.encode(raw_slice, screen_w as u16, screen_h as u16, ColorType::Bgra).is_ok() {
                    let ts_ms = crate::clock::qpc_now_ms() as u32;
                    let mut packet = BytesMut::with_capacity(12 + jpeg_buf.len());
                    packet.put_u32_le(ts_ms);
                    packet.put_u16_le(screen_w as u16);
                    packet.put_u16_le(screen_h as u16);
                    packet.put_u32_le(frame_id as u32);
                    packet.put_slice(&jpeg_buf);

                    let _ = frame_tx.blocking_send(packet.freeze());
                    frame_id += 1;
                }
            }

            let elapsed = start_t.elapsed();
            if elapsed < target_interval {
                std::thread::sleep(target_interval - elapsed);
            }
        }

        unsafe {
            SelectObject(hdc_mem, old_bmp);
            DeleteObject(hbitmap);
            DeleteDC(hdc_mem);
            ReleaseDC(null_mut(), hdc_screen);
        }
    });

    // 6. Main Agent Event Loop: Write Encoded Frames to Named Pipe
    while running.load(Ordering::Relaxed) {
        tokio::select! {
            Some(frame) = frame_rx.recv() => {
                let payload_len = (1 + frame.len()) as u32;
                if let Err(_) = writer.write_all(&payload_len.to_le_bytes()).await {
                    break;
                }
                if let Err(_) = writer.write_all(&[PACKET_TYPE_FRAME]).await {
                    break;
                }
                if let Err(_) = writer.write_all(&frame).await {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if !running.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = input_task.abort();
    let _ = capture_thread.join();
    info!("=== AeroStream Session Agent Cleanly Terminated ===");
    Ok(())
}
