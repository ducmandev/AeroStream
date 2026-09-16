use openh264::encoder::Encoder;
use openh264::formats::{RgbSliceU8, YUVBuffer};
use std::ptr::null_mut;
use std::time::Instant;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn main() {
    println!("=== Testing OpenH264 Real-time Screen Encoding ===");

    unsafe {
        let input_desk = OpenInputDesktop(0, 0, 0x10000000);
        if !input_desk.is_null() {
            SetThreadDesktop(input_desk);
        }

        let hdc_screen = GetDC(null_mut());
        let src_w = GetSystemMetrics(SM_CXSCREEN);
        let src_h = GetSystemMetrics(SM_CYSCREEN);

        let target_w = 1280usize;
        let target_h = 720usize;

        let hdc_mem = CreateCompatibleDC(hdc_screen);
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

        let mut ppv_bits: *mut u8 = null_mut();
        let hbm = CreateDIBSection(hdc_mem, &bmi, DIB_RGB_COLORS, &mut ppv_bits as *mut _ as *mut _, null_mut(), 0);
        let old = SelectObject(hdc_mem, hbm);

        let mut encoder = Encoder::new().expect("Failed to create OpenH264 encoder");

        let byte_len = target_w * target_h * 4;
        let mut rgb_buf = vec![0u8; target_w * target_h * 3];
        let runs = 30;

        println!("Encoding {} frames at {}x{}...", runs, target_w, target_h);
        let start = Instant::now();
        let mut total_bytes = 0usize;

        for i in 0..runs {
            let cap_start = Instant::now();
            StretchBlt(hdc_mem, 0, 0, target_w as i32, target_h as i32, hdc_screen, 0, 0, src_w, src_h, SRCCOPY);
            let raw_slice = std::slice::from_raw_parts(ppv_bits, byte_len);

            // Fast BGRA -> RGB
            let mut rgb_idx = 0;
            for chunk in raw_slice.chunks_exact(4) {
                rgb_buf[rgb_idx] = chunk[2];
                rgb_buf[rgb_idx + 1] = chunk[1];
                rgb_buf[rgb_idx + 2] = chunk[0];
                rgb_idx += 3;
            }

            let t_enc = Instant::now();
            let rgb_source = RgbSliceU8::new(&rgb_buf, (target_w, target_h));
            let yuv = YUVBuffer::from_rgb_source(rgb_source);
            let bitstream = encoder.encode(&yuv).expect("Failed to encode frame");
            let encoded_size = bitstream.to_vec().len();
            total_bytes += encoded_size;

            if i < 5 || i == runs - 1 {
                println!(
                    "Frame {}: H264 Size = {} bytes | Cap+Conv: {:?} | Enc: {:?}",
                    i, encoded_size, t_enc.duration_since(cap_start), t_enc.elapsed()
                );
            }
        }

        let elapsed = start.elapsed();
        let avg_frame_time = elapsed / runs as u32;
        let total_kb = total_bytes / 1024;
        let avg_bitrate_mbps = (total_bytes as f64 * 8.0) / (elapsed.as_secs_f64() * 1_000_000.0);

        println!("--------------------------------------------------");
        println!("Total time: {:?}", elapsed);
        println!("Avg Frame Time: {:?}", avg_frame_time);
        println!("Total Data: {} KB for {} frames", total_kb, runs);
        println!("Effective Bitrate: {:.2} Mbps", avg_bitrate_mbps);
        println!("--------------------------------------------------");

        SelectObject(hdc_mem, old);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(null_mut(), hdc_screen);
    }
}
