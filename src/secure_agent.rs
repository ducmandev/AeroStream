use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use jpeg_encoder::{ColorType, Encoder};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::RemoteDesktop::*;
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::input::InputMessage;

pub const SERVICE_NAME: &str = "AeroStreamSecureHelper";
pub const SERVICE_DISPLAY_NAME: &str = "AeroStream Secure Helper Service";
pub const PROOF_FILE_PATH: &str = r"C:\Windows\Temp\aerostream-agent-proof.txt";
pub const SECURE_FRAME_PATH: &str = r"C:\Windows\Temp\aerostream-secure-frame.jpg";
pub const SECURE_FRAME_TMP_PATH: &str = r"C:\Windows\Temp\aerostream-secure-frame.tmp";
pub const LOG_SVC_PATH: &str = r"C:\Windows\Temp\aerostream-secure-svc.log";
pub const LOG_AGENT_PATH: &str = r"C:\Windows\Temp\aerostream-secure-agent.log";
pub const AGENT_PIPE_NAME: &str = r"\\.\pipe\aerostream-secure-agent";

#[link(name = "userenv")]
extern "system" {
    fn CreateEnvironmentBlock(
        lpEnvironment: *mut *mut std::ffi::c_void,
        hToken: HANDLE,
        bInherit: BOOL,
    ) -> BOOL;
    fn DestroyEnvironmentBlock(lpEnvironment: *mut std::ffi::c_void) -> BOOL;
}

#[link(name = "advapi32")]
extern "system" {
    fn GetUserNameW(lpBuffer: *mut u16, pcbBuffer: *mut u32) -> BOOL;
}

fn to_wstring(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn allow_everyone_access(path: &str) {
    unsafe {
        let mut sd: SECURITY_DESCRIPTOR = std::mem::zeroed();
        if InitializeSecurityDescriptor(&mut sd as *mut _ as *mut std::ffi::c_void, 1) != 0 {
            if SetSecurityDescriptorDacl(&mut sd as *mut _ as *mut std::ffi::c_void, 1, null_mut(), 0) != 0 {
                let path_w = to_wstring(path);
                SetFileSecurityW(path_w.as_ptr(), DACL_SECURITY_INFORMATION, &mut sd as *mut _ as *mut std::ffi::c_void);
            }
        }
    }
}

pub fn log_to_file(path: &str, line: &str) {
    let existed = std::path::Path::new(path).exists();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}", chrono_like_now(), line);
        if !existed {
            allow_everyone_access(path);
        }
    }
}

fn chrono_like_now() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    format!("{}.{:03}s epoch", secs, millis)
}

/// Checks whether the current process token is elevated.
pub fn is_process_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut ret_len = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Enables necessary token privileges on current process (SeDebugPrivilege, SeAssignPrimaryTokenPrivilege, etc.)
pub unsafe fn enable_token_privileges() {
    let mut token: HANDLE = null_mut();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) != 0 {
        for priv_name in &[
            "SeDebugPrivilege",
            "SeAssignPrimaryTokenPrivilege",
            "SeIncreaseQuotaPrivilege",
            "SeTcbPrivilege",
            "SeImpersonatePrivilege",
        ] {
            let wide = to_wstring(priv_name);
            let mut luid: LUID = std::mem::zeroed();
            if LookupPrivilegeValueW(null(), wide.as_ptr(), &mut luid) != 0 {
                let mut tp = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES {
                        Luid: luid,
                        Attributes: SE_PRIVILEGE_ENABLED,
                    }],
                };
                let _ = AdjustTokenPrivileges(
                    token,
                    0,
                    &mut tp as *mut _ as *mut _,
                    std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                    null_mut(),
                    null_mut(),
                );
            }
        }
        CloseHandle(token);
    }
}

/// Returns the session ID for the current process, fallback to active console session ID.
pub fn get_current_session_id() -> u32 {
    let mut session_id = 0u32;
    unsafe {
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) != 0 {
            return session_id;
        }
        WTSGetActiveConsoleSessionId()
    }
}

/// Retrieves the username running the current process via GetUserNameW.
pub fn get_current_username() -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        if GetUserNameW(buf.as_mut_ptr(), &mut len) != 0 && len > 0 {
            let actual_len = if len as usize <= buf.len() { (len - 1) as usize } else { buf.len() };
            String::from_utf16_lossy(&buf[..actual_len])
        } else {
            std::env::var("USERNAME").unwrap_or_else(|_| "UNKNOWN".to_string())
        }
    }
}

/// Retrieves the desktop object name.
pub unsafe fn get_desktop_name(h_desk: HDESK) -> String {
    if h_desk.is_null() {
        return "(null)".to_string();
    }
    let mut needed = 0u32;
    GetUserObjectInformationW(h_desk, UOI_NAME, null_mut(), 0, &mut needed);
    if needed == 0 {
        return "(unknown)".to_string();
    }
    let mut buf = vec![0u16; (needed as usize) / 2 + 1];
    if GetUserObjectInformationW(h_desk, UOI_NAME, buf.as_mut_ptr() as _, needed, &mut needed) != 0 {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    } else {
        "(unknown)".to_string()
    }
}

