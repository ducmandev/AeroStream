use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::info;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Cryptography::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::RemoteDesktop::*;

const CREDS_FILE_PATH: &str = "data/aerostream-session-creds.bin";
const SESSION_PIPE_NAME: &str = r"\\.\pipe\aerostream-session-ipc";
const RDP_FILE_PATH: &str = "data/session.rdp";

const PACKET_TYPE_FRAME: u8 = 0x01;
#[allow(dead_code)]
const PACKET_TYPE_AUDIO: u8 = 0x02;
const PACKET_TYPE_INPUT: u8 = 0x10;

fn to_wstring(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn from_wstring(slice: &[u16]) -> String {
    let len = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf16_lossy(&slice[..len])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCapability {
    pub supported: bool,
    pub os_edition: String,
    pub os_supported: bool,
    pub rdp_enabled: bool,
    pub creds_configured: bool,
    pub configured_username: Option<String>,
    pub current_console_user: String,
    pub is_current_user: bool,
    pub reason: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfigPayload {
    pub username: String,
    pub password: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Idle,
    Starting,
    Active,
    Disconnected,
    Stopping,
}

#[allow(dead_code)]
pub struct SessionBroker {
    pub current_session_id: Arc<AtomicU32>,
    pub is_active: Arc<AtomicBool>,
    pub state: Arc<Mutex<SessionState>>,
    pub session_frame_sender: broadcast::Sender<Bytes>,
    pub session_audio_sender: broadcast::Sender<Bytes>,
    pub session_input_tx: mpsc::Sender<crate::input::InputMessage>,
    session_input_rx: Arc<Mutex<Option<mpsc::Receiver<crate::input::InputMessage>>>>,
    pub session_client_count: Arc<AtomicU32>,
}

impl SessionBroker {
    pub fn new() -> Self {
        let (session_frame_sender, _) = broadcast::channel(4);
        let (session_audio_sender, _) = broadcast::channel(32);
        let (session_input_tx, session_input_rx) = mpsc::channel(64);

        Self {
            current_session_id: Arc::new(AtomicU32::new(0)),
            is_active: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(SessionState::Idle)),
            session_frame_sender,
            session_audio_sender,
            session_input_tx,
            session_input_rx: Arc::new(Mutex::new(Some(session_input_rx))),
            session_client_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Probe OS Edition, RDP configuration, and stored credentials
    pub fn probe_capabilities(&self) -> SessionCapability {
        let (os_supported, os_edition) = Self::probe_os_edition();
        let rdp_enabled = Self::probe_rdp_enabled();
        let (creds_configured, configured_username) = Self::probe_credentials();
        let current_console_user = Self::get_current_console_user();
        let is_current_user = configured_username
            .as_ref()
            .map(|u| u.trim().eq_ignore_ascii_case(&current_console_user))
            .unwrap_or(false);

        let mut reason = None;
        let supported = if !os_supported {
            reason = Some(format!(
                "Windows edition '{}' does not support hosting RDP sessions (Pro/Enterprise/Server required)",
                os_edition
            ));
            false
        } else if !rdp_enabled {
            reason = Some("Remote Desktop host is disabled (Terminal Server listener inactive)".to_string());
            false
        } else if !creds_configured {
            reason = Some("Session user credentials not configured yet (Setup required)".to_string());
            false
        } else {
            true
        };

        SessionCapability {
            supported,
            os_edition,
            os_supported,
            rdp_enabled,
            creds_configured,
            configured_username,
            current_console_user,
            is_current_user,
            reason,
        }
    }

    fn probe_os_edition() -> (bool, String) {
        unsafe {
            let mut hkey: HKEY = null_mut();
            let subkey = to_wstring("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) == ERROR_SUCCESS {
                let val_name = to_wstring("ProductName");
                let mut buf = [0u16; 256];
                let mut buf_size = (buf.len() * 2) as u32;
                let mut val_type = 0u32;
                let res = RegQueryValueExW(
                    hkey,
                    val_name.as_ptr(),
                    null_mut(),
                    &mut val_type,
                    buf.as_mut_ptr() as *mut u8,
                    &mut buf_size,
                );
                RegCloseKey(hkey);

                if res == ERROR_SUCCESS {
                    let name = from_wstring(&buf);
                    let lower = name.to_lowercase();
                    let is_supported = lower.contains("pro")
                        || lower.contains("enterprise")
                        || lower.contains("server")
                        || lower.contains("education");
                    return (is_supported, name);
                }
            }
        }
        (true, "Windows 10 Pro (Assumed)".to_string())
    }

    fn probe_rdp_enabled() -> bool {
        unsafe {
            let mut hkey: HKEY = null_mut();
            let subkey = to_wstring("SYSTEM\\CurrentControlSet\\Control\\Terminal Server");
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) == ERROR_SUCCESS {
                let val_name = to_wstring("fDenyTSConnections");
                let mut val: u32 = 1;
                let mut val_size = std::mem::size_of::<u32>() as u32;
                let mut val_type = 0u32;
                let res = RegQueryValueExW(
                    hkey,
                    val_name.as_ptr(),
                    null_mut(),
                    &mut val_type,
                    &mut val as *mut u32 as *mut u8,
                    &mut val_size,
                );
                RegCloseKey(hkey);

                if res == ERROR_SUCCESS {
                    return val == 0;
                }
            }
        }
        false
    }

    fn probe_credentials() -> (bool, Option<String>) {
        let path = Path::new(CREDS_FILE_PATH);
        if !path.exists() {
            return (false, None);
        }
        match Self::load_credentials() {
            Ok((username, _)) => (true, Some(username)),
            Err(_) => (false, None),
        }
    }

    pub fn get_current_console_user() -> String {
        std::env::var("USERNAME").unwrap_or_else(|_| "crayn".to_string())
    }

    /// Save user credentials encrypted with DPAPI (supports both current console user and secondary user)
    pub fn save_credentials(username: &str, password: &str) -> Result<(), String> {
        let u = username.trim();
        if u.is_empty() {
            return Err("Username cannot be empty".to_string());
        }

        if let Some(parent) = Path::new(CREDS_FILE_PATH).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let combined = format!("{}:{}", u, password);
        let bytes = combined.as_bytes();

        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };

        let desc = to_wstring("AeroStream Session Credentials");
        let success = unsafe {
            CryptProtectData(
                &in_blob,
                desc.as_ptr(),
                null_mut(),
                null_mut(),
                null_mut(),
                0,
                &mut out_blob,
            )
        };

        if success == 0 {
            return Err(format!("CryptProtectData failed with error {}", unsafe { GetLastError() }));
        }

        let encrypted = unsafe {
            std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec()
        };
        unsafe {
            LocalFree(out_blob.pbData as _);
        }

        std::fs::write(CREDS_FILE_PATH, encrypted).map_err(|e| format!("Failed to write credentials file: {}", e))?;

        let current_user = Self::get_current_console_user();
        if u.eq_ignore_ascii_case(&current_user) {
            info!("[SessionBroker] Configured credentials for current Windows user '{}'. When connecting in Session Mode, host console will auto-lock (matching native Windows RDP).", u);
        } else {
            info!("[SessionBroker] Configured credentials for secondary Windows user '{}'. Session Mode will run in an isolated multi-user session.", u);
        }
        Ok(())
    }

    /// Load and decrypt credentials using DPAPI
    pub fn load_credentials() -> Result<(String, String), String> {
        let encrypted = std::fs::read(CREDS_FILE_PATH).map_err(|e| format!("Failed to read credentials: {}", e))?;
        if encrypted.is_empty() {
            return Err("Empty credentials file".to_string());
        }

        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: encrypted.len() as u32,
            pbData: encrypted.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };

        let success = unsafe {
            CryptUnprotectData(
                &in_blob,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                0,
                &mut out_blob,
            )
        };

        if success == 0 {
            return Err(format!("CryptUnprotectData failed with error {}", unsafe { GetLastError() }));
        }

        let decrypted = unsafe {
            std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec()
        };
        unsafe {
            LocalFree(out_blob.pbData as _);
        }

        let combined = String::from_utf8(decrypted).map_err(|e| format!("Corrupted credentials UTF-8: {}", e))?;
        let parts: Vec<&str> = combined.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err("Invalid credentials format".to_string());
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }

    /// Start isolated RDP Session & Named Pipe IPC bridge
    pub async fn ensure_session_started(&self) -> Result<(), String> {
        let mut state_guard = self.state.lock().await;
        if *state_guard == SessionState::Active && self.is_active.load(Ordering::Relaxed) {
            return Ok(());
        }

        info!("[SessionBroker] Initializing Session Mode lifecycle...");
        *state_guard = SessionState::Starting;

        // 1. Load secondary user credentials
        let (username, password) = match Self::load_credentials() {
            Ok(c) => c,
            Err(e) => {
                *state_guard = SessionState::Idle;
                return Err(format!("Cannot start Session Mode: {}", e));
            }
        };

        // 2. Open Named Pipe Server for Session Agent
        let server = match ServerOptions::new()
            .first_pipe_instance(true)
            .create(SESSION_PIPE_NAME)
        {
            Ok(s) => s,
            Err(e) => {
                *state_guard = SessionState::Idle;
                return Err(format!("Failed to create Named Pipe server: {:?}", e));
            }
        };

        // 3. Register credentials in Windows Credential Manager via cmdkey for both 127.0.0.2 and 127.0.0.1
        info!("[SessionBroker] Registering TERMSRV credentials for user '{}'", username);
        let user_arg = format!("/user:{}", username);
        let pass_arg = format!("/pass:{}", password);
        for target in &["TERMSRV/127.0.0.2", "TERMSRV/127.0.0.1", "TERMSRV/localhost"] {
            let _ = std::process::Command::new("cmdkey")
                .args(&[
                    *target,
                    user_arg.as_str(),
                    pass_arg.as_str(),
                ])
                .output();
        }

        // Auto-lock physical console if connecting as the current console user (matching native Windows RDP behavior)
        let current_user = Self::get_current_console_user();
        let is_current_user = username.trim().eq_ignore_ascii_case(&current_user);
        if is_current_user {
            info!("[SessionBroker] Connecting Session Mode for current user '{}' -> auto-locking physical console for privacy (Windows RDP style)...", username);
            unsafe {
                windows_sys::Win32::System::Shutdown::LockWorkStation();
            }
        }

        // 4. Generate .rdp file for mstsc connection using 127.0.0.2 (bypasses mstsc 127.0.0.1 loopback restriction)
        let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_str = exe_path.to_string_lossy();
        let exe_dir = exe_path.parent().unwrap_or(Path::new(".")).to_string_lossy();

        if let Some(parent) = Path::new(RDP_FILE_PATH).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let rdp_content = format!(
            "full address:s:127.0.0.2:3389\n\
             username:s:{username}\n\
             screen mode id:i:1\n\
             desktopwidth:i:1280\n\
             desktopheight:i:720\n\
             session bpp:i:32\n\
             alternate shell:s:{exe_str} --session-agent aerostream-session-ipc\n\
             shell working dir:s:{exe_dir}\n\
             prompt for credentials:i:0\n\
             authentication level:i:0\n\
             enablecredsspsupport:i:0\n"
        );
        let _ = std::fs::write(RDP_FILE_PATH, rdp_content);

        // 5. Spawn background mstsc process
        info!("[SessionBroker] Spawning background mstsc.exe to establish localhost RDP session...");
        let _mstsc_child = std::process::Command::new("mstsc.exe")
            .arg(RDP_FILE_PATH)
            .spawn()
            .map_err(|e| format!("Failed to spawn mstsc.exe: {:?}", e))?;

        // 6. Wait for Session Agent to connect over Named Pipe (timeout 15s)
        info!("[SessionBroker] Waiting for Session Agent IPC handshake...");
        let connected_server = match tokio::time::timeout(Duration::from_secs(15), server.connect()).await {
            Ok(Ok(())) => {
                info!("[SessionBroker] Session Agent successfully connected over Named Pipe!");
                server
            }
            Ok(Err(e)) => {
                *state_guard = SessionState::Idle;
                return Err(format!("Named Pipe connection error: {:?}", e));
            }
            Err(_) => {
                *state_guard = SessionState::Idle;
                return Err("Timeout (15s) waiting for Session Agent to launch in RDP session".to_string());
            }
        };

        // 7. Track session ID via WTS
        if let Some(sid) = Self::find_user_session_id(&username) {
            info!("[SessionBroker] Detected active Windows RDP session ID {} for user '{}'", sid, username);
            self.current_session_id.store(sid, Ordering::Relaxed);
        }

        // 8. Start Named Pipe Bridge Workers
        let (mut reader, mut writer) = tokio::io::split(connected_server);
        let frame_sender = self.session_frame_sender.clone();
        let is_active = Arc::clone(&self.is_active);
        is_active.store(true, Ordering::Relaxed);

        // Read frames from agent -> broadcast to session clients
        let active_flag_read = Arc::clone(&is_active);
        tokio::spawn(async move {
            let mut len_buf = [0u8; 4];
            while active_flag_read.load(Ordering::Relaxed) {
                if let Err(_) = reader.read_exact(&mut len_buf).await {
                    break;
                }
                let packet_len = u32::from_le_bytes(len_buf) as usize;
                if packet_len == 0 || packet_len > 10_000_000 {
                    break;
                }
                let mut data = vec![0u8; packet_len];
                if let Err(_) = reader.read_exact(&mut data).await {
                    break;
                }
                if data[0] == PACKET_TYPE_FRAME && data.len() > 1 {
                    let frame_bytes = Bytes::copy_from_slice(&data[1..]);
                    let _ = frame_sender.send(frame_bytes);
                }
            }
            active_flag_read.store(false, Ordering::Relaxed);
            info!("[SessionBroker] Pipe reader task finished.");
        });

        // Write input events from session clients -> agent
        let mut rx_guard = self.session_input_rx.lock().await;
        if let Some(mut input_rx) = rx_guard.take() {
            let active_flag_write = Arc::clone(&is_active);
            tokio::spawn(async move {
                while active_flag_write.load(Ordering::Relaxed) {
                    if let Some(input_msg) = input_rx.recv().await {
                        if let Ok(json_str) = serde_json::to_string(&input_msg) {
                            let json_bytes = json_str.as_bytes();
                            let total_len = (1 + json_bytes.len()) as u32;
                            if let Err(_) = writer.write_all(&total_len.to_le_bytes()).await {
                                break;
                            }
                            if let Err(_) = writer.write_all(&[PACKET_TYPE_INPUT]).await {
                                break;
                            }
                            if let Err(_) = writer.write_all(json_bytes).await {
                                break;
                            }
                        }
                    }
                }
            });
        }

        *state_guard = SessionState::Active;
        Ok(())
    }

    pub async fn client_connected(&self) {
        self.session_client_count.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn client_disconnected(&self) {
        let remaining = self.session_client_count.fetch_sub(1, Ordering::Relaxed) - 1;
        info!("[SessionBroker] Session client disconnected, remaining: {}", remaining);
        if remaining == 0 {
            self.teardown_session().await;
        }
    }

    /// Teardown isolated RDP session and clean up credentials
    pub async fn teardown_session(&self) {
        let mut state_guard = self.state.lock().await;
        info!("[SessionBroker] Tearing down Session Mode...");
        *state_guard = SessionState::Stopping;

        self.is_active.store(false, Ordering::Relaxed);
        let sid = self.current_session_id.swap(0, Ordering::Relaxed);
        if sid > 0 {
            let is_current = if let Ok((u, _)) = Self::load_credentials() {
                u.trim().eq_ignore_ascii_case(&Self::get_current_console_user())
            } else {
                false
            };

            if is_current {
                info!("[SessionBroker] Preserving user applications: disconnecting (not logging off) session ID {}", sid);
                Self::disconnect_session(sid);
            } else {
                Self::logoff_session(sid);
            }
        }

        // Clean up cmdkey targets
        for target in &["/delete:TERMSRV/127.0.0.2", "/delete:TERMSRV/127.0.0.1", "/delete:TERMSRV/localhost"] {
            let _ = std::process::Command::new("cmdkey").arg(target).output();
        }

        *state_guard = SessionState::Idle;
        info!("[SessionBroker] Session Mode cleanly terminated.");
    }

    /// Disconnect session cleanly (preserves all open applications and work)
    pub fn disconnect_session(session_id: u32) {
        if session_id > 0 {
            info!("[SessionBroker] Disconnecting Windows RDP session ID {} (preserving apps)", session_id);
            unsafe {
                WTSDisconnectSession(WTS_CURRENT_SERVER_HANDLE, session_id, 0);
            }
        }
    }

    /// Query active WTS Session ID for secondary user
    pub fn find_user_session_id(target_user: &str) -> Option<u32> {
        unsafe {
            let mut p_session_info: *mut WTS_SESSION_INFOW = null_mut();
            let mut count: u32 = 0;

            if WTSEnumerateSessionsW(
                WTS_CURRENT_SERVER_HANDLE,
                0,
                1,
                &mut p_session_info,
                &mut count,
            ) != 0
            {
                let sessions = std::slice::from_raw_parts(p_session_info, count as usize);
                for s in sessions {
                    let mut p_user_buffer: *mut u16 = null_mut();
                    let mut bytes_returned: u32 = 0;

                    if WTSQuerySessionInformationW(
                        WTS_CURRENT_SERVER_HANDLE,
                        s.SessionId,
                        WTSUserName,
                        &mut p_user_buffer as *mut _ as *mut _,
                        &mut bytes_returned,
                    ) != 0
                    {
                        let user_name = from_wstring(std::slice::from_raw_parts(
                            p_user_buffer,
                            (bytes_returned / 2) as usize,
                        ));
                        WTSFreeMemory(p_user_buffer as _);

                        if user_name.trim().eq_ignore_ascii_case(target_user) {
                            WTSFreeMemory(p_session_info as _);
                            return Some(s.SessionId);
                        }
                    }
                }
                WTSFreeMemory(p_session_info as _);
            }
        }
        None
    }

    /// Logoff session cleanly
    pub fn logoff_session(session_id: u32) {
        if session_id > 0 {
            info!("[SessionBroker] Logging off Windows RDP session ID {}", session_id);
            unsafe {
                WTSLogoffSession(WTS_CURRENT_SERVER_HANDLE, session_id, 0);
            }
        }
    }
}
