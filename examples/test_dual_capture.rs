use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn main() {
    println!("=== Testing Dual Interleaved GDI Capture ===");
    let running = Arc::new(AtomicBool::new(true));
    let frames = Arc::new(AtomicU32::new(0));

    let mut handles = vec![];
    for worker_id in 0..2 {
        let r = running.clone();
        let f = frames.clone();
        handles.push(std::thread::spawn(move || {
            unsafe {
                let input_desk = OpenInputDesktop(0, 0, 0x10000000);
                if !input_desk.is_null() {
                    SetThreadDesktop(input_desk);
                }

                let hdc_screen = GetDC(null_mut());
                let src_w = GetSystemMetrics(SM_CXSCREEN);
                let src_h = GetSystemMetrics(SM_CYSCREEN);

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

                let mut ppv: *mut u8 = null_mut();
                let hbm = CreateDIBSection(hdc_mem, &bmi, DIB_RGB_COLORS, &mut ppv as *mut _ as *mut _, null_mut(), 0);
                let old = SelectObject(hdc_mem, hbm);

                // Offset worker 1 by 16ms
                if worker_id == 1 {
                    std::thread::sleep(Duration::from_millis(16));
                }

                while r.load(Ordering::Relaxed) {
                    let t0 = Instant::now();
                    StretchBlt(hdc_mem, 0, 0, target_w, target_h, hdc_screen, 0, 0, src_w, src_h, SRCCOPY);
                    f.fetch_add(1, Ordering::Relaxed);
                    let el = t0.elapsed();
                    if el < Duration::from_millis(32) {
                        std::thread::sleep(Duration::from_millis(32) - el);
                    }
                }

                SelectObject(hdc_mem, old);
                DeleteObject(hbm);
                DeleteDC(hdc_mem);
                ReleaseDC(null_mut(), hdc_screen);
            }
        }));
    }

    for i in 1..=4 {
        std::thread::sleep(Duration::from_secs(1));
        let count = frames.swap(0, Ordering::Relaxed);
        println!("Sec {}: Total Interleaved Frames = {} FPS", i, count);
    }

    running.store(false, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
}