/// Manages desktop switching for the calling thread following RustDesk windows.cc (selectInputDesktop/switchToDesktop).
pub struct DesktopManager {
    last_desktop: HDESK,
    pub current_name: String,
}

impl DesktopManager {
    pub fn new() -> Self {
        Self {
            last_desktop: null_mut(),
            current_name: String::new(),
        }
    }

    pub unsafe fn sync(&mut self) -> (bool, String) {
        let cur = GetThreadDesktop(GetCurrentThreadId());
        let cur_name = get_desktop_name(cur);

        // Open input desktop with DESKTOP_SWITCHDESKTOP | GENERIC_WRITE | DESKTOP_READOBJECTS | DESKTOP_ENUMERATE
        let access = 0x0100 | 0x40000000 | 0x0001 | 0x0040 | 0x0002;
        let mut input = OpenInputDesktop(0, 0, access);
        if input.is_null() {
            input = OpenInputDesktop(0, 0, 0x02000000); // MAXIMUM_ALLOWED
        }
        if input.is_null() {
            return (false, cur_name);
        }

        let input_name = get_desktop_name(input);
        if cur_name.eq_ignore_ascii_case(&input_name) {
            CloseDesktop(input);
            self.current_name = cur_name.clone();
            return (false, cur_name);
        }

        log_to_file(
            LOG_AGENT_PATH,
            &format!("[DESKTOP-SWITCH] Transition detected: '{}' -> '{}'. Calling SetThreadDesktop...", cur_name, input_name),
        );

        let ok = SetThreadDesktop(input);
        if ok != 0 {
            log_to_file(
                LOG_AGENT_PATH,
                &format!("[DESKTOP-SWITCH] SetThreadDesktop succeeded to '{}'", input_name),
            );
            if !self.last_desktop.is_null() {
                CloseDesktop(self.last_desktop);
            }
            self.last_desktop = input;
            self.current_name = input_name.clone();
            (true, input_name)
        } else {
            let err = GetLastError();
            log_to_file(
                LOG_AGENT_PATH,
                &format!("[DESKTOP-SWITCH] SetThreadDesktop to '{}' failed: error {}", input_name, err),
            );
            CloseDesktop(input);
            (false, cur_name)
        }
    }
}

impl Drop for DesktopManager {
    fn drop(&mut self) {
        unsafe {
            if !self.last_desktop.is_null() {
                CloseDesktop(self.last_desktop);
                self.last_desktop = null_mut();
            }
        }
    }
}

/// Enumerates processes to locate winlogon.exe belonging to the given session.
pub unsafe fn find_winlogon_pid(session_id: u32) -> Option<u32> {
    let mut p_proc_info: *mut WTS_PROCESS_INFOW = null_mut();
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
            let mut len = 0;
            while *p.pProcessName.add(len) != 0 {
                len += 1;
            }
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(p.pProcessName, len));
            if name.eq_ignore_ascii_case("winlogon.exe") {
                winlogon_pid = Some(p.ProcessId);
                break;
            }
        }
    }
    WTSFreeMemory(p_proc_info as _);
    winlogon_pid
}

/// Duplicates the PRIMARY token from winlogon.exe of target session.
/// Unlike impersonation tokens, a PRIMARY token (TokenPrimary) is mandatory for CreateProcessAsUserW.
pub unsafe fn duplicate_winlogon_primary_token(session_id: u32) -> Result<HANDLE, String> {
    let pid = find_winlogon_pid(session_id)
        .ok_or_else(|| format!("winlogon.exe not found in session {}", session_id))?;

    let mut h_proc = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_DUP_HANDLE, 0, pid);
    if h_proc.is_null() {
        h_proc = OpenProcess(0x1000 | 0x0400, 0, pid);
    }
    if h_proc.is_null() {
        return Err(format!("OpenProcess on winlogon (PID {}) failed: {}", pid, GetLastError()));
    }

    let mut h_token: HANDLE = null_mut();
    if OpenProcessToken(h_proc, TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY, &mut h_token) == 0 {
        let err = GetLastError();
        CloseHandle(h_proc);
        return Err(format!("OpenProcessToken on winlogon failed: {}", err));
    }

    let mut h_primary_token: HANDLE = null_mut();
    let dup_ok = DuplicateTokenEx(
        h_token,
        0x02000000, // MAXIMUM_ALLOWED
        null_mut(),
        SecurityImpersonation,
        TokenPrimary,
        &mut h_primary_token,
    );

    let err = GetLastError();
    CloseHandle(h_token);
    CloseHandle(h_proc);

    if dup_ok == 0 {
        return Err(format!("DuplicateTokenEx(TokenPrimary) failed: {}", err));
    }

    let mut sess = session_id;
    let _ = SetTokenInformation(
        h_primary_token,
        TokenSessionId,
        &mut sess as *mut _ as *mut _,
        std::mem::size_of::<u32>() as u32,
    );

    Ok(h_primary_token)
}

// ---------------------------------------------------------------------------
// Role 1: Transient Service (--secure-svc [session_id])
// ---------------------------------------------------------------------------

static G_STATUS_HANDLE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static G_TARGET_SESSION_ID: AtomicU32 = AtomicU32::new(0xFFFFFFFF);

