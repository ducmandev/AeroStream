use crate::input::InputMessage;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use std::net::SocketAddr;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

const INDEX_HTML: &str = include_str!("web/index.html");
const STYLE_CSS: &str = include_str!("web/style.css");
const APP_JS: &str = include_str!("web/app.js");
const REVERSE_HTML: &str = include_str!("web/reverse.html");
const MOBILE_UI_HTML: &str = include_str!("web/mobile_ui_prototype.html");


pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/ui", get(handle_mobile_ui))
        .route("/prototype", get(handle_mobile_ui))
        .route("/style.css", get(handle_css))
        .route("/app.js", get(handle_js))
        .route("/app.apk", get(handle_download_apk))
        .route("/aerostream.apk", get(handle_download_apk))
        .route("/reverse", get(handle_reverse))
        .route("/api/status", get(handle_status))
        .route("/api/auth/pair", axum::routing::post(handle_auth_pair))
        .route("/api/session/config", axum::routing::post(handle_session_config))
        .route("/api/secure-agent/spawn", axum::routing::post(handle_secure_agent_spawn))
        .route("/ws", get(handle_ws_upgrade))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn handle_download_apk() -> impl IntoResponse {
    let apk_candidates = [
        "deploy/AeroStream-Android.apk",
        "deploy/android/AeroStream-Android.apk",
        "AeroStream-Android.apk",
        "android_app/build/app/outputs/flutter-apk/app-release.apk",
        "../deploy/AeroStream-Android.apk",
        "../AeroStream-Android.apk",
    ];
    for path in &apk_candidates {
        if let Ok(data) = std::fs::read(path) {
            return (
                [
                    (header::CONTENT_TYPE, "application/vnd.android.package-archive"),
                    (header::CONTENT_DISPOSITION, "attachment; filename=\"AeroStream.apk\""),
                ],
                data,
            ).into_response();
        }
    }
    (StatusCode::NOT_FOUND, "APK not found. Please build the Android app first.").into_response()
}

async fn handle_index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn handle_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STYLE_CSS)
}

async fn handle_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], APP_JS)
}

async fn handle_reverse() -> impl IntoResponse {
    Html(REVERSE_HTML)
}

async fn handle_mobile_ui() -> impl IntoResponse {
    Html(MOBILE_UI_HTML)
}

pub fn is_lan_or_local(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local()
        }
        std::net::IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
        }
    }
}

pub fn get_context_ice_servers(
    client_ip: &std::net::IpAddr,
    mode_override: Option<&str>,
) -> Vec<webrtc::ice_transport::ice_server::RTCIceServer> {
    let use_stun = match mode_override {
        Some("lan") => false,
        Some("stun") | Some("wan") | Some("internet") => true,
        _ => !is_lan_or_local(client_ip),
    };

    if !use_stun {
        return vec![];
    }

    let mut servers = vec![webrtc::ice_transport::ice_server::RTCIceServer {
        urls: vec![
            "stun:stun.l.google.com:19302".to_string(),
            "stun:stun1.l.google.com:19302".to_string(),
        ],
        ..Default::default()
    }];

    if let Ok(turn_url) = std::env::var("AEROSTREAM_TURN_URL") {
        let username = std::env::var("AEROSTREAM_TURN_USER").unwrap_or_default();
        let credential = std::env::var("AEROSTREAM_TURN_CRED").unwrap_or_default();
        servers.push(webrtc::ice_transport::ice_server::RTCIceServer {
            urls: vec![turn_url],
            username,
            credential,
        });
    }

    servers
}

async fn handle_status(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let client_ip = addr.ip();
    let is_lan = is_lan_or_local(&client_ip);
    let ice_servers = get_context_ice_servers(&client_ip, None);
    let ice_urls: Vec<String> = ice_servers.into_iter().flat_map(|s| s.urls).collect();
    let capability = state.session_broker.probe_capabilities();
    let modes = if capability.supported {
        vec!["console", "session"]
    } else {
        vec!["console"]
    };

    Json(json!({
        "status": "online",
        "fps": state.get_fps(),
        "active_clients": state.active_clients(),
        "uptime_secs": state.server_start_time.elapsed().as_secs(),
        "configured_fps": state.stream_settings.get_fps(),
        "configured_quality": state.stream_settings.get_quality(),
        "configured_height": state.stream_settings.get_target_height(),
        "pin": state.config.pin,
        "server_time": crate::clock::qpc_now_ms(),
        "is_lan": is_lan,
        "ice_servers": ice_urls,
        "modes": modes,
        "session_capability": capability,
    }))
}

