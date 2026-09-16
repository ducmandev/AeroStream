use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub port: u16,
    pub pin: String,
    pub fps: u32,
    pub quality: u8,
    pub scale: f32,
    pub monitor_index: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            pin: "123456".to_string(),
            fps: 60,
            quality: 80,
            scale: 1.0,
            monitor_index: 0,
        }
    }
}

/// Dynamic settings that can be adjusted on-the-fly by connected clients
#[derive(Debug)]
pub struct DynamicStreamSettings {
    pub fps: AtomicU32,
    pub quality: AtomicU8,
    pub target_height: AtomicU32,
    pub paused: AtomicBool,
}

impl DynamicStreamSettings {
    pub fn new(fps: u32, quality: u8) -> Self {
        Self {
            fps: AtomicU32::new(fps),
            quality: AtomicU8::new(quality),
            target_height: AtomicU32::new(720), // 720p default for optimal 60 FPS performance
            paused: AtomicBool::new(false),
        }
    }

    pub fn get_fps(&self) -> u32 {
        self.fps.load(Ordering::Relaxed).clamp(15, 120)
    }

    pub fn set_fps(&self, fps: u32) {
        self.fps.store(fps.clamp(15, 120), Ordering::Relaxed);
    }

    pub fn get_quality(&self) -> u8 {
        self.quality.load(Ordering::Relaxed).clamp(30, 95)
    }

    pub fn set_quality(&self, quality: u8) {
        self.quality.store(quality.clamp(30, 95), Ordering::Relaxed);
    }

    pub fn get_target_height(&self) -> u32 {
        self.target_height.load(Ordering::Relaxed).clamp(360, 2160)
    }

    pub fn set_target_height(&self, height: u32) {
        self.target_height.store(height.clamp(360, 2160), Ordering::Relaxed);
    }

    pub fn apply_preset(&self, mode: &str) {
        match mode.to_lowercase().as_str() {
            "eco" => {
                self.set_target_height(540);
                self.set_quality(55);
                self.set_fps(60);
            }
            "balanced" => {
                self.set_target_height(720);
                self.set_quality(70);
                self.set_fps(60);
            }
            "quality" | "high" => {
                self.set_target_height(1080);
                self.set_quality(80);
                self.set_fps(60);
            }
            "ultra" => {
                self.set_target_height(1080);
                self.set_quality(92);
                self.set_fps(60);
            }
            "2k" | "1440p" => {
                self.set_target_height(1440);
                self.set_quality(85);
                self.set_fps(60);
            }
            "smooth_30" => {
                self.set_target_height(720);
                self.set_quality(65);
                self.set_fps(30);
            }
            _ => {}
        }
    }

    #[allow(dead_code)]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    #[allow(dead_code)]
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}
