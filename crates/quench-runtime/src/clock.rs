//! The clocks the engine and its host read, in one place (M6, P6 work clock): the operating
//! system's clocks for the JavaScript-visible reads, and the engine work clock (`now_work`) that
//! the policies read in place of wall time.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Whether a build-time evaluation watches the program's inputs from its environment
/// (`watch_inputs`): strict AOT runs an entry at build time, and a run that read the clock
/// (`CapabilityId::ClockMillis`), the local time zone or `Math.random` computed values a later
/// run would not. The host's own clock reads (its event loop's) are no input of the program.
static INPUTS_WATCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! {
    /// The execution a watched run is on, which the first input ends (`watch_inputs`). The run
    /// and its clock reads share the thread that watches them.
    static WATCHED_RUN: std::cell::RefCell<Option<crate::ExecutionInterrupt>> = const { std::cell::RefCell::new(None) };
}

/// Starts watching the process's environment inputs (`note_input`), for the rest of the process.
/// The watched run is `run`: its first input requests its termination (`note_watched_input`), so
/// the input's handler ends the run there rather than letting the run go on past an input. An input
/// native's own entry captures instead (`input_edge_armed`): its run returns the caller chain as its
/// outcome (`JsError::captured`), with no interrupt flag set.
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

/// The monotonic instant now.
pub fn instant_now() -> Instant {
    Instant::now()
}

/// The wall-clock time now.
pub fn system_now() -> SystemTime {
    SystemTime::now()
}

/// Milliseconds since the Unix epoch, as `Date.now` reads them.
pub fn unix_millis() -> f64 {
    // One `CLOCK_REALTIME` read, as `SystemTime::now` makes, without its `Duration` steps.
    #[cfg(unix)]
    {
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