#[derive(Deserialize)]
struct SessionConfigReq {
    pin: String,
    username: String,
    password: String,
}

async fn handle_session_config(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<SessionConfigReq>,
) -> Response {
    let client_ip = addr.ip();
    if payload.pin != state.config.pin {
        warn!("[AUDIT] Unauthorized session config attempt from {}", client_ip);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "status": "error",
                "message": "Invalid PIN code."
            })),
        )
            .into_response();
    }

    match crate::session_broker::SessionBroker::save_credentials(&payload.username, &payload.password) {
        Ok(()) => {
            info!("[AUDIT] Updated session credentials from {}", client_ip);
            Json(json!({
                "status": "ok",
                "message": "Session credentials encrypted and saved successfully."
            }))
            .into_response()
        }
        Err(e) => {
            warn!("[AUDIT] Failed to save session credentials: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "error",
                    "message": e
                })),
            )
                .into_response()
        }
    }
}

async fn handle_secure_agent_spawn() -> Response {
    match tokio::task::spawn_blocking(crate::secure_agent::spawn_secure_agent).await {
        Ok(Ok(session_id)) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "session_id": session_id,
                "message": "Secure agent spawned successfully as SYSTEM in session"
            })),
        )
            .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": e
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": format!("Task join error: {:?}", e)
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct PairRequest {
    pin: String,
    #[serde(default)]
    device: Option<String>,
}

