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

/// Whether a build-time evaluation watches the program's inputs from its environment
/// (`watch_inputs`): strict AOT runs an entry at build time, and a run that read the clock
/// (`CapabilityId::ClockMillis`), the local time zone or `Math.random` computed values a later
/// run would not. The host's own clock reads (its event loop's) are no input of the program.
static INPUTS_WATCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! {
    /// The execution a watched run is on, which the first input ends (`watch_inputs`). The run
    /// and its clock reads share the thread that watches them.
    static WATCHED_RUN: std::cell::RefCell<Option<crate::ExecutionInterrupt>> = const { std::cell::RefCell::new(None) };
    /// The heap state a watched run captured at its first input (`store_input_capture`), taken by
    /// its build (`take_input_capture`).
    static INPUT_CAPTURE: std::cell::RefCell<Option<Box<crate::vm::activation::FrameRecord>>> = const { std::cell::RefCell::new(None) };
}

/// Starts watching the process's environment inputs (`note_input`), for the rest of the process.
/// The watched run is `run`: its first input requests its termination (`note_watched_input`), so
/// the input's handler ends the run there rather than letting the run go on past an input. A core
/// build arms the edge the same way and captures at the input native's entry instead
/// (`input_edge_armed`, `store_input_capture`).
pub fn watch_inputs(run: crate::ExecutionInterrupt) {
    WATCHED_RUN.with(|watched| *watched.borrow_mut() = Some(run));
    INPUTS_WATCHED.store(true, Ordering::Relaxed);
}

/// Whether a watched run's input edge is armed: a native that reads an input captures its
/// caller chain at its entry, before it runs (`Vm::capture_input_native`).
#[inline]
pub fn input_edge_armed() -> bool {
    INPUTS_WATCHED.load(Ordering::Relaxed)
}

/// Keeps the chain a watched run captured at its input native's entry and ends the run there.
/// The chain is the only state a build keeps past its first input, so the run goes no further.
pub fn store_input_capture(chain: Box<crate::vm::activation::FrameRecord>) {
    INPUT_CAPTURE.with(|captured| *captured.borrow_mut() = Some(chain));
    request_watched_termination();
}

/// The chain a watched run captured at its input (`store_input_capture`), if it reached one.
pub fn take_input_capture() -> Option<Box<crate::vm::activation::FrameRecord>> {
    INPUT_CAPTURE.with(|captured| captured.borrow_mut().take())
}

/// Ends the watched run, if one is watched on this thread.
fn request_watched_termination() {
    WATCHED_RUN.with(|watched| {
        if let Some(run) = watched.borrow().as_ref() {
            run.request_termination();
        }
    });
}

/// Notes a read of an input from the environment while inputs are watched: one
/// relaxed load otherwise.
#[inline]
pub fn note_input() {
    if INPUTS_WATCHED.load(Ordering::Relaxed) {
        note_watched_input();
    }
}

/// The watched half of `note_input`, out of line so a run that watches nothing (every run but a
/// strict AOT build) pays the one load and branch, not the clock read. Its handler ends the
/// watched run: the run's effects past the input are never the build's to keep, since a build
/// keeps only the state before its first input. The input's name is not kept here: the caller
/// that ends the run names it.
#[cold]
#[inline(never)]
fn note_watched_input() {
    request_watched_termination();
}

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
    // One `CLOCK_REALTIME` read, as `SystemTime::now` makes, without its `Duration` steps.
    #[cfg(unix)]
    if !is_virtual() {
        let mut time: libc::timespec = unsafe { std::mem::zeroed() };
        // SAFETY: `time` is a valid out-pointer for the call.
        if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut time) } == 0 && time.tv_sec >= 0 {
            return time.tv_sec as f64 * 1000.0 + time.tv_nsec as f64 / 1e6;
        }
    }
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

/// Work done by every heap in the process, in typical-host nanoseconds (`now_work`).
static WORK: AtomicU64 = AtomicU64::new(0);

/// The engine work clock: a monotone count of work done, in typical-host nanoseconds, that
/// any policy may read in place of wall time (GC pacing, RC epochs, allocator release, code
/// aging). It advances at each collection by the mutator work since the previous one
/// (weight allocated) and the collection's own work (weight it traced), and between
/// collections by native calls (`note_step`), lane runs (`note_lane_run`) and lane loop
/// iterations (`note_backedges`, once per fuel batch of backward jumps), so code that
/// allocates little still moves it; identical on every run of the same program.
/// One relaxed load.
pub fn now_work() -> u64 {
    WORK.load(Ordering::Relaxed)
}

/// Slow-path steps (`note_step`) between charges to the work clock.
const STEP_BATCH: u32 = 1024;
/// Work, in typical-host nanoseconds, of one native library call (~100-400 instructions).
const WORK_PER_STEP: u64 = 60;
/// Work of one interpreter lane run, charged when it leaves (`note_lane_run`).
const WORK_PER_LANE_RUN: u64 = 400;

#[thread_local]
static STEPS_LEFT: std::cell::Cell<u32> = std::cell::Cell::new(STEP_BATCH);

/// One native library call (`Vm::call_native`): with lane runs (`note_lane_run`) the
/// mutator term of the work clock, charged only where execution already pays a call, so the
/// lane's hot loops and the general loop's steps pay nothing. A thread-local decrement;
/// every `STEP_BATCH` calls charge their work at once.
#[inline(always)]
pub fn note_step() {
    let left = STEPS_LEFT.get();
    if left > 1 {
        STEPS_LEFT.set(left - 1);
    } else {
        STEPS_LEFT.set(STEP_BATCH);
        charge_work(u64::from(STEP_BATCH) * WORK_PER_STEP);
    }
}

#[thread_local]
static LANE_RUNS_LEFT: std::cell::Cell<u32> = std::cell::Cell::new(LANE_RUN_BATCH);
/// Lane runs between charges to the work clock.
const LANE_RUN_BATCH: u32 = 256;
/// Work, in typical-host nanoseconds, of one loop iteration in the lane.
const WORK_PER_BACKEDGE: u64 = 10;
/// Credit `count` backward jumps a lane run took (its safepoint, once per fuel batch).
#[inline]
pub fn note_backedges(count: u32) {
    charge_work(u64::from(count) * WORK_PER_BACKEDGE);
}

/// One interpreter lane run that has left to the general loop (batched like `note_step`).
#[inline(always)]
pub fn note_lane_run() {
    let left = LANE_RUNS_LEFT.get();
    if left > 1 {
        LANE_RUNS_LEFT.set(left - 1);
    } else {
        LANE_RUNS_LEFT.set(LANE_RUN_BATCH);
        charge_work(u64::from(LANE_RUN_BATCH) * WORK_PER_LANE_RUN);
    }
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