unsafe extern "system" fn service_ctrl_handler(control: u32) {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            let handle = G_STATUS_HANDLE.load(Ordering::SeqCst) as SERVICE_STATUS_HANDLE;
            if !handle.is_null() {
                let status = SERVICE_STATUS {
                    dwServiceType: SERVICE_WIN32_OWN_PROCESS,
                    dwCurrentState: SERVICE_STOPPED,
                    dwControlsAccepted: 0,
                    dwWin32ExitCode: 0,
                    dwServiceSpecificExitCode: 0,
                    dwCheckPoint: 0,
                    dwWaitHint: 0,
                };
                SetServiceStatus(handle, &status);
            }
        }
        _ => {}
    }
}

unsafe fn launch_agent_into_session(session_id: u32) -> Result<u32, String> {
    log_to_file(LOG_SVC_PATH, &format!("Attempting to spawn secure agent into session {}", session_id));

    enable_token_privileges();

    let h_primary_token = duplicate_winlogon_primary_token(session_id)?;
    log_to_file(LOG_SVC_PATH, "Acquired winlogon primary token successfully");

    let mut env_block: *mut std::ffi::c_void = null_mut();
    let has_env = CreateEnvironmentBlock(&mut env_block, h_primary_token, 0) != 0;
    if has_env {
        log_to_file(LOG_SVC_PATH, "Created Unicode environment block");
    }

    let mut si: STARTUPINFOW = std::mem::zeroed();
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut desk_w = to_wstring(r"winsta0\default");
    si.lpDesktop = desk_w.as_mut_ptr();

    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd_line = to_wstring(&format!("\"{}\" --secure-agent", current_exe.display()));

    let mut flags = 0u32;
    if has_env {
        flags |= CREATE_UNICODE_ENVIRONMENT;
    }

    let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
    let create_ok = CreateProcessAsUserW(
        h_primary_token,
        null(),
        cmd_line.as_mut_ptr(),
        null_mut(),
        null_mut(),
        0,
        flags,
        if has_env { env_block } else { null_mut() },
        null(),
        &si,
        &mut pi,
    );

    let err = GetLastError();

    if has_env && !env_block.is_null() {
        DestroyEnvironmentBlock(env_block);
    }
    CloseHandle(h_primary_token);

    if create_ok != 0 {
        let child_pid = pi.dwProcessId;
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        log_to_file(LOG_SVC_PATH, &format!("CreateProcessAsUserW SUCCEEDED! Child PID: {}", child_pid));
        Ok(child_pid)
    } else {
        let msg = format!("CreateProcessAsUserW FAILED: error {}", err);
        log_to_file(LOG_SVC_PATH, &msg);
        Err(msg)
    }
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut windows_sys::core::PWSTR) {
    let svc_name_w = to_wstring(SERVICE_NAME);
    let status_handle = RegisterServiceCtrlHandlerW(svc_name_w.as_ptr(), Some(service_ctrl_handler));
    if status_handle.is_null() {
        log_to_file(LOG_SVC_PATH, "RegisterServiceCtrlHandlerW failed");
        ExitProcess(1);
    }
    G_STATUS_HANDLE.store(status_handle as isize, Ordering::SeqCst);

    let mut status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: SERVICE_START_PENDING,
        dwControlsAccepted: 0,
        dwWin32ExitCode: 0,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 1,
        dwWaitHint: 5000,
    };
    SetServiceStatus(status_handle, &status);

    status.dwCurrentState = SERVICE_RUNNING;
    status.dwControlsAccepted = SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN;
    SetServiceStatus(status_handle, &status);

    let mut session_id = G_TARGET_SESSION_ID.load(Ordering::SeqCst);
    if session_id == 0xFFFFFFFF {
        session_id = WTSGetActiveConsoleSessionId();
    }

    let result = launch_agent_into_session(session_id);

    status.dwCurrentState = SERVICE_STOPPED;
    status.dwControlsAccepted = 0;
    if result.is_ok() {
        status.dwWin32ExitCode = 0;
        status.dwServiceSpecificExitCode = 0;
    } else {
        status.dwWin32ExitCode = 1066; // ERROR_SERVICE_SPECIFIC_ERROR
        status.dwServiceSpecificExitCode = 1;
    }
    SetServiceStatus(status_handle, &status);

    ExitProcess(if result.is_ok() { 0 } else { 1 });
}

