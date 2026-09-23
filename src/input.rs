use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::LibraryLoader::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::RemoteDesktop::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InputMessage {
    #[serde(rename = "mouse_move")]
    MouseMove { x: f32, y: f32 },
    #[serde(rename = "mouse_delta")]
    MouseDelta { dx: i32, dy: i32 },
    #[serde(rename = "mouse_down")]
    MouseDown {
        button: String,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        y: Option<f32>,
    },
    #[serde(rename = "mouse_up")]
    MouseUp {
        button: String,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        y: Option<f32>,
    },
    #[serde(rename = "mouse_click")]
    MouseClick {
        button: String,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        y: Option<f32>,
    },
    #[serde(rename = "mouse_double")]
    MouseDouble {
        button: String,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        y: Option<f32>,
    },
    #[serde(rename = "mouse_wheel")]
    MouseWheel { delta_x: i32, delta_y: i32 },
    #[serde(rename = "key_down")]
    KeyDown { key: String },
    #[serde(rename = "key_up")]
    KeyUp { key: String },
    #[serde(rename = "key_click")]
    KeyClick { key: String },
    #[serde(rename = "shortcut")]
    Shortcut { keys: Vec<String> },
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "wake_lock_screen")]
    WakeLockScreen,
    #[serde(rename = "gamepad_axis")]
    GamepadAxis {
        stick: String, // "left" or "right"
        x: f32,        // -1.0 to 1.0
        y: f32,        // -1.0 to 1.0
    },
    #[serde(rename = "gamepad_button")]
    GamepadButton {
        button: String, // "A", "B", "X", "Y", "LB", "RB", etc.
        pressed: bool,
    },
}

/// IPC Named Pipe Client connecting to the SYSTEM Secure Agent worker (`\\.\pipe\aerostream-secure-agent`)
pub struct SecureAgentPipeClient {
    handle: HANDLE,
}

impl SecureAgentPipeClient {
    pub fn new() -> Self {
        Self {
            handle: INVALID_HANDLE_VALUE,
        }
    }

    fn ensure_connected(&mut self) -> bool {
        if self.handle != INVALID_HANDLE_VALUE {
            return true;
        }

        let pipe_name: Vec<u16> = crate::secure_agent::AGENT_PIPE_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let mut h = CreateFileW(
                pipe_name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );

            if h == INVALID_HANDLE_VALUE {
                let err = GetLastError();
                if err == ERROR_PIPE_BUSY {
                    WaitNamedPipeW(pipe_name.as_ptr(), 300);
                    h = CreateFileW(
                        pipe_name.as_ptr(),
                        GENERIC_READ | GENERIC_WRITE,
                        0,
                        std::ptr::null_mut(),
                        OPEN_EXISTING,
                        0,
                        std::ptr::null_mut(),
                    );
                }
            }

            if h != INVALID_HANDLE_VALUE {
                self.handle = h;
                true
            } else {
                false
            }
        }
    }

    pub fn send(&mut self, msg: &InputMessage) -> bool {
        let json = match serde_json::to_string(msg) {
            Ok(j) => j,
            Err(_) => return false,
        };
        let payload = format!("{}\n", json);
        let bytes = payload.as_bytes();

        for _ in 0..2 {
            if !self.ensure_connected() {
                continue;
            }

            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    self.handle,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };

            if ok != 0 && written > 0 {
                return true;
            } else {
                unsafe {
                    CloseHandle(self.handle);
                }
                self.handle = INVALID_HANDLE_VALUE;
            }
        }
        false
    }

    pub fn close(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = INVALID_HANDLE_VALUE;
        }
    }
}

impl Drop for SecureAgentPipeClient {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct InputManager {
    tx: std::sync::mpsc::Sender<InputMessage>,
    screen_width: Arc<AtomicU32>,
    screen_height: Arc<AtomicU32>,
}

impl InputManager {
    pub fn new(screen_width: u32, screen_height: u32) -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::channel::<InputMessage>();
        let sw = Arc::new(AtomicU32::new(screen_width));
        let sh = Arc::new(AtomicU32::new(screen_height));
        let sw_clone = sw.clone();
        let sh_clone = sh.clone();

        std::thread::Builder::new()
            .name("aerostream-input-worker".into())
            .spawn(move || {
                Self::run_input_worker(rx, sw_clone, sh_clone);
            })
            .map_err(|e| format!("Failed to spawn input worker: {:?}", e))?;

        Ok(Self {
            tx,
            screen_width: sw,
            screen_height: sh,
        })
    }

    #[allow(dead_code)]
    pub fn update_screen_size(&self, width: u32, height: u32) {
        self.screen_width.store(width, Ordering::Relaxed);
        self.screen_height.store(height, Ordering::Relaxed);
    }

