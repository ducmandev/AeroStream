use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

struct SharedFrame {
    bgra: Vec<u8>,
    width: u32,
    height: u32,
    frame_id: u64,
}

fn main() {
    println!("=== Testing Pipelined Capture & Compression (Target: 720p) ===");

    let running = Arc::new(AtomicBool::new(true));
    let frame_slot = Arc::new(Mutex::new(None::<SharedFrame>));
    let frames_captured = Arc::new(AtomicU32::new(0));
    let frames_compressed = Arc::new(AtomicU32::new(0));

    let running_cap = running.clone();
    let frame_slot_cap = frame_slot.clone();
    let cap_counter = frames_captured.clone();

    // 1. Dedicated Capture Thread
    let cap_handle = std::thread::spawn(move || {
        unsafe {
            let input_desk = OpenInputDesktop(0, 0, 0x10000000);
            if !input_desk.is_null() {
                SetThreadDesktop(input_desk);
            }

            let hdc_screen = GetDC(null_mut());
            let src_w = GetSystemMetrics(SM_CXSCREEN);
            let src_h = GetSystemMetrics(SM_CYSCREEN);

            // 720p target
            let target_w = 1280;
            let target_h = 720;

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

            let mut ppv_bits: *mut u8 = null_mut();
            let hbm = CreateDIBSection(
                hdc_mem,
                &bmi,
                DIB_RGB_COLORS,
                &mut ppv_bits as *mut _ as *mut _,
                null_mut(),
                0,
            );
            let old_obj = SelectObject(hdc_mem, hbm);

            let buf_size = (target_w * target_h * 4) as usize;
            let mut id = 0u64;

            while running_cap.load(Ordering::Relaxed) {
                let start = Instant::now();
                StretchBlt(hdc_mem, 0, 0, target_w, target_h, hdc_screen, 0, 0, src_w, src_h, SRCCOPY);
                let raw_slice = std::slice::from_raw_parts(ppv_bits, buf_size);

                id += 1;
                {
                    let mut guard = frame_slot_cap.lock().unwrap();
                    if let Some(ref mut sf) = *guard {
                        sf.bgra.copy_from_slice(raw_slice);
                        sf.frame_id = id;
                    } else {
                        *guard = Some(SharedFrame {
                            bgra: raw_slice.to_vec(),
                            width: target_w as u32,
                            height: target_h as u32,
                            frame_id: id,
                        });
                    }
                }
                cap_counter.fetch_add(1, Ordering::Relaxed);

                // Target 60 FPS capture rate (16.6ms)
                let elapsed = start.elapsed();
                if elapsed < Duration::from_millis(16) {
                    std::thread::sleep(Duration::from_millis(16) - elapsed);
                }
            }

            SelectObject(hdc_mem, old_obj);
            DeleteObject(hbm);
            DeleteDC(hdc_mem);
            ReleaseDC(null_mut(), hdc_screen);
        }
    });

    // 2. Dedicated Compression Thread
    let running_comp = running.clone();
    let frame_slot_comp = frame_slot.clone();
    let comp_counter = frames_compressed.clone();

    let comp_handle = std::thread::spawn(move || {
        let mut last_processed_id = 0u64;
        let mut local_bgra = Vec::with_capacity(1280 * 720 * 4);
        let mut jpeg_buf = Vec::with_capacity(1280 * 720 / 2);

        while running_comp.load(Ordering::Relaxed) {
            let mut work: Option<(u64, u32, u32)> = None;

            {
                if let Ok(guard) = frame_slot_comp.lock() {
                    if let Some(ref sf) = *guard {
                        if sf.frame_id != last_processed_id {
                            last_processed_id = sf.frame_id;
                            if local_bgra.len() != sf.bgra.len() {
                                local_bgra.resize(sf.bgra.len(), 0);
                            }
                            local_bgra.copy_from_slice(&sf.bgra);
                            work = Some((sf.frame_id, sf.width, sf.height));
                        }
                    }
                }
            }

            if let Some((_, w, h)) = work {
                jpeg_buf.clear();
                let enc = jpeg_encoder::Encoder::new(&mut jpeg_buf, 68);
                if let Ok(_) = enc.encode(&local_bgra, w as u16, h as u16, jpeg_encoder::ColorType::Bgra) {
                    comp_counter.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    });

    // Monitor for 5 seconds
    let start_time = Instant::now();
    for i in 1..=5 {
        std::thread::sleep(Duration::from_secs(1));
        let caps = frames_captured.swap(0, Ordering::Relaxed);
        let comps = frames_compressed.swap(0, Ordering::Relaxed);
        println!("Sec {}: Captured = {} FPS, Compressed = {} FPS", i, caps, comps);
    }

    running.store(false, Ordering::Relaxed);
    let _ = cap_handle.join();
    let _ = comp_handle.join();

    println!("Total test completed in {:?}", start_time.elapsed());
}
