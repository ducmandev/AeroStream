use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const ID_BTN_OPEN_BROWSER: isize = 101;
const ID_BTN_COPY_LINK: isize = 102;
const ID_BTN_EXIT: isize = 103;

const SS_CENTER: u32 = 0x00000001;
const BS_PUSHBUTTON: u32 = 0x00000000;
const CF_UNICODETEXT: u32 = 13;

static mut G_CLIENT_URL: Option<String> = None;
static mut G_PIN: Option<String> = None;

fn to_wstring(str: &str) -> Vec<u16> {
    OsStr::new(str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

pub fn run_gui(
    ip: String,
    port: u16,
    pin: String,
    screen_w: u32,
    screen_h: u32,
) {
    let client_url = format!("http://{}:{}/?pin={}", ip, port, pin);
    unsafe {
        G_CLIENT_URL = Some(client_url.clone());
        G_PIN = Some(pin.clone());
    }

    unsafe {
        let hinstance = GetModuleHandleW(null_mut());
        let class_name = to_wstring("AeroStreamMainWindow");
        let wnd_title = to_wstring("AeroStream Remote Desktop Host");

        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            lpszMenuName: null_mut(),
            lpszClassName: class_name.as_ptr(),
        };

        let reg_res = RegisterClassW(&wnd_class);
        crate::debug_log::log_gui(&format!("RegisterClassW res: {}, err: {}", reg_res, GetLastError()));

        // Center on screen
        let win_w = 480;
        let win_h = 400;
        let screen_cx = GetSystemMetrics(SM_CXSCREEN);
        let screen_cy = GetSystemMetrics(SM_CYSCREEN);
        let pos_x = (screen_cx - win_w) / 2;
        let pos_y = (screen_cy - win_h) / 2;

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            wnd_title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
            pos_x,
            pos_y,
            win_w,
            win_h,
            null_mut(),
            null_mut(),
            hinstance,
            null_mut(),
        );

        crate::debug_log::log_gui(&format!("CreateWindowExW hwnd: {:?}, err: {}", hwnd, GetLastError()));

        if hwnd.is_null() {
            return;
        }

        let btn_cls = to_wstring("BUTTON");
        let static_cls = to_wstring("STATIC");

        // Title
        let title_text = to_wstring("AEROSTREAM REMOTE DESKTOP");
        CreateWindowExW(
            0,
            static_cls.as_ptr(),
            title_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_CENTER,
            20, 20, 420, 26,
            hwnd, null_mut(), hinstance, null_mut(),
        );

        // Subtitle
        let sub_text = to_wstring(&format!("Host Display: {}x{}  |  Server: Online", screen_w, screen_h));
        CreateWindowExW(
            0,
            static_cls.as_ptr(),
            sub_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_CENTER,
            20, 48, 420, 20,
            hwnd, null_mut(), hinstance, null_mut(),
        );

        // IP Address Label
        let ip_text = to_wstring(&format!("Host Address: http://{}:{}", ip, port));
        CreateWindowExW(
            0,
            static_cls.as_ptr(),
            ip_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_CENTER,
            20, 85, 420, 22,
            hwnd, null_mut(), hinstance, null_mut(),
        );

        // PIN Label
        let pin_title = to_wstring("SESSION SECURITY PIN");
        CreateWindowExW(
            0,
            static_cls.as_ptr(),
            pin_title.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_CENTER,
            20, 120, 420, 20,
            hwnd, null_mut(), hinstance, null_mut(),
        );

        // Large PIN Display
        let pin_display = to_wstring(&format!("[  {}  ]", pin));
        CreateWindowExW(
            0,
            static_cls.as_ptr(),
            pin_display.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_CENTER,
            20, 145, 420, 36,
            hwnd, null_mut(), hinstance, null_mut(),
        );

        // Button: Open in Browser
        let btn1_text = to_wstring("Open Web Viewer in Browser");
        CreateWindowExW(
            0,
            btn_cls.as_ptr(),
            btn1_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
            50, 200, 360, 40,
            hwnd, ID_BTN_OPEN_BROWSER as HMENU, hinstance, null_mut(),
        );

        // Button: Copy Link
        let btn2_text = to_wstring("Copy Client Connection Link");
        CreateWindowExW(
            0,
            btn_cls.as_ptr(),
            btn2_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
            50, 250, 360, 36,
            hwnd, ID_BTN_COPY_LINK as HMENU, hinstance, null_mut(),
        );

        // Button: Exit
        let btn3_text = to_wstring("Stop & Close Server");
        CreateWindowExW(
            0,
            btn_cls.as_ptr(),
            btn3_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
            50, 296, 360, 34,
            hwnd, ID_BTN_EXIT as HMENU, hinstance, null_mut(),
        );

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        // Windows Message Pump
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let res = GetMessageW(&mut msg, null_mut(), 0, 0);
            if res <= 0 {
                crate::debug_log::log_gui(&format!("GetMessageW terminated with res: {}, err: {}", res, GetLastError()));
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let ctrl_id = (wparam & 0xFFFF) as isize;
            match ctrl_id {
                ID_BTN_OPEN_BROWSER => {
                    if let Some(ref url) = G_CLIENT_URL {
                        let w_url = to_wstring(url);
                        let w_open = to_wstring("open");
                        ShellExecuteW(
                            hwnd,
                            w_open.as_ptr(),
                            w_url.as_ptr(),
                            null_mut(),
                            null_mut(),
                            SW_SHOWNORMAL,
                        );
                    }
                }
                ID_BTN_COPY_LINK => {
                    if let Some(ref url) = G_CLIENT_URL {
                        copy_to_clipboard(hwnd, url);
                        let msg_text = to_wstring("Client link copied to clipboard!");
                        let caption = to_wstring("AeroStream");
                        MessageBoxW(hwnd, msg_text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONINFORMATION);
                    }
                }
                ID_BTN_EXIT => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            crate::debug_log::log_gui("wnd_proc WM_CLOSE");
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            crate::debug_log::log_gui("wnd_proc WM_DESTROY");
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn copy_to_clipboard(hwnd: HWND, text: &str) {
    let w_str = to_wstring(text);
    let bytes_len = w_str.len() * 2;

    if OpenClipboard(hwnd) != 0 {
        EmptyClipboard();
        let h_glob = windows_sys::Win32::System::Memory::GlobalAlloc(
            windows_sys::Win32::System::Memory::GMEM_MOVEABLE,
            bytes_len,
        );
        if !h_glob.is_null() {
            let ptr = windows_sys::Win32::System::Memory::GlobalLock(h_glob);
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(w_str.as_ptr() as *const u8, ptr as *mut u8, bytes_len);
                windows_sys::Win32::System::Memory::GlobalUnlock(h_glob);
                SetClipboardData(CF_UNICODETEXT, h_glob);
            }
        }
        CloseClipboard();
    }
}
