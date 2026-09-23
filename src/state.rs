use crate::config::{AppConfig, DynamicStreamSettings};
use crate::input::InputManager;
use bytes::Bytes;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct SessionToken {
    pub token: String,
    pub client_ip: IpAddr,
    pub created_at: Instant,
    pub expires_at: Instant,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub stream_settings: Arc<DynamicStreamSettings>,
    pub input_manager: Arc<InputManager>,
    pub frame_sender: broadcast::Sender<Bytes>,
    pub h264_sender: broadcast::Sender<Bytes>,
    pub cursor_sender: broadcast::Sender<String>,
    pub audio_sender: broadcast::Sender<Bytes>,
    pub reverse_frame_sender: broadcast::Sender<Bytes>,
    pub latest_frame: Arc<RwLock<Option<Bytes>>>,
    pub latest_h264_keyframe: Arc<RwLock<Option<Bytes>>>,
    pub request_h264_keyframe: Arc<AtomicBool>,
    pub connected_clients: Arc<AtomicUsize>,
    pub current_fps: Arc<AtomicU32>,
    pub server_start_time: std::time::Instant,
    pub failed_pin_attempts: Arc<Mutex<HashMap<IpAddr, (u32, Instant)>>>,
    pub active_tokens: Arc<RwLock<HashMap<String, SessionToken>>>,
    pub session_broker: Arc<crate::session_broker::SessionBroker>,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        input_manager: InputManager,
        _screen_w: u32,
        _screen_h: u32,
    ) -> Self {
        let (frame_sender, _) = broadcast::channel(4);
        let (h264_sender, _) = broadcast::channel(8);
        let (cursor_sender, _) = broadcast::channel(16);
        let (audio_sender, _) = broadcast::channel(64);
        let (reverse_frame_sender, _) = broadcast::channel(2);
        let stream_settings = Arc::new(DynamicStreamSettings::new(config.fps, config.quality));
        let session_broker = Arc::new(crate::session_broker::SessionBroker::new());

        Self {
            config: Arc::new(config),
            stream_settings,
            input_manager: Arc::new(input_manager),
            frame_sender,
            h264_sender,
            cursor_sender,
            audio_sender,
            reverse_frame_sender,
            latest_frame: Arc::new(RwLock::new(None)),
            latest_h264_keyframe: Arc::new(RwLock::new(None)),
            request_h264_keyframe: Arc::new(AtomicBool::new(false)),
            connected_clients: Arc::new(AtomicUsize::new(0)),
            current_fps: Arc::new(AtomicU32::new(0)),
            server_start_time: std::time::Instant::now(),
            failed_pin_attempts: Arc::new(Mutex::new(HashMap::new())),
            active_tokens: Arc::new(RwLock::new(HashMap::new())),
            session_broker,
        }
    }

    #[allow(dead_code)]
    pub fn set_latest_frame(&self, frame: Bytes) {
        if let Ok(mut guard) = self.latest_frame.write() {
            *guard = Some(frame);
        }
    }

    pub fn get_latest_frame(&self) -> Option<Bytes> {
        self.latest_frame.read().ok().and_then(|guard| guard.clone())
    }

    pub fn client_connected(&self) {
        self.connected_clients.fetch_add(1, Ordering::Relaxed);
    }

    pub fn client_disconnected(&self) {
        self.connected_clients.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn active_clients(&self) -> usize {
        self.connected_clients.load(Ordering::Relaxed)
    }

    pub fn get_fps(&self) -> u32 {
        self.current_fps.load(Ordering::Relaxed)
    }

    pub fn check_pin_lockout(&self, ip: &IpAddr) -> Result<(), u64> {
        let mut attempts = match self.failed_pin_attempts.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some((count, last_attempt)) = attempts.get(ip) {
            let elapsed = last_attempt.elapsed();
            if *count >= 5 {
                if elapsed < std::time::Duration::from_secs(60) {
                    let remaining = 60 - elapsed.as_secs();
                    return Err(remaining.max(1));
                } else {
                    attempts.remove(ip);
                }
            } else if elapsed >= std::time::Duration::from_secs(60) {
                attempts.remove(ip);
            }
        }
        Ok(())
    }

    pub fn record_pin_failure(&self, ip: IpAddr) -> u32 {
        let mut attempts = match self.failed_pin_attempts.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let entry = attempts.entry(ip).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= std::time::Duration::from_secs(60) && entry.0 < 5 {
            entry.0 = 0;
        }
        entry.0 += 1;
        entry.1 = Instant::now();
        entry.0
    }

    pub fn record_pin_success(&self, ip: &IpAddr) {
        let mut attempts = match self.failed_pin_attempts.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        attempts.remove(ip);
    }

    pub fn create_session_token(&self, ip: IpAddr) -> String {
        let token = format!("{:016x}{:016x}", rand::random::<u64>(), rand::random::<u64>());
        let session = SessionToken {
            token: token.clone(),
            client_ip: ip,
            created_at: Instant::now(),
            expires_at: Instant::now() + std::time::Duration::from_secs(86400),
        };
        if let Ok(mut tokens) = self.active_tokens.write() {
            tokens.insert(token.clone(), session);
        }
        token
    }

    pub fn validate_session_token(&self, token: &str) -> bool {
        if let Ok(tokens) = self.active_tokens.read() {
            if let Some(session) = tokens.get(token) {
                return Instant::now() < session.expires_at;
            }
        }
        false
    }

    #[allow(dead_code)]
    pub fn cleanup_expired_tokens(&self) {
        if let Ok(mut tokens) = self.active_tokens.write() {
            let now = Instant::now();
            tokens.retain(|_, v| now < v.expires_at);
        }
    }
}
