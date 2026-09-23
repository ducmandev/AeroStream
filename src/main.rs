#![windows_subsystem = "windows"]

mod audio;
mod capture;
mod capture_dxgi;
mod clock;
mod codec_h264;
mod codec_mf;
mod config;
mod debug_log;
mod gui;
mod input;
mod secure_agent;
mod server;
mod session_agent;
mod session_broker;
mod state;
mod webrtc_session;

use audio::AudioEngine;
use capture::CaptureEngine;
use codec_mf::MediaFoundationEncoder;
use config::AppConfig;
use debug_log::log_gui;
use input::InputManager;
use rand::Rng;
use state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::panic::set_hook(Box::new(|info| {
        crate::debug_log::log_gui(&format!("PANIC: {:?}", info));
    }));

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--secure-svc") {
        return crate::secure_agent::run_secure_service();
    }
    if args.iter().any(|a| a == "--secure-agent") {
        return crate::secure_agent::run_secure_agent();
    }
    if args.iter().any(|a| a == "--spawn-secure-agent") {
        return crate::secure_agent::run_spawn_cli();
    }

    if let Some(pos) = args.iter().position(|a| a == "--session-agent") {
        let pipe_name = args.get(pos + 1).cloned().unwrap_or_else(|| "aerostream-session-ipc".to_string());
        crate::clock::init_clock();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        return rt.block_on(crate::session_agent::run_session_agent(pipe_name));
    }

    let _ = rustls::crypto::ring::default_provider().install_default();
    crate::clock::init_clock();
    log_gui("main() started with QPC clock");

    // Initialize Tracing Subscriber to aerostream.log
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("aerostream.log")
    {
        use tracing_subscriber::util::SubscriberInitExt;
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .finish();
        subscriber.init();
    }
    tracing::info!("=== AeroStream Engine Starting ===");
    unsafe {
        use windows_sys::Win32::Security::*;
        use windows_sys::Win32::System::Threading::*;
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0 {
            let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
            let mut ret_len = 0;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut _ as *mut _,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            );
            windows_sys::Win32::Foundation::CloseHandle(token);
            if ok != 0 && elevation.TokenIsElevated != 0 {
                enable_token_privileges();
                tracing::info!("Privilege Level: Elevated Administrator (Full UIPI bypass & SeDebugPrivilege enabled for lock screen & elevated apps)");

                // Auto-spawn or verify SYSTEM Secure Agent worker for lock screen remote unlock
                std::thread::spawn(|| {
                    use windows_sys::Win32::Foundation::*;
                    use windows_sys::Win32::Storage::FileSystem::*;
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let pipe_name: Vec<u16> = crate::secure_agent::AGENT_PIPE_NAME
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                    unsafe {
                        let h = CreateFileW(
                            pipe_name.as_ptr(),
                            GENERIC_READ | GENERIC_WRITE,
                            0,
                            std::ptr::null_mut(),
                            OPEN_EXISTING,
                            0,
                            std::ptr::null_mut(),
                        );
                        if h != INVALID_HANDLE_VALUE {
                            CloseHandle(h);
                            tracing::info!("[SECURE-AGENT] Existing SYSTEM Secure Agent detected on named pipe.");
                            return;
                        }
                    }
                    tracing::info!("[SECURE-AGENT] No active Secure Agent detected. Auto-spawning SYSTEM worker into session...");
                    match crate::secure_agent::spawn_secure_agent() {
                        Ok(_) => tracing::info!("[SECURE-AGENT] Auto-spawned SYSTEM Secure Agent successfully!"),
                        Err(e) => tracing::warn!("[SECURE-AGENT] Auto-spawn Secure Agent failed: {}", e),
                    }
                });
            } else {
                tracing::info!("Privilege Level: Standard User (Run Unlock-UIPI.bat or Run as Administrator to bypass UIPI and unlock login screens)");
            }
        }
    }
    // 1. Set DPI Awareness & Detect Screen Resolution
    unsafe {
        SetProcessDPIAware();
    }
    let (screen_w, screen_h) = unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN) as u32;
        let height = GetSystemMetrics(SM_CYSCREEN) as u32;
        (width, height)
    };

    // 2. Generate Random 6-digit PIN
    let mut rng = rand::thread_rng();
    let pin: u32 = rng.gen_range(100_000..=999_999);
    let pin_str = pin.to_string();

    // 3. Setup Configuration & Test Port Availability
    let port = 8080;
    let test_addr = SocketAddr::from(([0, 0, 0, 0], port));
    let test_listener = match std::net::TcpListener::bind(test_addr) {
        Ok(l) => l,
        Err(e) => {
            let err_msg = format!(
                "Port {} is already in use by another instance or process: {:?}\nPlease close any existing instance of AeroStream before launching a new one.",
                port, e
            );
            crate::debug_log::log_gui(&format!("FATAL BIND ERROR: {}", err_msg));
            unsafe {
                let wide_title: Vec<u16> = "AeroStream - Port In Use\0".encode_utf16().collect();
                let wide_msg: Vec<u16> = format!("{}\0", err_msg).encode_utf16().collect();
                MessageBoxW(std::ptr::null_mut(), wide_msg.as_ptr(), wide_title.as_ptr(), MB_OK | MB_ICONERROR);
            }
            return Err(e.into());
        }
    };
    drop(test_listener);

    let config = AppConfig {
        port,
        pin: pin_str.clone(),
        fps: 60,
        quality: 80,
        scale: 1.0,
        monitor_index: 0,
    };

    // 4. Detect Local IP Address
    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let use_tls = std::env::args().any(|a| a == "--tls");
    let protocol = if use_tls { "https" } else { "http" };
    let client_url = format!("{}://{}:{}/?pin={}", protocol, local_ip, port, pin_str);
    log_gui(&format!("PIN: {} | URL: {} (TLS: {})", pin_str, client_url, use_tls));

    // 5. Initialize Input Manager & App State
    let input_manager = match InputManager::new(screen_w, screen_h) {
        Ok(im) => {
            log_gui("InputManager initialized successfully");
            im
        }
        Err(e) => {
            log_gui(&format!("ERROR: Failed to init input manager: {}", e));
            return Err(e.into());
        }
    };
    let app_state = AppState::new(config, input_manager, screen_w, screen_h);
    log_gui("AppState created");

    // 6. Start Screen Capture Thread (Hybrid DXGI / GDI + H.264 / JPEG)
    let capture_engine = Arc::new(CaptureEngine::new(
        app_state.stream_settings.clone(),
        app_state.frame_sender.clone(),
        app_state.h264_sender.clone(),
        app_state.cursor_sender.clone(),
        app_state.current_fps.clone(),
        app_state.latest_frame.clone(),
        app_state.latest_h264_keyframe.clone(),
        app_state.request_h264_keyframe.clone(),
    ));
    capture_engine.start();
    log_gui("CaptureEngine started");

    let audio_engine = Arc::new(AudioEngine::new(app_state.audio_sender.clone()));
    audio_engine.start();
    log_gui("AudioEngine started (WASAPI Loopback + Opus 48kHz)");

    // Phase 3 Hardware Encoder MFT initialization
    let _mf_encoder = MediaFoundationEncoder::new();
    if let Some(hw_name) = _mf_encoder.hardware_name() {
        log_gui(&format!("Media Foundation Hardware Encoder: {}", hw_name));
    }

    // 7. Spawn Tokio Runtime for HTTP/HTTPS & WebSocket Server in background thread
    let app_state_clone = app_state.clone();
    let local_ip_tls = local_ip.clone();
    std::thread::Builder::new()
        .name("tokio-server-worker".into())
        .spawn(move || {
            crate::debug_log::log_gui("tokio thread started");
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime");

            rt.block_on(async move {
                let router = server::create_router(app_state_clone);
                let addr = SocketAddr::from(([0, 0, 0, 0], port));

                if use_tls {
                    crate::debug_log::log_gui("Starting TLS/WSS mode with self-signed certificate...");
                    match generate_self_signed_tls(&local_ip_tls) {
                        Ok((cert_pem, key_pem)) => {
                            match axum_server::tls_rustls::RustlsConfig::from_pem(cert_pem, key_pem).await {
                                Ok(rustls_config) => {
                                    crate::debug_log::log_gui(&format!("HTTPS/WSS Server bound to https://0.0.0.0:{}", port));
                                    let _ = axum_server::bind_rustls(addr, rustls_config)
                                        .serve(router.into_make_service_with_connect_info::<SocketAddr>())
                                        .await;
                                }
                                Err(e) => {
                                    crate::debug_log::log_gui(&format!("ERROR: Failed to create RustlsConfig: {:?}", e));
                                }
                            }
                        }
                        Err(e) => {
                            crate::debug_log::log_gui(&format!("ERROR: Failed to generate self-signed TLS cert: {:?}", e));
                        }
                    }
                } else {
                    match tokio::net::TcpListener::bind(addr).await {
                        Ok(listener) => {
                            crate::debug_log::log_gui(&format!("HTTP/WS Server bound to http://0.0.0.0:{}", port));
                            let _ = axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await;
                        }
                        Err(e) => {
                            crate::debug_log::log_gui(&format!("ERROR: Failed to bind to port {}: {:?}", port, e));
                        }
                    }
                }
            });
        })
        .expect("Failed to spawn server thread");

    // 8. Run Native Windows GUI Window on main thread
    log_gui("Calling gui::run_gui");
    gui::run_gui(local_ip, port, pin_str, screen_w, screen_h);
    log_gui("gui::run_gui finished");

    Ok(())
}

