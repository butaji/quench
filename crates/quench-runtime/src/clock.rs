//! The clocks the engine and its host read, in one place (M6, P6 work clock).
//!
//! By default every reader gets the operating system's clocks. Under `QUENCH_DEV=virtual-clock`
//! the JavaScript-visible clocks (`Date.now`, `performance.now`, `process.hrtime`, timers) read
//! one virtual clock instead: it starts at a fixed instant, moves forward a fixed step on each
//! read, and jumps to the next timer when the event loop would otherwise wait. `Math.random`
//! then starts from a fixed seed. A run's instruction count no longer depends on how fast the
//! host is, so A/B comparisons can trust small deltas. Scores printed in this mode measure
//! virtual time and must not be used.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How far the virtual clock moves on each read: enough that two reads differ, as
/// benchmark harnesses divide by elapsed time.
const VIRTUAL_STEP_NS: u64 = 100_000;
/// Where virtual wall time starts: 2026-01-01T00:00:00Z.
const VIRTUAL_EPOCH_SECS: u64 = 1_767_225_600;
/// The `Math.random` seed under the virtual clock (any odd value).
const VIRTUAL_RANDOM_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

static VIRTUAL_NS: AtomicU64 = AtomicU64::new(0);

/// Whether the JavaScript-visible clocks are virtual (`QUENCH_DEV=virtual-clock`), read once.
pub fn is_virtual() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::trace::Dev::VirtualClock.on())
}

/// The instant the virtual clock counts from, taken on first use.
fn virtual_base() -> Instant {
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

/// Nanoseconds of virtual time after a read, which moves the clock one step.
fn virtual_tick() -> u64 {
    VIRTUAL_NS.fetch_add(VIRTUAL_STEP_NS, Ordering::Relaxed) + VIRTUAL_STEP_NS
}

/// The monotonic instant now.
pub fn instant_now() -> Instant {
    if is_virtual() {
        virtual_base() + Duration::from_nanos(virtual_tick())
    } else {
        Instant::now()
    }
}

/// The wall-clock time now.
pub fn system_now() -> SystemTime {
    if is_virtual() {
        UNIX_EPOCH + Duration::from_secs(VIRTUAL_EPOCH_SECS) + Duration::from_nanos(virtual_tick())
    } else {
        SystemTime::now()
    }
}

/// Milliseconds since the Unix epoch, as `Date.now` reads them.
pub fn unix_millis() -> f64 {
    system_now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64() * 1000.0
}

/// A monotonic clock in nanoseconds (`CLOCK_MONOTONIC` by default); `None` where unavailable.
pub fn monotonic_ns() -> Option<f64> {
    if is_virtual() {
        // Offset by a day so that origins taken from it are never zero.
        return Some(86_400.0 * 1e9 + virtual_tick() as f64);
    }
    #[cfg(unix)]
    {
        let mut time: libc::timespec = unsafe { std::mem::zeroed() };
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
            return None;
        }
        Some(time.tv_sec as f64 * 1_000_000_000.0 + time.tv_nsec as f64)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Whether engine policies read the work clock instead of wall time (`QUENCH_DEV=work-clock`,
/// implied by `virtual-clock`): GC old-generation sizing charges work units, so a run's
/// collections do not depend on host speed or timing jitter.
pub fn is_work() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| is_virtual() || crate::trace::Dev::WorkClock.on())
}

/// Work done by every heap in the process, in typical-host nanoseconds (`now_work`).
static WORK: AtomicU64 = AtomicU64::new(0);

/// The engine work clock: a monotone count of work done, in typical-host nanoseconds, that
/// any policy may read in place of wall time (GC pacing, RC epochs, allocator release, code
/// aging). It advances at each collection by the mutator work since the previous one
/// (weight allocated) and the collection's own work (weight it traced), so it is coarse
/// between collections but identical on every run of the same program. One relaxed load.
pub fn now_work() -> u64 {
    WORK.load(Ordering::Relaxed)
}

/// Adds `units` of work to the work clock (`now_work`); heaps call it once per collection.
pub fn charge_work(units: u64) {
    WORK.fetch_add(units, Ordering::Relaxed);
}

/// Under the virtual clock, moves virtual time forward to `due` (a waiting event loop's next
/// timer) and returns true, so the loop runs the timer without sleeping; false otherwise.
pub fn skip_to(due: Instant) -> bool {
    if !is_virtual() {
        return false;
    }
    let target = due.saturating_duration_since(virtual_base()).as_nanos() as u64;
    VIRTUAL_NS.fetch_max(target, Ordering::Relaxed);
    true
}

/// The fixed `Math.random` seed under the virtual clock.
pub fn fixed_random_seed() -> Option<u64> {
    is_virtual().then_some(VIRTUAL_RANDOM_SEED)
}
