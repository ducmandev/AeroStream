use std::ptr::null_mut;
use std::time::{Duration, Instant};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn main() {
    unsafe {
        let input_desk = OpenInputDesktop(0, 0, 0x10000000);
        if !input_desk.is_null() {
            SetThreadDesktop(input_desk);
        }

        let hdc_screen = GetDC(null_mut());
        let src_w = GetSystemMetrics(SM_CXSCREEN);
        let src_h = GetSystemMetrics(SM_CYSCREEN);

        for (target_w, target_h, quality) in [
            (src_w, src_h, 70), // Native 1080p
            (1280, 720, 70),    // 720p
            (960, 540, 60),     // 540p
        ] {
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            SetStretchBltMode(hdc_mem, COLORONCOLOR);

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: target_w,
                    biHeight: -target_h, // top-down
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
            let mut jpeg = Vec::with_capacity((target_w * target_h) as usize / 2);

            // Warmup
            if target_w == src_w && target_h == src_h {
                BitBlt(hdc_mem, 0, 0, target_w, target_h, hdc_screen, 0, 0, SRCCOPY);
            } else {
                StretchBlt(hdc_mem, 0, 0, target_w, target_h, hdc_screen, 0, 0, src_w, src_h, SRCCOPY);
            }

            let runs = 20;
            let mut total_cap = Duration::ZERO;
            let mut total_enc = Duration::ZERO;
            let start = Instant::now();

            for _ in 0..runs {
                let cap_start = Instant::now();
                if target_w == src_w && target_h == src_h {
                    BitBlt(hdc_mem, 0, 0, target_w, target_h, hdc_screen, 0, 0, SRCCOPY);
                } else {
                    StretchBlt(hdc_mem, 0, 0, target_w, target_h, hdc_screen, 0, 0, src_w, src_h, SRCCOPY);
                }
                // Memory is ALREADY in ppv_bits!
                let raw_slice = std::slice::from_raw_parts(ppv_bits, buf_size);
                total_cap += cap_start.elapsed();

                let enc_start = Instant::now();
                jpeg.clear();
                let enc = jpeg_encoder::Encoder::new(&mut jpeg, quality);
                enc.encode(raw_slice, target_w as u16, target_h as u16, jpeg_encoder::ColorType::Bgra).unwrap();
                total_enc += enc_start.elapsed();
            }

            let avg_total = start.elapsed() / runs;
            let avg_cap = total_cap / runs;
            let avg_enc = total_enc / runs;
            let sync_fps = 1_000_000.0 / avg_total.as_micros() as f64;
            let pipelined_fps = 1_000_000.0 / avg_cap.max(avg_enc).as_micros() as f64;

            println!(
                "DIBSection {}x{} Q{}: Cap = {:?}, Enc = {:?}, Total = {:?} -> Sync FPS = {:.1}, Pipelined FPS = {:.1}, Size = {} KB",
                target_w, target_h, quality, avg_cap, avg_enc, avg_total, sync_fps, pipelined_fps, jpeg.len() / 1024
            );

            SelectObject(hdc_mem, old_obj);
            DeleteObject(hbm);
            DeleteDC(hdc_mem);
        }

        ReleaseDC(null_mut(), hdc_screen);
    }
}
