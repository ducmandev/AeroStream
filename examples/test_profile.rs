use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;
use std::ptr::null_mut;
use std::time::Instant;
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

        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm = CreateCompatibleBitmap(hdc_screen, src_w, src_h);
        let old_obj = SelectObject(hdc_mem, hbm);

        let mut bgra = vec![0u8; (src_w * src_h * 4) as usize];
        let mut rgb = vec![0u8; (src_w * src_h * 3) as usize];
        let mut jpeg = Vec::with_capacity(rgb.len() / 8);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: src_w,
                biHeight: -src_h,
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

        let t0 = Instant::now();
        BitBlt(hdc_mem, 0, 0, src_w, src_h, hdc_screen, 0, 0, SRCCOPY);
        let bitblt_time = t0.elapsed();

        let t1 = Instant::now();
        GetDIBits(hdc_mem, hbm, 0, src_h as u32, bgra.as_mut_ptr() as *mut _, &mut bmi, DIB_RGB_COLORS);
        let dib_time = t1.elapsed();

        // Method 1: Old image crate (BGRA -> RGB -> JpegEncoder)
        let t2 = Instant::now();
        rgb.clear();
        for chunk in bgra.chunks_exact(4) {
            rgb.push(chunk[2]);
            rgb.push(chunk[1]);
            rgb.push(chunk[0]);
        }
        let convert_time = t2.elapsed();

        let t3 = Instant::now();
        jpeg.clear();
        let mut encoder = JpegEncoder::new_with_quality(&mut jpeg, 65);
        encoder.encode(&rgb, src_w as u32, src_h as u32, ExtendedColorType::Rgb8).unwrap();
        let image_encode_time = t3.elapsed();

        // Method 2: jpeg-encoder crate directly with Bgra (no conversion!)
        let t4 = Instant::now();
        let mut fast_jpeg = Vec::with_capacity(jpeg.len());
        let fast_enc = jpeg_encoder::Encoder::new(&mut fast_jpeg, 65);
        fast_enc.encode(&bgra, src_w as u16, src_h as u16, jpeg_encoder::ColorType::Bgra).unwrap();
        let fast_encode_time = t4.elapsed();

        println!("--- Profiling 1080p Frame ---");
        println!("BitBlt time                 : {:?}", bitblt_time);
        println!("GetDIBits time              : {:?}", dib_time);
        println!("Method 1 (convert+image enc): {:?}", convert_time + image_encode_time);
        println!("Method 2 (jpeg-encoder Bgra): {:?}", fast_encode_time);
        println!("Size 1: {} KB, Size 2: {} KB", jpeg.len() / 1024, fast_jpeg.len() / 1024);

        SelectObject(hdc_mem, old_obj);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(null_mut(), hdc_screen);
    }
}
