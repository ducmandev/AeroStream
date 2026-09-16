use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

struct RawFrame {
    bgra: Vec<u8>,
    width: u32,
    height: u32,
    frame_id: u64,
}

#[tokio::main]
async fn main() {
    println!("=== Testing Full 60 FPS Pipeline Simulation ===");

    let running = Arc::new(AtomicBool::new(true));
    let (tx_raw, mut rx_raw) = tokio::sync::mpsc::channel::<RawFrame>(2);
    let (tx_stream, mut rx_stream) = broadcast::channel::<bytes::Bytes>(32);

    let fps_captured = Arc::new(AtomicU32::new(0));
    let fps_encoded = Arc::new(AtomicU32::new(0));
    let fps_received = Arc::new(AtomicU32::new(0));

    // Target 720p (1280x720)
    let target_w = 1280;
    let target_h = 720;

    // Spawn 2 Interleaved Capture Workers
    for worker_id in 0..2 {
        let r = running.clone();
        let tx = tx_raw.clone();
        let cap_counter = fps_captured.clone();

        std::thread::spawn(move || {
            unsafe {
                let input_desk = OpenInputDesktop(0, 0, 0x10000000);
                if !input_desk.is_null() {
                    SetThreadDesktop(input_desk);
                }

                let hdc_screen = GetDC(null_mut());
                let src_w = GetSystemMetrics(SM_CXSCREEN);
                let src_h = GetSystemMetrics(SM_CYSCREEN);

                let hdc_mem = CreateCompatibleDC(hdc_screen);
                SetStretchBltMode(hdc_mem, COLORONCOLOR);

                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: target_w,
                        biHeight: -target_h,
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

                let mut ppv: *mut u8 = null_mut();
                let hbm = CreateDIBSection(hdc_mem, &bmi, DIB_RGB_COLORS, &mut ppv as *mut _ as *mut _, null_mut(), 0);
                let old = SelectObject(hdc_mem, hbm);

                // Offset worker 1 by 16ms for 60 FPS interleaving
                if worker_id == 1 {
                    std::thread::sleep(Duration::from_millis(16));
                }

                let buf_size = (target_w * target_h * 4) as usize;
                let mut frame_id = worker_id as u64;

                while r.load(Ordering::Relaxed) {
                    let t0 = Instant::now();
                    StretchBlt(hdc_mem, 0, 0, target_w, target_h, hdc_screen, 0, 0, src_w, src_h, SRCCOPY);
                    let raw_slice = std::slice::from_raw_parts(ppv, buf_size);

                    frame_id += 2;
                    let frame = RawFrame {
                        bgra: raw_slice.to_vec(),
                        width: target_w as u32,
                        height: target_h as u32,
                        frame_id,
                    };

                    // Try sending, if channel is full drop to maintain ultra-low latency
                    let _ = tx.try_send(frame);
                    cap_counter.fetch_add(1, Ordering::Relaxed);

                    let el = t0.elapsed();
                    // Sleep ~32ms per thread -> 2 threads = ~16ms interval = 60 FPS
                    if el < Duration::from_millis(32) {
                        std::thread::sleep(Duration::from_millis(32) - el);
                    }
                }

                SelectObject(hdc_mem, old);
                DeleteObject(hbm);
                DeleteDC(hdc_mem);
                ReleaseDC(null_mut(), hdc_screen);
            }
        });
    }

    // Compression Worker
    let r_comp = running.clone();
    let enc_counter = fps_encoded.clone();
    let tx_bcast = tx_stream.clone();

    std::thread::spawn(move || {
        let mut jpeg_buf = Vec::with_capacity(1280 * 720 / 2);

        while r_comp.load(Ordering::Relaxed) {
            // Receive latest frame from capture workers
            if let Some(frame) = rx_raw.blocking_recv() {
                jpeg_buf.clear();
                let enc = jpeg_encoder::Encoder::new(&mut jpeg_buf, 68);
                if let Ok(_) = enc.encode(&frame.bgra, frame.width as u16, frame.height as u16, jpeg_encoder::ColorType::Bgra) {
                    let bytes = bytes::Bytes::copy_from_slice(&jpeg_buf);
                    let _ = tx_bcast.send(bytes);
                    enc_counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });

    // Mock Client Consumer (Simulates WebSocket receiver task)
    let r_client = running.clone();
    let recv_counter = fps_received.clone();
    tokio::spawn(async move {
        while r_client.load(Ordering::Relaxed) {
            if let Ok(_frame) = rx_stream.recv().await {
                recv_counter.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Monitor for 5 seconds
    for i in 1..=5 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let c = fps_captured.swap(0, Ordering::Relaxed);
        let e = fps_encoded.swap(0, Ordering::Relaxed);
        let r = fps_received.swap(0, Ordering::Relaxed);
        println!("Sec {}: Captured = {} FPS | Encoded = {} FPS | Received by Client = {} FPS", i, c, e, r);
    }

    running.store(false, Ordering::Relaxed);
}
