use dxgi_capture_rs::DXGIManager;
use openh264::encoder::Encoder;
use openh264::formats::{RgbSliceU8, YUVBuffer};
use std::time::Instant;

fn main() {
    println!("=== Testing DXGI + H.264 End-to-End Pipeline ===");
    unsafe {
        use windows_sys::Win32::System::StationsAndDesktops::*;
        let input_desk = OpenInputDesktop(0, 0, 0x10000000);
        if !input_desk.is_null() {
            let _ = SetThreadDesktop(input_desk);
        }
    }

    let mut manager = match DXGIManager::new(100) {
        Ok(m) => m,
        Err(e) => {
            println!("DXGI init failed: {:?}", e);
            return;
        }
    };

    let mut encoder = Encoder::new().expect("Failed to create OpenH264 encoder");

    let runs = 30;
    println!("Running {} frames through DXGI GPU capture -> H.264 encode...", runs);
    let mut total_bytes = 0usize;
    let mut rgb_buf = Vec::new();
    let start = Instant::now();

    for i in 0..runs {
        let t_frame = Instant::now();
        match manager.capture_frame() {
            Ok((pixels, (w, h))) => {
                let t_cap = t_frame.elapsed();
                let target_w = w;
                let target_h = h;
                let required_rgb = target_w * target_h * 3;
                if rgb_buf.len() != required_rgb {
                    rgb_buf.resize(required_rgb, 0);
                }

                // Fast BGRA -> RGB
                let raw_u8: &[u8] = unsafe {
                    std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixels.len() * 4)
                };

                let mut rgb_idx = 0;
                for chunk in raw_u8.chunks_exact(4) {
                    rgb_buf[rgb_idx] = chunk[2];
                    rgb_buf[rgb_idx + 1] = chunk[1];
                    rgb_buf[rgb_idx + 2] = chunk[0];
                    rgb_idx += 3;
                }

                let t_conv = t_frame.elapsed();
                let rgb_source = RgbSliceU8::new(&rgb_buf, (target_w, target_h));
                let yuv = YUVBuffer::from_rgb_source(rgb_source);
                let bitstream = encoder.encode(&yuv).expect("Failed to encode");
                let nalu_len = bitstream.to_vec().len();
                total_bytes += nalu_len;

                let t_total = t_frame.elapsed();
                if i < 5 || i == runs - 1 {
                    println!(
                        "Frame {}: H.264 Size = {} bytes | DXGI Cap: {:?} | Conv: {:?} | Total Frame Time: {:?}",
                        i, nalu_len, t_cap, t_conv - t_cap, t_total
                    );
                }
            }
            Err(e) => {
                println!("Frame {} capture error: {:?}", i, e);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    let elapsed = start.elapsed();
    let total_kb = total_bytes / 1024;
    let avg_mbps = (total_bytes as f64 * 8.0) / (elapsed.as_secs_f64() * 1_000_000.0);
    println!("--------------------------------------------------");
    println!("Total time: {:?}", elapsed);
    println!("Total Data Transmitted: {} KB for {} frames", total_kb, runs);
    println!("Average Bitrate: {:.2} Mbps", avg_mbps);
    println!("--------------------------------------------------");
}
