//! Bridge: Timer and microtask system
//!
//! Optimized timer handling - stores only IDs in Rust, callbacks in JS.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

// ===================================================================
// Timer system
// ===================================================================

/// Timer entry - stores ID and metadata only, NOT the callback
#[derive(Debug, Clone)]
struct TimerEntry {
    id: u32,
    delay_ms: u64,
    is_interval: bool,
    created_at: Instant,
    last_fired: Option<Instant>,
}

/// Timer registry
static TIMERS: std::sync::LazyLock<
    std::sync::Arc<std::sync::Mutex<HashMap<u32, TimerEntry>>>,
> = std::sync::LazyLock::new(|| {
    std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()))
});

static NEXT_TIMER_ID: std::sync::LazyLock<std::sync::Arc<AtomicU32>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(AtomicU32::new(1)));

/// Reset timer ID counter (for testing)
pub fn __ink_reset_timer_id() {
    NEXT_TIMER_ID.store(1, Ordering::SeqCst);
}

/// Create a one-shot timer (setTimeout equivalent)
pub fn __ink_set_timeout(callback_js: &str, delay_ms: u64) -> u32 {
    let _js_id: u32 = callback_js.parse().unwrap_or(0);
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);

    let entry = TimerEntry {
        id,
        delay_ms,
        is_interval: false,
        created_at: Instant::now(),
        last_fired: None,
    };

    let mut timers = TIMERS.lock().unwrap();
    timers.insert(id, entry);
    tracing::debug!("Created timeout rust_id={}, delay={}ms", id, delay_ms);
    id
}

/// Create an interval timer (setInterval equivalent)
pub fn __ink_set_interval(callback_js: &str, interval_ms: u64) -> u32 {
    let _js_id: u32 = callback_js.parse().unwrap_or(0);
    let id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);

    let entry = TimerEntry {
        id,
        delay_ms: interval_ms,
        is_interval: true,
        created_at: Instant::now(),
        last_fired: None,
    };

    let mut timers = TIMERS.lock().unwrap();
    timers.insert(id, entry);
    tracing::debug!("Created interval rust_id={}, interval={}ms", id, interval_ms);
    id
}

/// Clear a timer
pub fn __ink_clear_timer(id: u32) -> bool {
    let mut timers = TIMERS.lock().unwrap();
    let removed = timers.remove(&id).is_some();
    if removed {
        tracing::debug!("Cleared timer {}", id);
    }
    removed
}

/// Clear all timers (for testing)
pub fn __ink_clear_all_timers() {
    let mut timers = TIMERS.lock().unwrap();
    timers.clear();
}

/// Get IDs of all timers that should fire now
pub fn __ink_process_timers() -> String {
    let now = Instant::now();
    let mut timers = TIMERS.lock().unwrap();

    let timers_to_fire: Vec<u32> = timers
        .iter()
        .filter(|(_, timer)| {
            let elapsed = now
                .duration_since(timer.last_fired.unwrap_or(timer.created_at))
                .as_millis() as u64;
            elapsed >= timer.delay_ms
        })
        .filter(|(_, timer)| timer.is_interval || timer.last_fired.is_none())
        .map(|(&id, _)| id)
        .collect();

    for id in &timers_to_fire {
        if let Some(entry) = timers.get_mut(id) {
            if entry.is_interval {
                entry.last_fired = Some(now);
            } else {
                timers.remove(id);
            }
        }
    }

    let ids_str = timers_to_fire
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", ids_str)
}

/// Check if there are any pending timers
pub fn __ink_has_pending_timers() -> bool {
    let timers = TIMERS.lock().unwrap();
    !timers.is_empty()
}

/// Get time until next timer fires
pub fn __ink_next_timer_delay() -> Option<Duration> {
    let now = Instant::now();
    let timers = TIMERS.lock().unwrap();

    timers
        .values()
        .map(|timer| {
            let since = timer.last_fired.unwrap_or(timer.created_at);
            let elapsed = now.duration_since(since);
            if elapsed >= Duration::from_millis(timer.delay_ms) {
                Duration::ZERO
            } else {
                Duration::from_millis(timer.delay_ms) - elapsed
            }
        })
        .min()
}

// ===================================================================
// Microtask system
// ===================================================================

/// Flag to indicate pending microtasks
static HAS_PENDING_MICROTASKS: AtomicBool = AtomicBool::new(false);

/// Clear microtask flag (for testing)
pub fn __ink_clear_all_microtasks() {
    HAS_PENDING_MICROTASKS.store(false, Ordering::SeqCst);
}

/// Signal that microtasks are pending
pub fn __ink_enqueue_microtask(_callback_js: &str) {
    HAS_PENDING_MICROTASKS.store(true, Ordering::SeqCst);
}

/// Drain microtasks - returns true if there were pending microtasks
pub fn __ink_drain_microtasks() -> bool {
    let had_pending = HAS_PENDING_MICROTASKS.load(Ordering::SeqCst);
    HAS_PENDING_MICROTASKS.store(false, Ordering::SeqCst);
    had_pending
}
