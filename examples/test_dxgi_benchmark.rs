use dxgi_capture_rs::{DXGIManager, CaptureError};
use std::time::Instant;

fn main() {
    println!("=== Testing DXGI Desktop Duplication API ===");
    unsafe {
        use windows_sys::Win32::System::StationsAndDesktops::*;
        let input_desk = OpenInputDesktop(0, 0, 0x10000000);
        if !input_desk.is_null() {
            let res = SetThreadDesktop(input_desk);
            println!("SetThreadDesktop: {}", res);
        }
    }
    let mut manager = match DXGIManager::new(100) {
        Ok(m) => {
            println!("DXGIManager initialized successfully!");
            m
        }
        Err(e) => {
            println!("Failed to create DXGIManager: {:?}", e);
            return;
        }
    };

    println!("Capturing 30 frames via DXGI GPU pipeline...");
    let start = Instant::now();
    let mut success = 0;
    let mut timeouts = 0;

    for i in 0..30 {
        let t0 = Instant::now();
        match manager.capture_frame() {
            Ok((pixels, (width, height))) => {
                success += 1;
                let dt = t0.elapsed();
                if i == 0 {
                    println!("BGRA8 size = {}, pixels len = {}", std::mem::size_of_val(&pixels[0]), pixels.len());
                    // Cast Vec<BGRA8> to &[u8]
                    let raw_u8: &[u8] = unsafe {
                        std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixels.len() * 4)
                    };
                    println!("Cast to &[u8] len = {} (expected {})", raw_u8.len(), width * height * 4);
                }
                if i < 5 || i == 29 {
                    println!("Frame {}: {}x{}, capture time: {:?}", i, width, height, dt);
                }
            }
            Err(CaptureError::Timeout) => {
                timeouts += 1;
            }
            Err(e) => {
                println!("Frame {} error: {:?}", i, e);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    let elapsed = start.elapsed();
    println!("--------------------------------------------------");
    println!("Total time: {:?}", elapsed);
    println!("Success: {} frames | Timeouts (static screen): {}", success, timeouts);
    println!("--------------------------------------------------");
}
