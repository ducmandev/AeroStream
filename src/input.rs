use enigo::{Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use windows_sys::Win32::System::StationsAndDesktops::*;
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

    unsafe fn attach_input_desktop() -> bool {
        // 0x10000000 = MAXIMUM_ALLOWED
        let desk = OpenInputDesktop(0, 0, 0x10000000);
        if !desk.is_null() {
            let res = SetThreadDesktop(desk);
            CloseDesktop(desk);
            res != 0
        } else {
            let desk = OpenInputDesktop(0, 0, 0x0001 | 0x0080);
            if !desk.is_null() {
                let res = SetThreadDesktop(desk);
                CloseDesktop(desk);
                res != 0
            } else {
                false
            }
        }
    }

    unsafe fn set_cursor_pos_with_retry(abs_x: i32, abs_y: i32) {
        if SetCursorPos(abs_x, abs_y) == 0 {
            let _ = Self::attach_input_desktop();
            SetCursorPos(abs_x, abs_y);
        }
    }

    unsafe fn dispatch_mouse_event(flags: u32, dx: i32, dy: i32, dw_data: i32, dw_extra_info: usize) {
        let mut pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            let _ = Self::attach_input_desktop();
        }
        mouse_event(flags, dx, dy, dw_data, dw_extra_info);
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

        let mut enigo = match Enigo::new(&Settings::default()) {
            Ok(e) => Some(e),
            Err(err) => {
                crate::debug_log::log_gui(&format!("Enigo initialization error: {:?}", err));
                None
            }
        };

        let mut stick_w = false;
        let mut stick_a = false;
        let mut stick_s = false;
        let mut stick_d = false;

        while let Ok(msg) = rx.recv() {
            let screen_w = sw.load(Ordering::Relaxed).max(1);
            let screen_h = sh.load(Ordering::Relaxed).max(1);

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
                    if let Some(ref mut en) = enigo {
                        if let Some(k) = parse_key(&key) {
                            let _ = en.key(k, Direction::Press);
                        }
                    }
                }
                InputMessage::KeyUp { key } => {
                    if let Some(ref mut en) = enigo {
                        if let Some(k) = parse_key(&key) {
                            let _ = en.key(k, Direction::Release);
                        }
                    }
                }
                InputMessage::KeyClick { key } => {
                    if let Some(ref mut en) = enigo {
                        if let Some(k) = parse_key(&key) {
                            let _ = en.key(k, Direction::Click);
                        }
                    }
                }
                InputMessage::Shortcut { keys } => {
                    if let Some(ref mut en) = enigo {
                        let parsed_keys: Vec<Key> = keys.iter().filter_map(|k| parse_key(k)).collect();
                        for &k in &parsed_keys {
                            let _ = en.key(k, Direction::Press);
                        }
                        for &k in parsed_keys.iter().rev() {
                            let _ = en.key(k, Direction::Release);
                        }
                    }
                }
                InputMessage::Text { text } => {
                    if let Some(ref mut en) = enigo {
                        let _ = en.text(&text);
                    }
                }
                InputMessage::GamepadAxis { stick, x, y } => {
                    if let Some(ref mut en) = enigo {
                        if stick == "left" {
                            let threshold = 0.35;
                            let should_w = y < -threshold;
                            let should_s = y > threshold;
                            let should_a = x < -threshold;
                            let should_d = x > threshold;

                            Self::update_key_state_local(en, Key::Unicode('w'), &mut stick_w, should_w);
                            Self::update_key_state_local(en, Key::Unicode('s'), &mut stick_s, should_s);
                            Self::update_key_state_local(en, Key::Unicode('a'), &mut stick_a, should_a);
                            Self::update_key_state_local(en, Key::Unicode('d'), &mut stick_d, should_d);
                        } else if stick == "right" {
                            let sens = 12.0;
                            let dx = (x * sens).round() as i32;
                            let dy = (y * sens).round() as i32;
                            if dx != 0 || dy != 0 {
                                let _ = en.move_mouse(dx, dy, Coordinate::Rel);
                            }
                        }
                    }
                }
                InputMessage::GamepadButton { button, pressed } => {
                    if let Some(ref mut en) = enigo {
                        let dir = if pressed { Direction::Press } else { Direction::Release };
                        match button.to_uppercase().as_str() {
                            "A" => { let _ = en.key(Key::Space, dir); }
                            "B" => { let _ = en.key(Key::Control, dir); }
                            "X" => { let _ = en.key(Key::Unicode('e'), dir); }
                            "Y" => { let _ = en.key(Key::Unicode('r'), dir); }
                            "LB" => { let _ = en.key(Key::Shift, dir); }
                            "RB" => { let _ = en.key(Key::Unicode('f'), dir); }
                            "LT" => { let _ = en.button(Button::Right, dir); }
                            "RT" => { let _ = en.button(Button::Left, dir); }
                            "START" => { let _ = en.key(Key::Escape, dir); }
                            "SELECT" => { let _ = en.key(Key::Tab, dir); }
                            "UP" => { let _ = en.key(Key::UpArrow, dir); }
                            "DOWN" => { let _ = en.key(Key::DownArrow, dir); }
                            "LEFT" => { let _ = en.key(Key::LeftArrow, dir); }
                            "RIGHT" => { let _ = en.key(Key::RightArrow, dir); }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    fn update_key_state_local(enigo: &mut Enigo, key: Key, is_down: &mut bool, should_be_down: bool) {
        if should_be_down && !*is_down {
            let _ = enigo.key(key, Direction::Press);
            *is_down = true;
        } else if !should_be_down && *is_down {
            let _ = enigo.key(key, Direction::Release);
            *is_down = false;
        }
    }
}

#[allow(dead_code)]
fn parse_button(button: &str) -> Button {
    match button.to_lowercase().as_str() {
        "right" => Button::Right,
        "middle" => Button::Middle,
        _ => Button::Left,
    }
}

fn parse_key(key: &str) -> Option<Key> {
    match key.to_lowercase().as_str() {
        "enter" => Some(Key::Return),
        "backspace" => Some(Key::Backspace),
        "escape" | "esc" => Some(Key::Escape),
        "tab" => Some(Key::Tab),
        "space" => Some(Key::Space),
        "up" | "arrowup" => Some(Key::UpArrow),
        "down" | "arrowdown" => Some(Key::DownArrow),
        "left" | "arrowleft" => Some(Key::LeftArrow),
        "right" | "arrowright" => Some(Key::RightArrow),
        "control" | "ctrl" => Some(Key::Control),
        "shift" => Some(Key::Shift),
        "alt" => Some(Key::Alt),
        "meta" | "win" | "cmd" => Some(Key::Meta),
        "delete" | "del" => Some(Key::Delete),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" | "pgup" => Some(Key::PageUp),
        "pagedown" | "pgdn" => Some(Key::PageDown),
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        s if s.chars().count() == 1 => Some(Key::Unicode(s.chars().next().unwrap())),
        _ => None,
    }
}