    pub fn handle_input(&self, msg: InputMessage) {
        let _ = self.tx.send(msg);
    }

    unsafe fn from_wide_ptr(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        String::from_utf16_lossy(slice)
    }

    /// Duplicates the SYSTEM impersonation token from the winlogon.exe running in OUR
    /// session (works for both console and RDP sessions). The caller owns the handle.
    unsafe fn duplicate_winlogon_token() -> Option<HANDLE> {
        let mut session_id = 0u32;
        ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id);

        let mut p_proc_info: *mut WTS_PROCESS_INFOW = std::ptr::null_mut();
        let mut count: u32 = 0;

        if WTSEnumerateProcessesW(
            WTS_CURRENT_SERVER_HANDLE,
            0,
            1,
            &mut p_proc_info,
            &mut count,
        ) == 0 || p_proc_info.is_null() {
            return None;
        }

        let procs = std::slice::from_raw_parts(p_proc_info, count as usize);
        let mut winlogon_pid = None;

        for p in procs {
            if p.SessionId == session_id && !p.pProcessName.is_null() {
                let name = Self::from_wide_ptr(p.pProcessName);
                if name.eq_ignore_ascii_case("winlogon.exe") {
                    winlogon_pid = Some(p.ProcessId);
                    break;
                }
            }
        }
        WTSFreeMemory(p_proc_info as _);

        let pid = winlogon_pid?;

        // PROCESS_QUERY_LIMITED_INFORMATION (0x1000) or PROCESS_QUERY_INFORMATION (0x0400)
        let mut h_proc = OpenProcess(0x1000 | 0x0400, 0, pid);
        if h_proc.is_null() {
            h_proc = OpenProcess(0x1000, 0, pid);
        }
        if h_proc.is_null() {
            return None;
        }

