use std::ptr::null_mut;
use std::time::Instant;
use windows_sys::Win32::System::StationsAndDesktops::*;

fn main() {
    unsafe {
        let input_desk = OpenInputDesktop(0, 0, 0x10000000);
        if !input_desk.is_null() {
            let res = SetThreadDesktop(input_desk);
            println!("SetThreadDesktop: {}", res);
        }
    }

    println!("Testing xcap after SetThreadDesktop...");
    let t0 = Instant::now();
    let monitors = match xcap::Monitor::all() {
        Ok(m) => m,
        Err(e) => {
            println!("xcap::Monitor::all error: {:?}", e);
            return;
        }
    };
    println!("Found {} monitors in {:?}", monitors.len(), t0.elapsed());

    if let Some(m) = monitors.first() {
        for i in 1..=3 {
            let t_cap = Instant::now();
            match m.capture_image() {
                Ok(img) => {
                    println!("Run {}: Captured {}x{} in {:?}", i, img.width(), img.height(), t_cap.elapsed());
                }
                Err(e) => {
                    println!("Run {}: Capture failed with {:?}", i, e);
                }
            }
        }
    }
}
