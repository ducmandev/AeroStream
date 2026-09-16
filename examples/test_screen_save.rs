use std::ptr::null_mut;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn main() {
    unsafe {
        let win_station = GetProcessWindowStation();
        println!("Process Window Station: {:?}", win_station);

        let desk = GetThreadDesktop(GetCurrentThreadId());
        println!("Thread Desktop: {:?}", desk);

        let input_desk = OpenInputDesktop(0, 0, 0x10000000); // MAXIMUM_ALLOWED / GENERIC_ALL
        println!("OpenInputDesktop: {:?}, err: {}", input_desk, GetLastError());

        if !input_desk.is_null() {
            let set_res = SetThreadDesktop(input_desk);
            println!("SetThreadDesktop res: {}", set_res);
            
            // Now try GetDC and BitBlt
            let hdc_screen = GetDC(null_mut());
            println!("hdc_screen after SetThreadDesktop: {:?}", hdc_screen);

            let width = GetSystemMetrics(SM_CXSCREEN);
            let height = GetSystemMetrics(SM_CYSCREEN);
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            let hbm = CreateCompatibleBitmap(hdc_screen, width, height);
            let old = SelectObject(hdc_mem, hbm);

            SetLastError(0);
            let res = BitBlt(hdc_mem, 0, 0, width, height, hdc_screen, 0, 0, SRCCOPY);
            println!("BitBlt after SetThreadDesktop: res={}, err={}", res, GetLastError());

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
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

            let mut bgra: Vec<u8> = vec![0u8; (width * height * 4) as usize];
            GetDIBits(hdc_mem, hbm, 0, height as u32, bgra.as_mut_ptr() as *mut _, &mut bmi, DIB_RGB_COLORS);
            let non_zero = bgra.iter().filter(|&&b| b != 0).count();
            println!("Non-zero bytes after SetThreadDesktop = {} / {}", non_zero, bgra.len());

            SelectObject(hdc_mem, old);
            DeleteObject(hbm);
            DeleteDC(hdc_mem);
            ReleaseDC(null_mut(), hdc_screen);
            CloseDesktop(input_desk);
        }
    }
}
