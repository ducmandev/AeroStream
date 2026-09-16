use std::fs::OpenOptions;
use std::io::Write;

pub fn log_gui(msg: &str) {
    let log_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("aerostream_debug.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("aerostream_debug.log"));

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = writeln!(file, "[{}] {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(), msg);
    }
}