        let mut result = None;
        let mut h_token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(h_proc, TOKEN_DUPLICATE | TOKEN_QUERY, &mut h_token) != 0 {
            let mut h_dup_token: HANDLE = std::ptr::null_mut();
            if DuplicateTokenEx(
                h_token,
                0x02000000, // MAXIMUM_ALLOWED
                std::ptr::null_mut(),
                SecurityImpersonation,
                TokenImpersonation,
                &mut h_dup_token,
            ) != 0 {
                result = Some(h_dup_token);
            }
            CloseHandle(h_token);
        }
        CloseHandle(h_proc);
        result
    }

    /// Cached duplicated SYSTEM token for secure-desktop injection. Stored as usize so
    /// it stays Send/Sync; 0 means "not acquired yet".
    unsafe fn impersonate_cached_system() -> bool {
        static WINLOGON_SYSTEM_TOKEN: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

        fn try_impersonate(guard_value: usize) -> bool {
            unsafe { ImpersonateLoggedOnUser(guard_value as HANDLE) != 0 }
        }

        let mut guard = WINLOGON_SYSTEM_TOKEN.lock().unwrap_or_else(|p| p.into_inner());
        if *guard == 0 {
            match Self::duplicate_winlogon_token() {
                Some(tok) => *guard = tok as usize,
                None => return false,
            }
        }
        if try_impersonate(*guard) {
            return true;
        }
        // Token went stale (winlogon restarted / reboot): drop and re-acquire once.
        CloseHandle(*guard as HANDLE);
        *guard = 0;
        match Self::duplicate_winlogon_token() {
            Some(tok) => {
                *guard = tok as usize;
                try_impersonate(*guard)
            }
            None => false,
        }
    }

    unsafe fn try_impersonate_winlogon() -> bool {
        Self::impersonate_cached_system()
    }

    pub unsafe fn get_desktop_name(h_desk: HDESK) -> String {
        if h_desk.is_null() {
            return String::new();
        }
        let mut needed = 0;
        GetUserObjectInformationW(h_desk, UOI_NAME, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            return String::new();
        }
        let mut buf = vec![0u16; (needed as usize) / 2 + 1];
        if GetUserObjectInformationW(
            h_desk,
            UOI_NAME,
            buf.as_mut_ptr() as _,
            needed,
            &mut needed,
        ) != 0
        {
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..len])
        } else {
            String::new()
        }
    }

    pub unsafe fn attach_input_desktop() -> bool {
        let is_locked = crate::capture::CaptureEngine::check_is_desktop_locked();
        let target_name = if is_locked { "Winlogon" } else { "Default" };

        let cur_desk = GetThreadDesktop(GetCurrentThreadId());
        let cur_name = Self::get_desktop_name(cur_desk);

        if cur_name.eq_ignore_ascii_case(target_name) {
            return true;
        }

        if !is_locked {
            // Target is Default desktop
            let desk = OpenInputDesktop(0, 0, 0x02000000);
            if !desk.is_null() {
                let res = SetThreadDesktop(desk);
                if res != 0 {
                    tracing::info!("Input worker attached to Default desktop");
                    return true;
                } else {
                    CloseDesktop(desk);
                }
            }
            return false;
        }

        // Target is Winlogon desktop (System is locked)
        let winlogon_name: Vec<u16> = "Winlogon\0".encode_utf16().collect();

        // 1. Direct try
        let mut desk = OpenDesktopW(winlogon_name.as_ptr(), 0, 0, 0x02000000);
        if desk.is_null() {
            desk = OpenInputDesktop(0, 0, 0x02000000);
        }

        // 2. If access denied, impersonate SYSTEM from winlogon.exe to grant DACL
        if desk.is_null() {
            if Self::try_impersonate_winlogon() {
                let winlogon_sys = OpenDesktopW(winlogon_name.as_ptr(), 0, 0, 0x00040000 | 0x02000000);
                if !winlogon_sys.is_null() {
                    let sec_res = SetSecurityInfo(
                        winlogon_sys as _,
                        SE_WINDOW_OBJECT,
                        DACL_SECURITY_INFORMATION,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(), // NULL DACL grants full access to Everyone
                        std::ptr::null_mut(),
                    );
                    tracing::info!("Configured NULL DACL on Winlogon desktop: {} (0 = SUCCESS)", sec_res);
                    CloseDesktop(winlogon_sys);
                }
                RevertToSelf();

                // Now open with primary token since DACL allows access
                desk = OpenDesktopW(winlogon_name.as_ptr(), 0, 0, 0x02000000);
                if desk.is_null() {
                    desk = OpenInputDesktop(0, 0, 0x02000000);
                }
            }
        }

        if !desk.is_null() {
            let res = SetThreadDesktop(desk);
            if res != 0 {
                tracing::info!("Input worker successfully attached to Winlogon desktop!");
                return true;
            } else {
                tracing::warn!("SetThreadDesktop to Winlogon failed: {}", GetLastError());
                CloseDesktop(desk);
            }
        }

        false
    }

    pub(crate) unsafe fn set_cursor_pos_with_retry(abs_x: i32, abs_y: i32) {
        if SetCursorPos(abs_x, abs_y) == 0 {
            let _ = Self::attach_input_desktop();
            SetCursorPos(abs_x, abs_y);
        }
    }

    pub(crate) unsafe fn dispatch_mouse_event(flags: u32, dx: i32, dy: i32, dw_data: i32, dw_extra_info: usize) {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            let _ = Self::attach_input_desktop();
        }
        mouse_event(flags, dx, dy, dw_data, dw_extra_info);
    }

    pub(crate) unsafe fn wake_windows_lock_screen(screen_w: u32, screen_h: u32) {
        tracing::info!("Waking Windows lock screen (LogonUI / Winlogon)...");

        // 1. SendSAS: official Microsoft mechanism to dismiss LockApp wallpaper and display LogonUI
        let sas_res = trigger_sas();
        tracing::info!("wake_windows_lock_screen: trigger_sas result: {}", sas_res);

        std::thread::sleep(std::time::Duration::from_millis(150));

        // 2. Attach input worker to Winlogon desktop
        let attached = Self::attach_input_desktop();
        tracing::info!("wake_windows_lock_screen: attach_input_desktop result: {}", attached);

        // 3. Fallback mouse swipe up gesture for touchscreen / slide-up dismiss
        let mid_x = (screen_w / 2) as i32;
        let start_y = (screen_h * 8 / 10) as i32;
        let end_y = (screen_h * 2 / 10) as i32;

        SetCursorPos(mid_x, start_y);
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        std::thread::sleep(std::time::Duration::from_millis(20));

        let steps = 8;
        for i in 1..=steps {
            let cur_y = start_y + ((end_y - start_y) * i / steps);
            SetCursorPos(mid_x, cur_y);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
        std::thread::sleep(std::time::Duration::from_millis(80));

        // 4. Send wake keystrokes (Space and Enter) to ensure credential input box has focus
        send_native_key_click(VK_SPACE as u8, false);
        std::thread::sleep(std::time::Duration::from_millis(80));
        send_native_key_click(VK_RETURN as u8, false);
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    pub(crate) unsafe fn update_native_key_state(vk: u8, is_down: &mut bool, should_be_down: bool) {
        if should_be_down && !*is_down {
            send_native_key_down(vk, false);
            *is_down = true;
        } else if !should_be_down && *is_down {
            send_native_key_up(vk, false);
            *is_down = false;
        }
    }

    fn run_input_worker(
        rx: std::sync::mpsc::Receiver<InputMessage>,
        sw: Arc<AtomicU32>,
        sh: Arc<AtomicU32>,
    ) {
        unsafe {
            let attached = Self::attach_input_desktop();
            crate::debug_log::log_gui(&format!("aerostream-input-worker attached to input desktop: {}", attached));
        }

        let mut stick_w = false;
        let mut stick_a = false;
        let mut stick_s = false;
        let mut stick_d = false;
        // Auto-wake latch: fires the lock-screen wake chain once per lock episode when
        // input keeps arriving; re-arms automatically after unlock.
        let mut was_locked_latch = false;
        let mut last_lock_probe = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(10))
            .unwrap_or_else(std::time::Instant::now);
        let mut secure_agent = SecureAgentPipeClient::new();

        while let Ok(msg) = rx.recv() {
            let screen_w = sw.load(Ordering::Relaxed).max(1);
            let screen_h = sh.load(Ordering::Relaxed).max(1);

            // Auto-wake & lock detection
            if last_lock_probe.elapsed() >= std::time::Duration::from_millis(400) {
                last_lock_probe = std::time::Instant::now();
                let locked = crate::capture::CaptureEngine::check_is_desktop_locked();
                if locked && !was_locked_latch {
                    tracing::info!("[IPC-SECURE] Lock state detected. Triggering WakeLockScreen on Secure Agent");
                    secure_agent.send(&InputMessage::WakeLockScreen);
                }
                was_locked_latch = locked;
            }

            // Check if current message is intended for or arriving during lock screen
            let is_locked = was_locked_latch
                || matches!(msg, InputMessage::WakeLockScreen)
                || crate::capture::CaptureEngine::check_is_desktop_locked();

            if is_locked {
                was_locked_latch = true;
                // Forward directly to the genuine SYSTEM Secure Agent worker over Named Pipe
                if secure_agent.send(&msg) {
                    tracing::debug!("[IPC-SECURE] Successfully forwarded input to SYSTEM Secure Agent: {:?}", msg);
                    continue;
                }
                tracing::warn!("[IPC-SECURE] Secure Agent pipe not responding; falling back to local dispatch");
            } else {
                secure_agent.close();
            }

            // While locked, inject under the SYSTEM token: SendInput is UIPI-blocked on the
            // System-integrity Winlogon desktop even for an elevated admin token — the
            // injecting thread must itself carry SYSTEM integrity to type into the lock screen.
            let inject_as_system = was_locked_latch;
            if inject_as_system {
                unsafe {
                    if Self::impersonate_cached_system() {
                        tracing::debug!("[UIPI] Injecting input under SYSTEM impersonation (locked)");
                    } else {
                        tracing::warn!("[UIPI] SYSTEM impersonation failed; input may not reach the lock screen");
                    }
                }
            }

            match msg {
                InputMessage::MouseMove { x, y } => {
                    let abs_x = (x * screen_w as f32).round() as i32;
                    let abs_y = (y * screen_h as f32).round() as i32;
                    unsafe {
                        Self::set_cursor_pos_with_retry(abs_x, abs_y);
                    }
                }
                InputMessage::MouseDelta { dx, dy } => {
                    unsafe {
                        Self::dispatch_mouse_event(MOUSEEVENTF_MOVE, dx, dy, 0, 0);
                    }
                }
                InputMessage::MouseDown { button, x, y } => {
                    if let (Some(px), Some(py)) = (x, y) {
                        let abs_x = (px * screen_w as f32).round() as i32;
                        let abs_y = (py * screen_h as f32).round() as i32;
                        unsafe {
                            Self::set_cursor_pos_with_retry(abs_x, abs_y);
                        }
                    }
                    unsafe {
                        let flag = match button.to_lowercase().as_str() {
                            "right" => MOUSEEVENTF_RIGHTDOWN,
                            "middle" => MOUSEEVENTF_MIDDLEDOWN,
                            _ => MOUSEEVENTF_LEFTDOWN,
                        };
                        Self::dispatch_mouse_event(flag, 0, 0, 0, 0);
                    }
                }
                InputMessage::MouseUp { button, x, y } => {
                    if let (Some(px), Some(py)) = (x, y) {
                        let abs_x = (px * screen_w as f32).round() as i32;
                        let abs_y = (py * screen_h as f32).round() as i32;
                        unsafe {
                            Self::set_cursor_pos_with_retry(abs_x, abs_y);
                        }
                    }
                    unsafe {
                        let flag = match button.to_lowercase().as_str() {
                            "right" => MOUSEEVENTF_RIGHTUP,
                            "middle" => MOUSEEVENTF_MIDDLEUP,
                            _ => MOUSEEVENTF_LEFTUP,
                        };
                        Self::dispatch_mouse_event(flag, 0, 0, 0, 0);
                    }
                }
                InputMessage::MouseClick { button, x, y } => {
                    if let (Some(px), Some(py)) = (x, y) {
                        let abs_x = (px * screen_w as f32).round() as i32;
                        let abs_y = (py * screen_h as f32).round() as i32;
                        unsafe {
                            Self::set_cursor_pos_with_retry(abs_x, abs_y);
                        }
                    }
                    unsafe {
                        let (down, up) = match button.to_lowercase().as_str() {
                            "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                            "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                            _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                        };
                        Self::dispatch_mouse_event(down, 0, 0, 0, 0);
                        Self::dispatch_mouse_event(up, 0, 0, 0, 0);
                    }
                }
                InputMessage::MouseDouble { button, x, y } => {
                    if let (Some(px), Some(py)) = (x, y) {
                        let abs_x = (px * screen_w as f32).round() as i32;
                        let abs_y = (py * screen_h as f32).round() as i32;
                        unsafe {
                            Self::set_cursor_pos_with_retry(abs_x, abs_y);
                        }
                    }
                    unsafe {
                        let (down, up) = match button.to_lowercase().as_str() {
                            "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                            "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                            _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                        };
                        Self::dispatch_mouse_event(down, 0, 0, 0, 0);
                        Self::dispatch_mouse_event(up, 0, 0, 0, 0);
                        std::thread::sleep(std::time::Duration::from_millis(25));
                        Self::dispatch_mouse_event(down, 0, 0, 0, 0);
                        Self::dispatch_mouse_event(up, 0, 0, 0, 0);
                    }
                }
                InputMessage::MouseWheel { delta_x: _, delta_y } => {
                    unsafe {
                        let wheel_delta = (delta_y * 120) as i32;
                        Self::dispatch_mouse_event(MOUSEEVENTF_WHEEL, 0, 0, wheel_delta, 0);
                    }
                }
                InputMessage::KeyDown { key } => {
                    unsafe {
                        Self::attach_input_desktop();
                        if let Some((vk, ext)) = key_name_to_vk(&key) {
                            send_native_key_down(vk, ext);
                        } else if key.chars().count() == 1 {
                            inject_native_char(key.chars().next().unwrap(), true, false);
                        }
                    }
                }
                InputMessage::KeyUp { key } => {
                    unsafe {
                        Self::attach_input_desktop();
                        if let Some((vk, ext)) = key_name_to_vk(&key) {
                            send_native_key_up(vk, ext);
                        } else if key.chars().count() == 1 {
                            inject_native_char(key.chars().next().unwrap(), false, true);
                        }
                    }
                }
                InputMessage::KeyClick { key } => {
                    unsafe {
                        Self::attach_input_desktop();
                        if let Some((vk, ext)) = key_name_to_vk(&key) {
                            send_native_key_click(vk, ext);
                        } else if key.chars().count() == 1 {
                            inject_native_char(key.chars().next().unwrap(), true, true);
                        }
                    }
                }
                InputMessage::Shortcut { keys } => {
                    unsafe {
                        Self::attach_input_desktop();
                        if is_cad(&keys) {
                            tracing::info!("Shortcut Ctrl+Alt+Del requested -> triggering SendSAS");
                            trigger_sas();
                            // Also send wake keys
                            send_native_key_click(VK_SPACE as u8, false);
                            send_native_key_click(VK_RETURN as u8, false);
                        } else {
                            let mut parsed = Vec::new();
                            for k in &keys {
                                if let Some(vk_info) = key_name_to_vk(k) {
                                    parsed.push(vk_info);
                                }
                            }
                            for &(vk, ext) in &parsed {
                                send_native_key_down(vk, ext);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(15));
                            for &(vk, ext) in parsed.iter().rev() {
                                send_native_key_up(vk, ext);
                            }
                        }
                    }
                }
                InputMessage::Text { text } => {
                    unsafe {
                        inject_native_text(&text);
                    }
                }
                InputMessage::WakeLockScreen => {
                    unsafe {
                        Self::wake_windows_lock_screen(screen_w, screen_h);
                    }
                }
                InputMessage::GamepadAxis { stick, x, y } => {
                    if stick == "left" {
                        let threshold = 0.35;
                        let should_w = y < -threshold;
                        let should_s = y > threshold;
                        let should_a = x < -threshold;
                        let should_d = x > threshold;

                        unsafe {
                            Self::update_native_key_state(b'W', &mut stick_w, should_w);
                            Self::update_native_key_state(b'S', &mut stick_s, should_s);
                            Self::update_native_key_state(b'A', &mut stick_a, should_a);
                            Self::update_native_key_state(b'D', &mut stick_d, should_d);
                        }
                    } else if stick == "right" {
                        let sens = 12.0;
                        let dx = (x * sens).round() as i32;
                        let dy = (y * sens).round() as i32;
                        if dx != 0 || dy != 0 {
                            unsafe {
                                Self::dispatch_mouse_event(MOUSEEVENTF_MOVE, dx, dy, 0, 0);
                            }
                        }
                    }
                }
                InputMessage::GamepadButton { button, pressed } => {
                    unsafe {
                        match button.to_uppercase().as_str() {
                            "A" => {
                                if pressed {
                                    send_native_key_down(VK_SPACE as u8, false);
                                } else {
                                    send_native_key_up(VK_SPACE as u8, false);
                                }
                            }
                            "B" => {
                                if pressed {
                                    send_native_key_down(VK_CONTROL as u8, false);
                                } else {
                                    send_native_key_up(VK_CONTROL as u8, false);
                                }
                            }
                            "X" => {
                                if pressed {
                                    send_native_key_down(b'E', false);
                                } else {
                                    send_native_key_up(b'E', false);
                                }
                            }
                            "Y" => {
                                if pressed {
                                    send_native_key_down(b'R', false);
                                } else {
                                    send_native_key_up(b'R', false);
                                }
                            }
                            "LB" => {
                                if pressed {
                                    send_native_key_down(VK_SHIFT as u8, false);
                                } else {
                                    send_native_key_up(VK_SHIFT as u8, false);
                                }
                            }
                            "RB" => {
                                if pressed {
                                    send_native_key_down(b'F', false);
                                } else {
                                    send_native_key_up(b'F', false);
                                }
                            }
                            "LT" => {
                                let flag = if pressed {
                                    MOUSEEVENTF_RIGHTDOWN
                                } else {
                                    MOUSEEVENTF_RIGHTUP
                                };
                                Self::dispatch_mouse_event(flag, 0, 0, 0, 0);
                            }
                            "RT" => {
                                let flag = if pressed {
                                    MOUSEEVENTF_LEFTDOWN
                                } else {
                                    MOUSEEVENTF_LEFTUP
                                };
                                Self::dispatch_mouse_event(flag, 0, 0, 0, 0);
                            }
                            "START" => {
                                if pressed {
                                    send_native_key_down(VK_ESCAPE as u8, false);
                                } else {
                                    send_native_key_up(VK_ESCAPE as u8, false);
                                }
                            }
                            "SELECT" => {
                                if pressed {
                                    send_native_key_down(VK_TAB as u8, false);
                                } else {
                                    send_native_key_up(VK_TAB as u8, false);
                                }
                            }
                            "UP" => {
                                if pressed {
                                    send_native_key_down(VK_UP as u8, true);
                                } else {
                                    send_native_key_up(VK_UP as u8, true);
                                }
                            }
                            "DOWN" => {
                                if pressed {
                                    send_native_key_down(VK_DOWN as u8, true);
                                } else {
                                    send_native_key_up(VK_DOWN as u8, true);
                                }
                            }
                            "LEFT" => {
                                if pressed {
                                    send_native_key_down(VK_LEFT as u8, true);
                                } else {
                                    send_native_key_up(VK_LEFT as u8, true);
                                }
                            }
                            "RIGHT" => {
                                if pressed {
                                    send_native_key_down(VK_RIGHT as u8, true);
                                } else {
                                    send_native_key_up(VK_RIGHT as u8, true);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            if inject_as_system {
                unsafe {
                    RevertToSelf();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Native Keyboard Dispatcher (Driver Level Injection via keybd_event)
// ---------------------------------------------------------------------------

pub(crate) unsafe fn send_native_key_down(vk: u8, is_extended: bool) {
    let scan = MapVirtualKeyW(vk as u32, 0) as u8;
    let mut flags = 0;
    if is_extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    keybd_event(vk, scan, flags, 0);
}

pub(crate) unsafe fn send_native_key_up(vk: u8, is_extended: bool) {
    let scan = MapVirtualKeyW(vk as u32, 0) as u8;
    let mut flags = KEYEVENTF_KEYUP;
    if is_extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    keybd_event(vk, scan, flags, 0);
}

pub(crate) unsafe fn send_native_key_click(vk: u8, is_extended: bool) {
    send_native_key_down(vk, is_extended);
    std::thread::sleep(std::time::Duration::from_millis(15));
    send_native_key_up(vk, is_extended);
}

pub(crate) unsafe fn inject_native_char(ch: char, press: bool, release: bool) {
    if ch == '\n' || ch == '\r' {
        if press {
            send_native_key_down(VK_RETURN as u8, false);
        }
        if press && release {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if release {
            send_native_key_up(VK_RETURN as u8, false);
        }
        return;
    }
    if ch == '\t' {
        if press {
            send_native_key_down(VK_TAB as u8, false);
        }
        if press && release {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if release {
            send_native_key_up(VK_TAB as u8, false);
        }
        return;
    }
    if ch == '\x08' {
        if press {
            send_native_key_down(VK_BACK as u8, false);
        }
        if press && release {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if release {
            send_native_key_up(VK_BACK as u8, false);
        }
        return;
    }

    let mut buf = [0u16; 2];
    let utf16 = ch.encode_utf16(&mut buf);
    if utf16.len() != 1 {
        for &cu in utf16.iter() {
            if press {
                keybd_event(0, (cu & 0xFF) as u8, KEYEVENTF_UNICODE, 0);
            }
            if press && release {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if release {
                keybd_event(0, (cu & 0xFF) as u8, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, 0);
            }
        }
        return;
    }

    let res = VkKeyScanW(utf16[0]);
    if res != -1 {
        let vk = (res & 0xFF) as u8;
        let shift_state = ((res >> 8) & 0xFF) as u8;
        let need_shift = (shift_state & 1) != 0;
        let need_ctrl = (shift_state & 2) != 0;
        let need_alt = (shift_state & 4) != 0;

        let scan = MapVirtualKeyW(vk as u32, 0) as u8;

        if press {
            if need_shift {
                keybd_event(VK_SHIFT as u8, 0x2A, 0, 0);
            }
            if need_ctrl {
                keybd_event(VK_CONTROL as u8, 0x1D, 0, 0);
            }
            if need_alt {
                keybd_event(VK_MENU as u8, 0x38, 0, 0);
            }
            keybd_event(vk, scan, 0, 0);
        }
        if press && release {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if release {
            keybd_event(vk, scan, KEYEVENTF_KEYUP, 0);
            if need_alt {
                keybd_event(VK_MENU as u8, 0x38, KEYEVENTF_KEYUP, 0);
            }
            if need_ctrl {
                keybd_event(VK_CONTROL as u8, 0x1D, KEYEVENTF_KEYUP, 0);
            }
            if need_shift {
                keybd_event(VK_SHIFT as u8, 0x2A, KEYEVENTF_KEYUP, 0);
            }
        }
    } else {
        if press {
            keybd_event(0, (utf16[0] & 0xFF) as u8, KEYEVENTF_UNICODE, 0);
        }
        if press && release {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if release {
            keybd_event(0, (utf16[0] & 0xFF) as u8, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, 0);
        }
    }
}

pub(crate) unsafe fn inject_native_text(text: &str) {
    InputManager::attach_input_desktop();
    tracing::info!("Injecting native text into active desktop (length: {} chars)...", text.len());
    for ch in text.chars() {
        inject_native_char(ch, true, true);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

pub(crate) fn is_cad(keys: &[String]) -> bool {
    let mut has_ctrl = false;
    let mut has_alt = false;
    let mut has_del = false;
    for k in keys {
        let l = k.to_lowercase();
        if l == "ctrl" || l == "control" {
            has_ctrl = true;
        }
        if l == "alt" || l == "menu" {
            has_alt = true;
        }
        if l == "delete" || l == "del" {
            has_del = true;
        }
    }
    has_ctrl && has_alt && has_del
}

pub(crate) unsafe fn trigger_sas() -> bool {
    let lib_name: Vec<u16> = "sas.dll\0".encode_utf16().collect();
    let h_module = LoadLibraryW(lib_name.as_ptr());
    if h_module.is_null() {
        tracing::warn!("Failed to load sas.dll for SendSAS");
        return false;
    }
    type SendSASFn = unsafe extern "system" fn(i32);
    let proc_name = std::ffi::CString::new("SendSAS").unwrap();
    let proc_addr = GetProcAddress(h_module, proc_name.as_ptr() as _);
    let mut success = false;
    if let Some(send_sas) = proc_addr {
        let send_sas: SendSASFn = std::mem::transmute(send_sas);
        send_sas(0); // 0 = FALSE (AsUser = FALSE)
        tracing::info!("Triggered SendSAS(0) successfully");
        success = true;
    } else {
        tracing::warn!("SendSAS function not found in sas.dll");
    }
    FreeLibrary(h_module);
    success
}

pub(crate) fn key_name_to_vk(name: &str) -> Option<(u8, bool)> {
    match name.to_lowercase().as_str() {
        "enter" | "return" => Some((VK_RETURN as u8, false)),
        "backspace" | "back" => Some((VK_BACK as u8, false)),
        "tab" => Some((VK_TAB as u8, false)),
        "space" | " " => Some((VK_SPACE as u8, false)),
        "escape" | "esc" => Some((VK_ESCAPE as u8, false)),
        "ctrl" | "control" | "lctrl" | "leftcontrol" => Some((VK_CONTROL as u8, false)),
        "rctrl" | "rightcontrol" => Some((VK_RCONTROL as u8, true)),
        "shift" | "lshift" | "leftshift" => Some((VK_SHIFT as u8, false)),
        "rshift" | "rightshift" => Some((VK_RSHIFT as u8, false)),
        "alt" | "menu" | "lalt" | "leftalt" => Some((VK_MENU as u8, false)),
        "ralt" | "rightalt" => Some((VK_RMENU as u8, true)),
        "win" | "meta" | "cmd" | "lwin" | "leftwin" => Some((VK_LWIN as u8, true)),
        "rwin" | "rightwin" => Some((VK_RWIN as u8, true)),
        "del" | "delete" => Some((VK_DELETE as u8, true)),
        "insert" | "ins" => Some((VK_INSERT as u8, true)),
        "home" => Some((VK_HOME as u8, true)),
        "end" => Some((VK_END as u8, true)),
        "pageup" | "pgup" => Some((VK_PRIOR as u8, true)),
        "pagedown" | "pgdn" => Some((VK_NEXT as u8, true)),
        "up" | "arrowup" => Some((VK_UP as u8, true)),
        "down" | "arrowdown" => Some((VK_DOWN as u8, true)),
        "left" | "arrowleft" => Some((VK_LEFT as u8, true)),
        "right" | "arrowright" => Some((VK_RIGHT as u8, true)),
        "capslock" | "caps" => Some((VK_CAPITAL as u8, false)),
        "numlock" => Some((VK_NUMLOCK as u8, true)),
        "scrolllock" => Some((VK_SCROLL as u8, false)),
        "printscreen" | "prtscr" => Some((VK_SNAPSHOT as u8, false)),
        "pause" => Some((VK_PAUSE as u8, false)),
        "f1" => Some((VK_F1 as u8, false)),
        "f2" => Some((VK_F2 as u8, false)),
        "f3" => Some((VK_F3 as u8, false)),
        "f4" => Some((VK_F4 as u8, false)),
        "f5" => Some((VK_F5 as u8, false)),
        "f6" => Some((VK_F6 as u8, false)),
        "f7" => Some((VK_F7 as u8, false)),
        "f8" => Some((VK_F8 as u8, false)),
        "f9" => Some((VK_F9 as u8, false)),
        "f10" => Some((VK_F10 as u8, false)),
        "f11" => Some((VK_F11 as u8, false)),
        "f12" => Some((VK_F12 as u8, false)),
        "0" => Some((0x30, false)),
        "1" => Some((0x31, false)),
        "2" => Some((0x32, false)),
        "3" => Some((0x33, false)),
        "4" => Some((0x34, false)),
        "5" => Some((0x35, false)),
        "6" => Some((0x36, false)),
        "7" => Some((0x37, false)),
        "8" => Some((0x38, false)),
        "9" => Some((0x39, false)),
        "a" => Some((0x41, false)),
        "b" => Some((0x42, false)),
        "c" => Some((0x43, false)),
        "d" => Some((0x44, false)),
        "e" => Some((0x45, false)),
        "f" => Some((0x46, false)),
        "g" => Some((0x47, false)),
        "h" => Some((0x48, false)),
        "i" => Some((0x49, false)),
        "j" => Some((0x4A, false)),
        "k" => Some((0x4B, false)),
        "l" => Some((0x4C, false)),
        "m" => Some((0x4D, false)),
        "n" => Some((0x4E, false)),
        "o" => Some((0x4F, false)),
        "p" => Some((0x50, false)),
        "q" => Some((0x51, false)),
        "r" => Some((0x52, false)),
        "s" => Some((0x53, false)),
        "t" => Some((0x54, false)),
        "u" => Some((0x55, false)),
        "v" => Some((0x56, false)),
        "w" => Some((0x57, false)),
        "x" => Some((0x58, false)),
        "y" => Some((0x59, false)),
        "z" => Some((0x5A, false)),
        s if s.chars().count() == 1 => {
            let ch = s.chars().next().unwrap();
            let mut buf = [0u16; 2];
            let utf16 = ch.encode_utf16(&mut buf);
            let res = unsafe { VkKeyScanW(utf16[0]) };
            if res != -1 {
                Some(((res & 0xFF) as u8, false))
            } else {
                None
            }
        }
        _ => None,
    }
}