fn generate_self_signed_tls(local_ip: &str) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
    // Determine candidate paths for cert and key
    let mut candidate_dirs = vec![std::env::current_dir().unwrap_or_default()];
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let parent_buf = parent.to_path_buf();
            if !candidate_dirs.contains(&parent_buf) {
                candidate_dirs.push(parent_buf);
            }
        }
    }

    // 1. Check if persistent TLS certificates already exist in candidate locations
    for dir in &candidate_dirs {
        let cert_p = dir.join("cert.pem");
        let key_p = dir.join("key.pem");
        if cert_p.exists() && key_p.exists() {
            if let (Ok(cert_pem), Ok(key_pem)) = (std::fs::read(&cert_p), std::fs::read(&key_p)) {
                if !cert_pem.is_empty() && !key_pem.is_empty() {
                    crate::debug_log::log_gui(&format!("Loaded existing persistent TLS cert from {} and {}", cert_p.display(), key_p.display()));
                    tracing::info!("Loaded existing persistent TLS certificate from {} and {}", cert_p.display(), key_p.display());
                    return Ok((cert_pem, key_pem));
                }
            }
        }
    }

    // 2. Generate new persistent certificate
    let subject_alt_names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        local_ip.to_string(),
    ];
    let cert = rcgen::generate_simple_self_signed(subject_alt_names)?;
    let cert_pem = cert.cert.pem().into_bytes();
    let key_pem = cert.key_pair.serialize_pem().into_bytes();

    let save_dir = candidate_dirs.first().cloned().unwrap_or_else(|| std::path::PathBuf::from("."));
    let cert_p = save_dir.join("cert.pem");
    let key_p = save_dir.join("key.pem");

    let _ = std::fs::write(&cert_p, &cert_pem);
    let _ = std::fs::write(&key_p, &key_pem);
    crate::debug_log::log_gui(&format!("Generated and saved persistent TLS cert to {} and {}", cert_p.display(), key_p.display()));
    tracing::info!("Generated and saved persistent self-signed TLS certificate to {} and {}", cert_p.display(), key_p.display());

    Ok((cert_pem, key_pem))
}