/// Entry point when binary is invoked with --secure-svc.
pub fn run_secure_service() -> Result<(), Box<dyn std::error::Error>> {
    log_to_file(LOG_SVC_PATH, "=== AeroStream Secure Service Starting ===");

    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--secure-svc") {
        if let Some(val) = args.get(pos + 1) {
            if let Ok(sess) = val.parse::<u32>() {
                G_TARGET_SESSION_ID.store(sess, Ordering::SeqCst);
                log_to_file(LOG_SVC_PATH, &format!("Target session ID configured: {}", sess));
            }
        }
    }

    unsafe {
        let mut svc_name_w = to_wstring(SERVICE_NAME);
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: svc_name_w.as_mut_ptr(),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: null_mut(),
                lpServiceProc: None,
            },
        ];

        if StartServiceCtrlDispatcherW(table.as_ptr()) == 0 {
            let err = GetLastError();
            log_to_file(LOG_SVC_PATH, &format!("StartServiceCtrlDispatcherW failed: {}", err));
            return Err(format!("StartServiceCtrlDispatcherW failed: {}", err).into());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Role 2: Secure Agent Worker (--secure-agent)
// ---------------------------------------------------------------------------

/// GDI capture worker loop: captures active desktop to DIBSection and encodes to JPEG.
fn run_agent_gdi_capture(running: Arc<AtomicBool>) {
    log_to_file(LOG_AGENT_PATH, "GDI Capture worker thread started");

    let mut desk_mgr = DesktopManager::new();
    let mut hdc_screen: HDC = null_mut();
    let mut hdc_mem: HDC = null_mut();
    let mut hbm: HBITMAP = null_mut();
    let mut old_obj: HGDIOBJ = null_mut();
    let mut ppv_bits: *mut u8 = null_mut();
    let mut cur_w = 0u32;
    let mut cur_h = 0u32;
    let mut jpeg_buf = Vec::with_capacity(1920 * 1080 / 2);

    while running.load(Ordering::Relaxed) {
        let loop_start = Instant::now();
        unsafe {
            let (desk_changed, desk_name) = desk_mgr.sync();

            let src_w = GetSystemMetrics(SM_CXSCREEN) as u32;
            let src_h = GetSystemMetrics(SM_CYSCREEN) as u32;

            if src_w > 0 && src_h > 0 {
                if desk_changed || hdc_mem.is_null() || cur_w != src_w || cur_h != src_h {
                    if !hdc_mem.is_null() {
                        SelectObject(hdc_mem, old_obj);
                        DeleteObject(hbm);
                        DeleteDC(hdc_mem);
                        ReleaseDC(null_mut(), hdc_screen);
                    }

                    hdc_screen = GetDC(null_mut());
                    hdc_mem = CreateCompatibleDC(hdc_screen);
                    SetStretchBltMode(hdc_mem, COLORONCOLOR);

                    let bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: src_w as i32,
                            biHeight: -(src_h as i32),
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

                    hbm = CreateDIBSection(
                        hdc_mem,
                        &bmi,
                        DIB_RGB_COLORS,
                        &mut ppv_bits as *mut _ as *mut _,
                        null_mut(),
                        0,
                    );
                    old_obj = SelectObject(hdc_mem, hbm);
                    cur_w = src_w;
                    cur_h = src_h;
                    log_to_file(
                        LOG_AGENT_PATH,
                        &format!("[CAPTURE] Recreated GDI DIBSection: {}x{} on desktop '{}'", src_w, src_h, desk_name),
                    );
                }

                let blit_ok = BitBlt(
                    hdc_mem,
                    0,
                    0,
                    src_w as i32,
                    src_h as i32,
                    hdc_screen,
                    0,
                    0,
                    SRCCOPY,
                ) != 0;

                if blit_ok && !ppv_bits.is_null() {
                    let len = (src_w * src_h * 4) as usize;
                    let slice = std::slice::from_raw_parts(ppv_bits, len);

                    jpeg_buf.clear();
                    let encoder = Encoder::new(&mut jpeg_buf, 75);
                    if encoder.encode(slice, src_w as u16, src_h as u16, ColorType::Bgra).is_ok() {
                        if let Ok(mut f) = OpenOptions::new().create(true).write(true).truncate(true).open(SECURE_FRAME_TMP_PATH) {
                            if f.write_all(&jpeg_buf).is_ok() {
                                drop(f);
                                allow_everyone_access(SECURE_FRAME_TMP_PATH);
                                let _ = std::fs::rename(SECURE_FRAME_TMP_PATH, SECURE_FRAME_PATH);
                                allow_everyone_access(SECURE_FRAME_PATH);
                                let repo_frame = r"D:\StreamApp\target\aerostream-secure-frame.jpg";
                                let _ = std::fs::copy(SECURE_FRAME_PATH, repo_frame);
                                allow_everyone_access(repo_frame);
                            }
                        }
                    }
                } else {
                    if !hdc_mem.is_null() {
                        SelectObject(hdc_mem, old_obj);
                        DeleteObject(hbm);
                        DeleteDC(hdc_mem);
                        ReleaseDC(null_mut(), hdc_screen);
                        hdc_mem = null_mut();
                        hdc_screen = null_mut();
                        hbm = null_mut();
                        ppv_bits = null_mut();
                    }
                    cur_w = 0;
                    cur_h = 0;
                }
            }
        }

        let elapsed = loop_start.elapsed();
        let target_interval = Duration::from_millis(350);
        if elapsed < target_interval {
            std::thread::sleep(target_interval - elapsed);
        }
    }

    unsafe {
        if !hdc_mem.is_null() {
            SelectObject(hdc_mem, old_obj);
            DeleteObject(hbm);
            DeleteDC(hdc_mem);
            ReleaseDC(null_mut(), hdc_screen);
        }
    }
    log_to_file(LOG_AGENT_PATH, "GDI Capture worker thread exiting");
}

/// Named Pipe server thread listening on \\.\pipe\aerostream-secure-agent for line-delimited JSON InputMessage.
fn run_agent_pipe_server(running: Arc<AtomicBool>) {
    log_to_file(LOG_AGENT_PATH, "Named Pipe server thread started");

    let mut desk_mgr = DesktopManager::new();
    let pipe_name_w = to_wstring(AGENT_PIPE_NAME);

    // Prepare NULL DACL Security Attributes so any local process can connect
    let mut sd: SECURITY_DESCRIPTOR = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeSecurityDescriptor(&mut sd as *mut _ as *mut _, 1);
        SetSecurityDescriptorDacl(&mut sd as *mut _ as *mut _, 1, null_mut(), 0);
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: &mut sd as *mut _ as *mut _,
        bInheritHandle: 0,
    };

    while running.load(Ordering::Relaxed) {
        let h_pipe = unsafe {
            CreateNamedPipeW(
                pipe_name_w.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                65536,
                65536,
                0,
                &sa,
            )
        };

        if h_pipe == INVALID_HANDLE_VALUE {
            let err = unsafe { GetLastError() };
            log_to_file(LOG_AGENT_PATH, &format!("[PIPE] CreateNamedPipeW failed: error {}", err));
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }

        log_to_file(LOG_AGENT_PATH, "[PIPE] Waiting for client connection on \\\\.\\pipe\\aerostream-secure-agent...");
        let connected = unsafe {
            ConnectNamedPipe(h_pipe, null_mut()) != 0 || GetLastError() == 535 /* ERROR_PIPE_CONNECTED */
        };

        if !connected {
            unsafe { CloseHandle(h_pipe); }
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }

        log_to_file(LOG_AGENT_PATH, "[PIPE] Client connected! Ready for JSON InputMessage stream");

        let mut read_buf = [0u8; 4096];
        let mut line_buf = String::new();

        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            let mut bytes_read = 0u32;
            let read_ok = unsafe {
                ReadFile(
                    h_pipe,
                    read_buf.as_mut_ptr(),
                    read_buf.len() as u32,
                    &mut bytes_read,
                    null_mut(),
                )
            };

            if read_ok == 0 || bytes_read == 0 {
                log_to_file(LOG_AGENT_PATH, "[PIPE] Client disconnected or pipe closed");
                break;
            }

            let chunk = String::from_utf8_lossy(&read_buf[..bytes_read as usize]);
            line_buf.push_str(&chunk);

            while let Some(pos) = line_buf.find('\n') {
                let line = line_buf[..pos].trim().to_string();
                line_buf = line_buf[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                match serde_json::from_str::<InputMessage>(&line) {
                    Ok(msg) => {
                        unsafe {
                            let (_, active_desk) = desk_mgr.sync();
                            let sw = GetSystemMetrics(SM_CXSCREEN) as u32;
                            let sh = GetSystemMetrics(SM_CYSCREEN) as u32;
                            log_to_file(
                                LOG_AGENT_PATH,
                                &format!("[PIPE-INPUT] Dispatching on desktop '{}' ({}x{}): {:?}", active_desk, sw, sh, msg),
                            );
                            dispatch_secure_input(msg, sw, sh);
                        }
                    }
                    Err(e) => {
                        log_to_file(
                            LOG_AGENT_PATH,
                            &format!("[PIPE-INPUT] Malformed JSON: line='{}', err={}", line, e),
                        );
                    }
                }
            }
        }

        unsafe {
            DisconnectNamedPipe(h_pipe);
            CloseHandle(h_pipe);
        }
    }
    log_to_file(LOG_AGENT_PATH, "Named Pipe server thread exiting");
}