async fn handle_auth_pair(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<PairRequest>,
) -> Response {
    let client_ip = addr.ip();
    if let Err(remaining) = state.check_pin_lockout(&client_ip) {
        warn!("[AUDIT] Pair request rejected for {} - IP locked out ({}s remaining)", client_ip, remaining);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "status": "error",
                "message": format!("Too many failed attempts. IP locked out for {} seconds.", remaining),
                "remaining_seconds": remaining
            })),
        )
            .into_response();
    }

    if payload.pin != state.config.pin {
        let attempts = state.record_pin_failure(client_ip);
        warn!(
            "[AUDIT] Failed pairing attempt from {} with invalid PIN (attempt {}/5)",
            client_ip, attempts
        );
        if attempts >= 5 {
            warn!(
                "[AUDIT] IP {} has been locked out for 60 seconds due to 5 consecutive failed PIN attempts",
                client_ip
            );
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "status": "error",
                    "message": "Too many failed attempts. IP locked out for 60 seconds."
                })),
            )
                .into_response();
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "status": "error",
                "message": "Invalid PIN code"
            })),
        )
            .into_response();
    }

    state.record_pin_success(&client_ip);
    let token = state.create_session_token(client_ip);
    let device_name = payload.device.unwrap_or_else(|| "Unknown device".to_string());
    info!(
        "[AUDIT] Successful pairing from {} ({}) -> issued session token",
        client_ip, device_name
    );

    Json(json!({
        "status": "ok",
        "token": token,
        "expires_in": 86400
    }))
    .into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    pin: Option<String>,
    token: Option<String>,
    role: Option<String>,
    codec: Option<String>,
    ice: Option<String>,
    mode: Option<String>,
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<WsQuery>,
    State(state): State<AppState>,
) -> Response {
    let client_ip = addr.ip();
    if let Err(remaining) = state.check_pin_lockout(&client_ip) {
        warn!("[AUDIT] Connection rejected for {} - IP locked out ({}s remaining)", client_ip, remaining);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            format!("Too many failed PIN attempts. IP locked out for {} seconds.", remaining),
        )
            .into_response();
    }

    let role = params.role.unwrap_or_else(|| "viewer".to_string());
    let codec = params.codec;
    let ice = params.ice;
    let mode = params.mode.unwrap_or_else(|| "console".to_string());

    // Check capability if Session Mode is requested
    if mode == "session" {
        let cap = state.session_broker.probe_capabilities();
        if !cap.supported {
            let reason = cap.reason.unwrap_or_else(|| "Host does not support Session Mode".to_string());
            warn!("[AUDIT] Connection rejected for {} - requested session mode but unsupported: {}", client_ip, reason);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Session Mode unavailable: {}", reason),
            )
                .into_response();
        }
    }

    // 1. Session Token authentication (Phase 1.2)
    if let Some(ref token) = params.token {
        if state.validate_session_token(token) {
            info!(
                "[AUDIT] Successful authentication via Session Token from {} (role: {}, codec: {:?}, ice: {:?}, mode: {})",
                client_ip, role, codec, ice, mode
            );
            return ws.on_upgrade(move |socket| handle_socket(socket, state, role, codec, ice, mode, addr));
        } else {
            warn!("[AUDIT] Invalid or expired session token presented by {}", client_ip);
            return (StatusCode::UNAUTHORIZED, "Invalid or expired session token").into_response();
        }
    }

    // 2. Fallback: PIN authentication (backward compatibility)
    let pin_provided = params.pin.as_deref().unwrap_or_default();
    if pin_provided != state.config.pin {
        let attempts = state.record_pin_failure(client_ip);
        warn!(
            "[AUDIT] Unauthorized WebSocket attempt from {} with invalid PIN (attempt {}/5)",
            client_ip, attempts
        );
        if attempts >= 5 {
            warn!(
                "[AUDIT] IP {} has been locked out for 60 seconds due to 5 consecutive failed PIN attempts",
                client_ip
            );
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "Too many failed PIN attempts. IP locked out for 60 seconds.",
            )
                .into_response();
        }
        return (StatusCode::UNAUTHORIZED, "Invalid PIN code").into_response();
    }

    state.record_pin_success(&client_ip);
    info!(
        "[AUDIT] Successful authentication via PIN from {} (role: {}, codec: {:?}, ice: {:?}, mode: {})",
        client_ip, role, codec, ice, mode
    );

    ws.on_upgrade(move |socket| handle_socket(socket, state, role, codec, ice, mode, addr))
}

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    role: String,
    codec: Option<String>,
    ice: Option<String>,
    mode: String,
    addr: SocketAddr,
) {
    state.client_connected();
    let is_h264 = codec.as_deref() == Some("h264");
    info!(
        "[AUDIT] Client {} connected (role: {}, codec: {}, ice_mode: {:?}, remote_mode: {})",
        addr,
        role,
        if is_h264 { "H.264" } else { "JPEG" },
        ice,
        mode
    );

    let (mut sink, mut stream) = socket.split();

    if role == "receiver" {
        // Windows Host receiver dashboard for mobile screens
        let mut reverse_rx = state.reverse_frame_sender.subscribe();
        let receiver_task = tokio::spawn(async move {
            while let Ok(frame) = reverse_rx.recv().await {
                if let Err(_) = sink.send(Message::Binary(frame)).await {
                    break;
                }
            }
        });

        // Drain incoming messages
        while let Some(_) = stream.next().await {}
        let _ = receiver_task.abort();
    } else {
        // Standard client (phone or remote PC)
        if is_h264 {
            state.request_h264_keyframe.store(true, Ordering::Relaxed);
            if let Some(keyframe) = state.latest_h264_keyframe.read().ok().and_then(|g| g.clone()) {
                let _ = sink.send(Message::Binary(keyframe)).await;
            }
        } else {
            if let Some(initial_frame) = state.get_latest_frame() {
                let _ = sink.send(Message::Binary(initial_frame)).await;
            }
        }

        let is_session_mode = mode == "session";
        if is_session_mode {
            state.session_broker.client_connected().await;
            if let Err(e) = state.session_broker.ensure_session_started().await {
                warn!("[SessionBroker] Failed to start isolated session: {}", e);
            }
        }

        let mut frame_rx = if is_session_mode {
            state.session_broker.session_frame_sender.subscribe()
        } else if is_h264 {
            state.h264_sender.subscribe()
        } else {
            state.frame_sender.subscribe()
        };

        let (frame_tx, mut frame_rx_feeder) = tokio::sync::mpsc::channel::<bytes::Bytes>(1);
        let (audio_tx, mut audio_rx_feeder) = tokio::sync::mpsc::channel::<bytes::Bytes>(8);
        let (ctrl_tx, mut ctrl_rx) = tokio::sync::mpsc::channel::<String>(16);
        let ctrl_tx_ping = ctrl_tx.clone();

        // Send initial host desktop lock or session connection status, and QPC monotonic time sync
        if !is_session_mode {
            let is_locked = crate::capture::CaptureEngine::check_is_desktop_locked();
            let _ = ctrl_tx.try_send(format!("{{\"type\":\"lock_status\",\"locked\":{}}}", is_locked));
        } else {
            let is_session_active = state.session_broker.is_active.load(Ordering::Relaxed);
            let _ = ctrl_tx.try_send(format!("{{\"type\":\"session_status\",\"connected\":{}}}", is_session_active));
        }
        let server_time = crate::clock::qpc_now_ms();
        let _ = ctrl_tx.try_send(format!("{{\"type\":\"time_sync\",\"server_time\":{}}}", server_time));

        // 1. Task to stream screen frames, Opus audio, and priority control packets to client
        let send_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    Some(ctrl) = ctrl_rx.recv() => {
                        if let Err(e) = sink.send(Message::Text(ctrl.into())).await {
                            debug!("Failed to send control message: {:?}", e);
                            break;
                        }
                    }
                    Some(audio) = audio_rx_feeder.recv() => {
                        if let Err(e) = sink.send(Message::Binary(audio)).await {
                            debug!("Failed to send audio packet: {:?}", e);
                            break;
                        }
                    }
                    Some(frame) = frame_rx_feeder.recv() => {
                        if let Err(e) = sink.send(Message::Binary(frame)).await {
                            debug!("Failed to send frame to client: {:?}", e);
                            break;
                        }
                    }
                    else => break,
                }
            }
        });

        // 2. Cursor metadata forwarder: streams hardware cursor coords at high frequency without image encoding
        let mut cursor_rx = state.cursor_sender.subscribe();
        let ctrl_tx_cursor = ctrl_tx.clone();
        let cursor_task = tokio::spawn(async move {
            while let Ok(cursor_msg) = cursor_rx.recv().await {
                let _ = ctrl_tx_cursor.try_send(cursor_msg);
            }
        });

        // 2b. Audio forwarder: streams 0xFA 0xFA Opus packets with low latency
        let mut audio_rx = state.audio_sender.subscribe();
        let audio_tx_clone = audio_tx.clone();
        let audio_task = tokio::spawn(async move {
            while let Ok(audio_packet) = audio_rx.recv().await {
                let _ = audio_tx_clone.try_send(audio_packet);
            }
        });

        // 3. Real-time feeder task with Adaptive Bitrate Controller:
        // - Drops stale frames, guarantees 0ms bufferbloat
        // - Counts dropped frames from socket backpressure
        // - Automatically steps down resolution during network congestion (1080p -> 720p -> 540p)
        // - Respects manual mode selection (never overrides user's explicit Eco choice)
        // - Protects fast LAN clients when multiple devices are connected
        let is_manual_mode = Arc::new(AtomicBool::new(false));
        let is_manual_mode_feeder = is_manual_mode.clone();
        let stream_settings_feeder = state.stream_settings.clone();
        let connected_clients_feeder = state.connected_clients.clone();
        let feeder_task = tokio::spawn(async move {
            let mut dropped_in_window = 0u32;
            let mut last_eval = std::time::Instant::now();
            let mut last_downgrade = std::time::Instant::now();
            let mut last_upgrade = std::time::Instant::now();

            loop {
                let mut frame = match frame_rx.recv().await {
                    Ok(f) => f,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        dropped_in_window += 1;
                        continue;
                    }
                    Err(_) => break,
                };

                // Drain all accumulated frames, pick only the newest
                while let Ok(newer) = frame_rx.try_recv() {
                    dropped_in_window += 1;
                    frame = newer;
                }

                // If previous frame is still being transmitted over slow network, drop and do NOT queue
                if let Err(_) = frame_tx.try_send(frame) {
                    dropped_in_window += 1;
                }

                // Evaluate network congestion every 2 seconds
                if last_eval.elapsed() >= std::time::Duration::from_secs(2) {
                    let current_height = stream_settings_feeder.get_target_height();
                    let is_manual = is_manual_mode_feeder.load(Ordering::Relaxed);
                    let client_count = connected_clients_feeder.load(Ordering::Relaxed);

                    if dropped_in_window >= 5 && last_downgrade.elapsed() >= std::time::Duration::from_secs(4) {
                        // Congestion detected: only step down if single client (or per-client protection)
                        if client_count <= 1 && !is_manual {
                            if current_height > 720 {
                                stream_settings_feeder.apply_preset("balanced");
                                info!("Adaptive Controller: Congestion detected ({} drops), scaled down to 720p", dropped_in_window);
                            } else if current_height > 540 {
                                stream_settings_feeder.apply_preset("eco");
                                info!("Adaptive Controller: Severe congestion ({} drops), scaled down to 540p", dropped_in_window);
                            }
                            last_downgrade = std::time::Instant::now();
                            last_upgrade = std::time::Instant::now();
                        } else if client_count > 1 {
                            debug!("Adaptive Controller: Dropped {} frames on congested client; preserving global resolution for {} active clients", dropped_in_window, client_count);
                        }
                    } else if dropped_in_window == 0 && last_upgrade.elapsed() >= std::time::Duration::from_secs(12) && !is_manual {
                        // Only recover upwards if user did NOT lock manual mode
                        if current_height < 720 {
                            stream_settings_feeder.apply_preset("balanced");
                            info!("Adaptive Controller: Network stable, recovering to 720p");
                            last_upgrade = std::time::Instant::now();
                        }
                    }
                    dropped_in_window = 0;
                    last_eval = std::time::Instant::now();
                }
            }
        });

        // 3. Receive inputs, pings & reverse frames from client
        let input_manager = state.input_manager.clone();
        let stream_settings = state.stream_settings.clone();
        let reverse_sender = state.reverse_frame_sender.clone();

        let (webrtc_ws_tx, mut webrtc_ws_rx) = tokio::sync::mpsc::channel::<String>(32);
        let ctrl_tx_webrtc = ctrl_tx.clone();
        let _webrtc_ws_forwarder = tokio::spawn(async move {
            while let Some(msg) = webrtc_ws_rx.recv().await {
                let _ = ctrl_tx_webrtc.send(msg).await;
            }
        });

        let (session_close_tx, mut session_close_rx) = tokio::sync::mpsc::channel::<()>(8);
        let webrtc_session: Arc<tokio::sync::Mutex<Option<crate::webrtc_session::WebRtcSession>>> =
            Arc::new(tokio::sync::Mutex::new(None));

        let webrtc_session_cleanup = Arc::clone(&webrtc_session);
        let _webrtc_cleanup_worker = tokio::spawn(async move {
            while let Some(_) = session_close_rx.recv().await {
                let mut lock = webrtc_session_cleanup.lock().await;
                if let Some(session) = lock.take() {
                    info!("[WebRTC] Cleared dead session from slot (Bug B lifecycle fix)");
                    let _ = session.peer_connection.close().await;
                }
            }
        });

        while let Some(msg_result) = stream.next().await {
            match msg_result {
                Ok(Message::Binary(data)) => {
                    // Check if this is a reverse frame (Phone -> PC)
                    if data.len() > 2 && data[0] == 0xFE && data[1] == 0xFE {
                        let _ = reverse_sender.send(data);
                    }
                }
                Ok(Message::Text(text)) => {
                    // Try parsing commands or input
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match msg_type {
                            "signal" => {
                                let action = val.get("action").and_then(|v| v.as_str()).unwrap_or("");
                                match action {
                                    "offer" => {
                                        if let Some(sdp) = val.get("sdp").and_then(|v| v.as_str()) {
                                            let mut session_lock = webrtc_session.lock().await;
                                            let needs_new_session = match &*session_lock {
                                                None => true,
                                                Some(session) => session.is_closed_or_failed(),
                                            };

                                            if needs_new_session {
                                                if let Some(old_session) = session_lock.take() {
                                                    info!(
                                                        "[WebRTC] Replacing previous session (state: {:?}) with new session",
                                                        old_session.connection_state()
                                                    );
                                                    let _ = old_session.peer_connection.close().await;
                                                }

                                                let context_ice_servers = get_context_ice_servers(&addr.ip(), ice.as_deref());
                                                match crate::webrtc_session::WebRtcSession::new(
                                                    webrtc_ws_tx.clone(),
                                                    state.h264_sender.subscribe(),
                                                    state.audio_sender.subscribe(),
                                                    state.request_h264_keyframe.clone(),
                                                    input_manager.clone(),
                                                    session_close_tx.clone(),
                                                    context_ice_servers,
                                                ).await {
                                                    Ok(session) => {
                                                        *session_lock = Some(session);
                                                    }
                                                    Err(e) => {
                                                        error!("[WebRTC] Failed to initialize WebRtcSession: {:?}", e);
                                                    }
                                                }
                                            }

                                            if let Some(ref session) = *session_lock {
                                                match session.handle_offer(sdp).await {
                                                    Ok(answer_sdp) => {
                                                        let resp = json!({
                                                            "type": "signal",
                                                            "action": "answer",
                                                            "sdp": answer_sdp,
                                                        });
                                                        let _ = ctrl_tx_ping.try_send(resp.to_string());
                                                        info!("[WebRTC] Processed offer and sent answer to client {}", addr);
                                                    }
                                                    Err(e) => {
                                                        error!("[WebRTC] handle_offer error: {:?}", e);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    "candidate" => {
                                        if let Some(cand_val) = val.get("candidate") {
                                            let cand_init_res = if let Some(s) = cand_val.as_str() {
                                                serde_json::from_str::<webrtc::ice_transport::ice_candidate::RTCIceCandidateInit>(s)
                                            } else {
                                                serde_json::from_value::<webrtc::ice_transport::ice_candidate::RTCIceCandidateInit>(cand_val.clone())
                                            };

                                            match cand_init_res {
                                                Ok(cand_init) => {
                                                    let session_lock = webrtc_session.lock().await;
                                                    if let Some(ref session) = *session_lock {
                                                        if let Err(e) = session.add_ice_candidate(cand_init).await {
                                                            warn!("[WebRTC] add_ice_candidate error: {:?}", e);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!("[WebRTC] Failed to parse candidate: {:?}, raw: {:?}", e, cand_val);
                                                }
                                            }
                                        }
                                    }
                                    _ => {
                                        debug!("[WebRTC] Unrecognized signal action: {}", action);
                                    }
                                }
                            }
                            "ping" => {
                                let t = val.get("t").cloned();
                                let client_time = val.get("client_time").and_then(|v| v.as_u64())
                                    .or_else(|| val.get("t").and_then(|v| v.as_u64()))
                                    .unwrap_or(0);
                                let server_time = crate::clock::qpc_now_ms();
                                let resp = json!({
                                    "type": "pong",
                                    "t": t,
                                    "client_time": client_time,
                                    "server_time": server_time,
                                });
                                let _ = ctrl_tx_ping.try_send(resp.to_string());
                            }
                            "set_fps" => {
                                if let Some(fps) = val.get("fps").and_then(|v| v.as_u64()) {
                                    stream_settings.set_fps(fps as u32);
                                    info!("Updated target FPS to {}", fps);
                                }
                            }
                            "set_quality" => {
                                if let Some(q) = val.get("quality").and_then(|v| v.as_u64()) {
                                    stream_settings.set_quality(q as u8);
                                    info!("Updated target JPEG quality to {}", q);
                                }
                            }
                            "set_resolution" => {
                                if let Some(h) = val.get("height").and_then(|v| v.as_u64()) {
                                    stream_settings.set_target_height(h as u32);
                                    info!("Updated target stream height to {}p", h);
                                }
                            }
                            "set_mode" => {
                                if let Some(mode) = val.get("mode").and_then(|v| v.as_str()) {
                                    if mode.eq_ignore_ascii_case("auto") {
                                        is_manual_mode.store(false, Ordering::Relaxed);
                                        info!("Switched to Auto adaptive streaming mode");
                                    } else {
                                        is_manual_mode.store(true, Ordering::Relaxed);
                                        stream_settings.apply_preset(mode);
                                        info!("Applied manual streaming mode preset: {} (locked)", mode);
                                    }
                                }
                            }
                            "frame_loss" => {
                                state.request_h264_keyframe.store(true, Ordering::Relaxed);
                                info!(
                                    "[PLI] Frame loss reported by client {}, requesting forced H.264 IDR keyframe",
                                    addr
                                );
                            }
                            _ => {
                                match serde_json::from_value::<InputMessage>(val) {
                                    Ok(input_msg) => {
                                        if is_session_mode {
                                            let _ = state.session_broker.session_input_tx.try_send(input_msg);
                                        } else {
                                            input_manager.handle_input(input_msg);
                                        }
                                    }
                                    Err(e) => {
                                        debug!("Unrecognized or malformed WS message: {:?}", e);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    debug!("WebSocket stream error: {:?}", e);
                    break;
                }
                _ => {}
            }
        }

        if let Some(session) = webrtc_session.lock().await.take() {
            info!("[WebRTC] WebSocket closed, cleaning up active WebRtcSession for {}", addr);
            let _ = session.peer_connection.close().await;
        }

        let _ = cursor_task.abort();
        let _ = audio_task.abort();
        let _ = feeder_task.abort();
        let _ = send_task.abort();
        let _ = _webrtc_ws_forwarder.abort();
        let _ = _webrtc_cleanup_worker.abort();

        if is_session_mode {
            state.session_broker.client_disconnected().await;
        }
    }

    state.client_disconnected();
    info!("[AUDIT] WebSocket client disconnected: {} (active: {})", addr, state.active_clients());
}