unsafe fn enable_token_privileges() {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::System::Threading::*;

    let mut token: HANDLE = std::ptr::null_mut();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) != 0 {
        for priv_name in &[
            "SeDebugPrivilege",
            "SeImpersonatePrivilege",
            "SeTcbPrivilege",
            "SeAssignPrimaryTokenPrivilege",
            "SeIncreaseQuotaPrivilege",
        ] {
            let wide: Vec<u16> = priv_name.encode_utf16().chain(std::iter::once(0)).collect();
            let mut luid: LUID = std::mem::zeroed();
            if LookupPrivilegeValueW(std::ptr::null_mut(), wide.as_ptr(), &mut luid) != 0 {
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
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
        CloseHandle(token);
    }
    ensure_sas_policy();
}

unsafe fn ensure_sas_policy() {
    use windows_sys::Win32::System::Registry::*;
    let mut h_key: HKEY = std::ptr::null_mut();
    let subkey: Vec<u16> = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System\0"
        .encode_utf16()
        .collect();
    if RegCreateKeyExW(
        HKEY_LOCAL_MACHINE,
        subkey.as_ptr(),
        0,
        std::ptr::null_mut(),
        0,
        KEY_SET_VALUE,
        std::ptr::null_mut(),
        &mut h_key,
        std::ptr::null_mut(),
    ) == 0 {
        let policies: [(&str, u32); 4] = [
            ("SoftwareSASGeneration\0", 3), // 3 = Services and Ease of Access applications (enables SendSAS)
            ("EnableSecureUIAPaths\0", 0),  // 0 = Allow UIAccess apps from any directory (not just Program Files)
            ("EnableUIADesktopToggle\0", 1), // 1 = Allow UIAccess apps to toggle desktop and interact with secure desktop
            // PromptOnSecureDesktop: keep the OS default (1). Earlier builds set 0, which lowers
            // UAC security system-wide and does NOT help lock-screen unlock (lock screen is not a UAC prompt).
            ("PromptOnSecureDesktop\0", 1),
        ];

        for (name, val) in policies {
            let val_name: Vec<u16> = name.encode_utf16().collect();
            let _ = RegSetValueExW(
                h_key,
                val_name.as_ptr(),
                0,
                REG_DWORD,
                &val as *const _ as *const u8,
                std::mem::size_of::<u32>() as u32,
            );
        }
        RegCloseKey(h_key);
        tracing::info!("Configured system SAS, UIPI, and SecureDesktop policies successfully");
    }
}