// ---------------------------------------------------------------------------
// Native SendInput Dispatchers (Pure SYSTEM Token, Zero Impersonation/DACL Hack)
// ---------------------------------------------------------------------------

unsafe fn send_secure_char(ch: char) {
    if ch == '\n' || ch == '\r' {
        send_secure_key_click(VK_RETURN as u8, false);
        return;
    }
    if ch == '\t' {
        send_secure_key_click(VK_TAB as u8, false);
        return;
    }
    if ch == '\x08' {
        send_secure_key_click(VK_BACK as u8, false);
        return;
    }

    let mut buf = [0u16; 2];
    let utf16 = ch.encode_utf16(&mut buf);
    for &code_unit in utf16.iter() {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: code_unit,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: code_unit,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        SendInput(inputs.len() as u32, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(Duration::from_millis(30));
    }
}

unsafe fn send_secure_key_down(vk: u8, is_extended: bool) {
    let scan = MapVirtualKeyW(vk as u32, 0) as u16;
    let mut flags = 0u32;
    if is_extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let mut input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk as u16,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
}

unsafe fn send_secure_key_up(vk: u8, is_extended: bool) {
    let scan = MapVirtualKeyW(vk as u32, 0) as u16;
    let mut flags = KEYEVENTF_KEYUP;
    if is_extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let mut input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk as u16,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
}

unsafe fn send_secure_key_click(vk: u8, is_extended: bool) {
    send_secure_key_down(vk, is_extended);
    std::thread::sleep(Duration::from_millis(20));
    send_secure_key_up(vk, is_extended);
}

unsafe fn send_secure_mouse_event(flags: u32, dx: i32, dy: i32, data: i32) {
    let mut input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
}

unsafe fn secure_wake_lock_screen(screen_w: u32, screen_h: u32) {
    log_to_file(LOG_AGENT_PATH, "Waking Windows lock screen via trigger_sas and SendInput wake gestures");
    let _ = crate::input::trigger_sas();
    std::thread::sleep(Duration::from_millis(150));

    // Mouse swipe up
    let mid_x = (screen_w / 2) as i32;
    let start_y = (screen_h * 8 / 10) as i32;
    let end_y = (screen_h * 2 / 10) as i32;
    SetCursorPos(mid_x, start_y);
    send_secure_mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0);
    std::thread::sleep(Duration::from_millis(20));
    let steps = 8;
    for i in 1..=steps {
        let cur_y = start_y + ((end_y - start_y) * i / steps);
        SetCursorPos(mid_x, cur_y);
        std::thread::sleep(Duration::from_millis(10));
    }
    send_secure_mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0);
    std::thread::sleep(Duration::from_millis(100));

    // Space and Enter
    send_secure_key_click(VK_SPACE as u8, false);
    std::thread::sleep(Duration::from_millis(100));
    send_secure_key_click(VK_RETURN as u8, false);
    std::thread::sleep(Duration::from_millis(200));
}

