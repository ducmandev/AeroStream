use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

extern "system" {
    fn QueryPerformanceCounter(lpPerformanceCount: *mut i64) -> i32;
    fn QueryPerformanceFrequency(lpFrequency: *mut i64) -> i32;
}

static QPC_FREQ: AtomicI64 = AtomicI64::new(0);
static QPC_BASE_TICKS: AtomicI64 = AtomicI64::new(0);
static UNIX_BASE_MS: AtomicU64 = AtomicU64::new(0);

/// Initialize the high-resolution QPC clock anchored to Unix Epoch.
/// Safe to call multiple times; initialization happens once.
pub fn init_clock() {
    if QPC_FREQ.load(Ordering::Acquire) > 0 {
        return;
    }

    let mut freq = 0i64;
    let mut ticks = 0i64;
    unsafe {
        if QueryPerformanceFrequency(&mut freq) == 0 || freq <= 0 {
            freq = 10_000_000; // Standard 10MHz fallback
        }
        if QueryPerformanceCounter(&mut ticks) == 0 {
            ticks = 0;
        }
    }

    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    QPC_FREQ.store(freq, Ordering::Release);
    QPC_BASE_TICKS.store(ticks, Ordering::Release);
    UNIX_BASE_MS.store(unix_ms, Ordering::Release);

    tracing::info!(
        "[Phase 4 Clock] Windows QPC Monotonic Clock Initialized: freq={} Hz, base_ms={}",
        freq,
        unix_ms
    );
}

/// Returns current monotonic timestamp in milliseconds (anchored to Unix Epoch at init).
/// High precision, invariant to system clock adjustments (NTP/DST), zero drift.
pub fn qpc_now_ms() -> u64 {
    let freq = QPC_FREQ.load(Ordering::Acquire);
    if freq <= 0 {
        init_clock();
    }
    let freq = QPC_FREQ.load(Ordering::Relaxed).max(1);
    let base_ticks = QPC_BASE_TICKS.load(Ordering::Relaxed);
    let base_ms = UNIX_BASE_MS.load(Ordering::Relaxed);

    let mut current_ticks = 0i64;
    unsafe {
        if QueryPerformanceCounter(&mut current_ticks) == 0 {
            return SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
        }
    }

    let delta_ticks = current_ticks.saturating_sub(base_ticks).max(0);
    let delta_ms = (delta_ticks as u128 * 1000 / freq as u128) as u64;

    base_ms + delta_ms
}

/// Returns current monotonic timestamp in microseconds.
#[allow(dead_code)]
pub fn qpc_now_us() -> u64 {
    let freq = QPC_FREQ.load(Ordering::Acquire);
    if freq <= 0 {
        init_clock();
    }
    let freq = QPC_FREQ.load(Ordering::Relaxed).max(1);
    let base_ticks = QPC_BASE_TICKS.load(Ordering::Relaxed);
    let base_ms = UNIX_BASE_MS.load(Ordering::Relaxed);

    let mut current_ticks = 0i64;
    unsafe {
        if QueryPerformanceCounter(&mut current_ticks) == 0 {
            return SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;
        }
    }

    let delta_ticks = current_ticks.saturating_sub(base_ticks).max(0);
    let delta_us = (delta_ticks as u128 * 1_000_000 / freq as u128) as u64;

    (base_ms * 1000) + delta_us
}
