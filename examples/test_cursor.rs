use std::ptr::null_mut;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn main() {
    unsafe {
        let mut ci: CURSORINFO = std::mem::zeroed();
        ci.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
        if GetCursorInfo(&mut ci) != 0 && ci.flags == CURSOR_SHOWING {
            println!("Cursor is showing at ({}, {})", ci.ptScreenPos.x, ci.ptScreenPos.y);
        } else {
            println!("Cursor info not available or hidden");
        }
    }
}