/// Dispatches InputMessage directly from the SYSTEM worker into the active desktop.
unsafe fn dispatch_secure_input(msg: InputMessage, screen_w: u32, screen_h: u32) {
    let screen_w = screen_w.max(1);
    let screen_h = screen_h.max(1);

    match msg {
        InputMessage::MouseMove { x, y } => {
            let abs_x = (x * screen_w as f32).round() as i32;
            let abs_y = (y * screen_h as f32).round() as i32;
            SetCursorPos(abs_x, abs_y);
        }
        InputMessage::MouseDelta { dx, dy } => {
            send_secure_mouse_event(MOUSEEVENTF_MOVE, dx, dy, 0);
        }
        InputMessage::MouseDown { button, x, y } => {
            if let (Some(px), Some(py)) = (x, y) {
                let abs_x = (px * screen_w as f32).round() as i32;
                let abs_y = (py * screen_h as f32).round() as i32;
                SetCursorPos(abs_x, abs_y);
            }
            let flag = match button.to_lowercase().as_str() {
                "right" => MOUSEEVENTF_RIGHTDOWN,
                "middle" => MOUSEEVENTF_MIDDLEDOWN,
                _ => MOUSEEVENTF_LEFTDOWN,
            };
            send_secure_mouse_event(flag, 0, 0, 0);
        }
        InputMessage::MouseUp { button, x, y } => {
            if let (Some(px), Some(py)) = (x, y) {
                let abs_x = (px * screen_w as f32).round() as i32;
                let abs_y = (py * screen_h as f32).round() as i32;
                SetCursorPos(abs_x, abs_y);
            }
            let flag = match button.to_lowercase().as_str() {
                "right" => MOUSEEVENTF_RIGHTUP,
                "middle" => MOUSEEVENTF_MIDDLEUP,
                _ => MOUSEEVENTF_LEFTUP,
            };
            send_secure_mouse_event(flag, 0, 0, 0);
        }
        InputMessage::MouseClick { button, x, y } => {
            if let (Some(px), Some(py)) = (x, y) {
                let abs_x = (px * screen_w as f32).round() as i32;
                let abs_y = (py * screen_h as f32).round() as i32;
                SetCursorPos(abs_x, abs_y);
            }
            let (down, up) = match button.to_lowercase().as_str() {
                "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            };
            send_secure_mouse_event(down, 0, 0, 0);
            send_secure_mouse_event(up, 0, 0, 0);
        }
        InputMessage::MouseDouble { button, x, y } => {
            if let (Some(px), Some(py)) = (x, y) {
                let abs_x = (px * screen_w as f32).round() as i32;
                let abs_y = (py * screen_h as f32).round() as i32;
                SetCursorPos(abs_x, abs_y);
            }
            let (down, up) = match button.to_lowercase().as_str() {
                "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            };
            send_secure_mouse_event(down, 0, 0, 0);
            send_secure_mouse_event(up, 0, 0, 0);
            std::thread::sleep(Duration::from_millis(25));
            send_secure_mouse_event(down, 0, 0, 0);
            send_secure_mouse_event(up, 0, 0, 0);
        }
        InputMessage::MouseWheel { delta_x: _, delta_y } => {
            let wheel_delta = (delta_y * 120) as i32;
            send_secure_mouse_event(MOUSEEVENTF_WHEEL, 0, 0, wheel_delta);
        }
        InputMessage::KeyDown { key } => {
            if let Some((vk, ext)) = crate::input::key_name_to_vk(&key) {
                send_secure_key_down(vk, ext);
            } else if key.chars().count() == 1 {
                send_secure_char(key.chars().next().unwrap());
            }
        }
        InputMessage::KeyUp { key } => {
            if let Some((vk, ext)) = crate::input::key_name_to_vk(&key) {
                send_secure_key_up(vk, ext);
            }
        }
        InputMessage::KeyClick { key } => {
            if let Some((vk, ext)) = crate::input::key_name_to_vk(&key) {
                send_secure_key_click(vk, ext);
            } else if key.chars().count() == 1 {
                send_secure_char(key.chars().next().unwrap());
            }
        }
        InputMessage::Shortcut { keys } => {
            if crate::input::is_cad(&keys) {
                log_to_file(LOG_AGENT_PATH, "CAD shortcut requested -> trigger_sas()");
                let _ = crate::input::trigger_sas();
                send_secure_key_click(VK_SPACE as u8, false);
                send_secure_key_click(VK_RETURN as u8, false);
            } else {
                let mut parsed = Vec::new();
                for k in &keys {
                    if let Some(vk_info) = crate::input::key_name_to_vk(k) {
                        parsed.push(vk_info);
                    }
                }
                for &(vk, ext) in &parsed {
                    send_secure_key_down(vk, ext);
                }
                std::thread::sleep(Duration::from_millis(15));
                for &(vk, ext) in parsed.iter().rev() {
                    send_secure_key_up(vk, ext);
                }
            }
        }
        InputMessage::Text { text } => {
            log_to_file(LOG_AGENT_PATH, &format!("Injecting secure SendInput text: {} chars", text.len()));
            for ch in text.chars() {
                send_secure_char(ch);
            }
        }
        InputMessage::WakeLockScreen => {
            secure_wake_lock_screen(screen_w, screen_h);
        }
        _ => {}
    }
}

/// Entry point when binary is invoked with --secure-agent.
pub fn run_secure_agent() -> Result<(), Box<dyn std::error::Error>> {
    let pid = std::process::id();
    let session_id = get_current_session_id();
    let username = get_current_username();

    log_to_file(LOG_AGENT_PATH, &format!("=== AeroStream Secure Agent Worker Started (PID={}, Session={}, User={}) ===", pid, session_id, username));

    let desktop_name = unsafe {
        let cur_desk = GetThreadDesktop(GetCurrentThreadId());
        get_desktop_name(cur_desk)
    };

    let proof_content = format!(
        "=== AeroStream Secure Agent Proof ===\n\
        Timestamp: {}\n\
        PID: {}\n\
        Session ID: {}\n\
        User: {}\n\
        Desktop: {}\n\
        Status: SUCCESS - Running as SYSTEM worker in user session (M-SA.2)\n",
        chrono_like_now(),
        pid,
        session_id,
        username,
        desktop_name,
    );

    // 1. Write proof file to C:\Windows\Temp\aerostream-agent-proof.txt
    if let Ok(mut f) = OpenOptions::new().create(true).write(true).truncate(true).open(PROOF_FILE_PATH) {
        let _ = f.write_all(proof_content.as_bytes());
        allow_everyone_access(PROOF_FILE_PATH);
    }

    log_to_file(LOG_AGENT_PATH, &format!("Agent initialized: PID={}, Session={}, User={}, Desk={}", pid, session_id, username, desktop_name));

    let running = Arc::new(AtomicBool::new(true));

    // 2. Spawn GDI Capture worker thread
    let running_cap = running.clone();
    let cap_handle = std::thread::Builder::new()
        .name("secure-agent-capture".into())
        .spawn(move || {
            run_agent_gdi_capture(running_cap);
        })?;

    // 3. Spawn Named Pipe Server worker thread
    let running_pipe = running.clone();
    let pipe_handle = std::thread::Builder::new()
        .name("secure-agent-pipe".into())
        .spawn(move || {
            run_agent_pipe_server(running_pipe);
        })?;

    // 4. Main agent loop: monitor desktop transitions
    let mut desk_mgr = DesktopManager::new();
    while running.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(300));
        unsafe {
            let _ = desk_mgr.sync();
        }
    }

    let _ = cap_handle.join();
    let _ = pipe_handle.join();
    log_to_file(LOG_AGENT_PATH, "Agent exiting cleanly");
    Ok(())
}

// ---------------------------------------------------------------------------
// Role 3: Engine Client (spawn_secure_agent)
// ---------------------------------------------------------------------------

/// Spawns the secure agent into the target session using a transient Windows Service.
/// Cleans up the service immediately after spawning.
pub fn spawn_secure_agent() -> Result<u32, String> {
    // 1. Elevation check
    if !is_process_elevated() {
        let msg = "[SECURE-AGENT] REJECTED: Current process is NOT running as elevated Administrator. Cannot create or start services without administrative privileges.";
        crate::debug_log::log_gui(msg);
        tracing::warn!("{}", msg);
        eprintln!("{}", msg);
        return Err("Current process is not elevated. Run as Administrator.".to_string());
    }

    let target_session_id = get_current_session_id();
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;

    unsafe {
        // 2. Connect to SCM
        let h_scm = OpenSCManagerW(null(), null(), SC_MANAGER_ALL_ACCESS);
        if h_scm.is_null() {
            let err = GetLastError();
            let msg = format!("[SECURE-AGENT] OpenSCManagerW failed: err={}", err);
            crate::debug_log::log_gui(&msg);
            tracing::error!("{}", msg);
            return Err(msg);
        }

        let svc_name_w = to_wstring(SERVICE_NAME);
        let disp_name_w = to_wstring(SERVICE_DISPLAY_NAME);

        // 3. Check and clean up previous service if leftover
        let h_existing = OpenServiceW(h_scm, svc_name_w.as_ptr(), SERVICE_ALL_ACCESS);
        if !h_existing.is_null() {
            DeleteService(h_existing);
            CloseServiceHandle(h_existing);
            std::thread::sleep(Duration::from_millis(200));
        }

        // 4. Create transient service
        let bin_path = format!("\"{}\" --secure-svc {}", current_exe.display(), target_session_id);
        let bin_path_w = to_wstring(&bin_path);

        let h_service = CreateServiceW(
            h_scm,
            svc_name_w.as_ptr(),
            disp_name_w.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_DEMAND_START,
            SERVICE_ERROR_NORMAL,
            bin_path_w.as_ptr(),
            null(),
            null_mut(),
            null(),
            null(),
            null(),
        );

        if h_service.is_null() {
            let err = GetLastError();
            CloseServiceHandle(h_scm);
            let msg = format!("[SECURE-AGENT] CreateServiceW failed: err={}", err);
            crate::debug_log::log_gui(&msg);
            tracing::error!("{}", msg);
            return Err(msg);
        }

        let msg_created = format!("[SECURE-AGENT] Service created: {}", SERVICE_NAME);
        crate::debug_log::log_gui(&msg_created);
        tracing::info!("{}", msg_created);

        // 5. Start transient service
        if StartServiceW(h_service, 0, null()) == 0 {
            let err = GetLastError();
            DeleteService(h_service);
            CloseServiceHandle(h_service);
            CloseServiceHandle(h_scm);
            let msg = format!("[SECURE-AGENT] StartServiceW failed: err={}", err);
            crate::debug_log::log_gui(&msg);
            tracing::error!("{}", msg);
            return Err(msg);
        }

        let msg_started = "[SECURE-AGENT] Service started, awaiting spawn completion...";
        crate::debug_log::log_gui(msg_started);
        tracing::info!("{}", msg_started);

        // 6. Wait for service to stop (timeout: 10s)
        let mut service_status: SERVICE_STATUS = std::mem::zeroed();
        let start_wait = std::time::Instant::now();
        let mut stopped = false;

        while start_wait.elapsed() < Duration::from_secs(10) {
            if QueryServiceStatus(h_service, &mut service_status) != 0 {
                if service_status.dwCurrentState == SERVICE_STOPPED {
                    stopped = true;
                    let msg_stopped = format!(
                        "[SECURE-AGENT] Service stopped with exit code: win32={}, specific={}",
                        service_status.dwWin32ExitCode,
                        service_status.dwServiceSpecificExitCode
                    );
                    crate::debug_log::log_gui(&msg_stopped);
                    tracing::info!("{}", msg_stopped);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // 7. Delete transient service
        let del_res = DeleteService(h_service);
        CloseServiceHandle(h_service);
        CloseServiceHandle(h_scm);

        if del_res != 0 {
            let msg_del = "[SECURE-AGENT] Service deleted successfully";
            crate::debug_log::log_gui(msg_del);
            tracing::info!("{}", msg_del);
        } else {
            let msg_del_err = format!("[SECURE-AGENT] DeleteService returned: err={}", GetLastError());
            crate::debug_log::log_gui(&msg_del_err);
            tracing::warn!("{}", msg_del_err);
        }

        // 8. Verify proof file or successful exit code
        let proof_path = std::path::Path::new(PROOF_FILE_PATH);
        let mut proof_exists = false;
        for _ in 0..30 {
            if proof_path.exists() {
                proof_exists = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        if proof_exists {
            let msg_ok = format!("[SECURE-AGENT] Secure agent spawned successfully into session {}! Proof verified.", target_session_id);
            crate::debug_log::log_gui(&msg_ok);
            tracing::info!("{}", msg_ok);
            println!("{}", msg_ok);
            Ok(target_session_id)
        } else if stopped && service_status.dwWin32ExitCode == 0 {
            let msg_ok = format!("[SECURE-AGENT] Service reported success for session {}", target_session_id);
            crate::debug_log::log_gui(&msg_ok);
            tracing::info!("{}", msg_ok);
            println!("{}", msg_ok);
            Ok(target_session_id)
        } else {
            let msg_fail = "[SECURE-AGENT] Failed: Service timed out or secure agent proof was not detected".to_string();
            crate::debug_log::log_gui(&msg_fail);
            tracing::error!("{}", msg_fail);
            Err(msg_fail)
        }
    }
}

/// CLI runner for `--spawn-secure-agent`.
pub fn run_spawn_cli() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== AeroStream Secure Agent Spawn Trigger ===");
    if !is_process_elevated() {
        eprintln!("[SECURE-AGENT] REJECTED: Current process is NOT running as elevated Administrator.");
        eprintln!("[SECURE-AGENT] Please run PowerShell or Command Prompt as Administrator to spawn secure agent.");
        std::process::exit(1);
    }
    println!("[SECURE-AGENT] Privilege Level: Elevated Administrator");
    match spawn_secure_agent() {
        Ok(session_id) => {
            println!("[SECURE-AGENT] SUCCESS: Secure agent spawned into session {} and service deleted cleanly!", session_id);
            Ok(())
        }
        Err(e) => {
            eprintln!("[SECURE-AGENT] ERROR: {}", e);
            std::process::exit(1);
        }
    }
}
