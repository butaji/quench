//! The boolean `QUENCH_*` A/B and diagnostic switches, each read from the environment once and
//! named here, so a run's configuration is one list. A switch is on when its variable is set to
//! anything but `0`; every one defaults to off, except `QUENCH_FLOAT_SLOTS` under UltraJIT. (`trace` owns the named diagnostic channels and
//! development experiments, `QUENCH_TRACE` and `QUENCH_DEV`.)

/// Whether the environment variable `name` switches something on: set, and not `0`.
fn named(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| value != "0")
}

#[cfg(test)]
thread_local! {
    /// A test's override of `float_slots`, for the thread that sets it.
    static FLOAT_SLOTS_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `QUENCH_FLOAT_SLOTS`: a slot whose owner's type is Number holds its Number as the raw IEEE
/// double (`value_vec::encode_slot`), so native code reads and writes it with one load or store
/// and no tag test. Slot types are kept only where native code runs (`ValueArena::track`).
/// Read per slot decode (`value_vec::read_type`), so the initialized value is one inlined load.
#[inline(always)]
pub fn float_slots() -> bool {
    #[cfg(test)]
    if FLOAT_SLOTS_FOR_TEST.get() { return true; }
    // One byte loaded and tested once on the decided path, not a `OnceLock`'s state and value:
    // every field read decodes through here (`value_vec::read_type`). Only the unread state takes
    // the cold reader, which stores the answer; OFF and ON are then one compare, not two.
    let state = FLOAT_SLOTS.load(std::sync::atomic::Ordering::Relaxed);
    if state == SWITCH_UNREAD { return read_float_slots(); }
    state == SWITCH_ON
}

/// `float_slots`'s state: `SWITCH_UNREAD` until the environment is read, then off or on.
static FLOAT_SLOTS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(SWITCH_UNREAD);
const SWITCH_UNREAD: u8 = 0;
const SWITCH_OFF: u8 = 1;
const SWITCH_ON: u8 = 2;

/// `QUENCH_FLOAT_SLOTS` where it is set; otherwise on where UltraJIT runs (the default mode,
/// `--u-jit`, `--q-jit`), whose lowering stores and reads raw doubles in place
/// (`staged::property::StoreArm::raw_double`), and off for the interpreter and BaseJIT alone, which
/// decode a slot's owner on every read. A test build keeps the variable's reading alone, so
/// a test that wants them says so (`with_float_slots`).
#[cold]
#[inline(never)]
fn read_float_slots() -> bool {
    let on = if cfg!(test) || std::env::var_os("QUENCH_FLOAT_SLOTS").is_some() {
        named("QUENCH_FLOAT_SLOTS")
    } else {
        use crate::jit_mode::JitMode;
        matches!(JitMode::process(), JitMode::Full | JitMode::UltraOnly | JitMode::QuickOnly)
    };
    FLOAT_SLOTS.store(if on { SWITCH_ON } else { SWITCH_OFF }, std::sync::atomic::Ordering::Relaxed);
    on
}

/// Runs `body` with `float_slots` on for this thread.
#[cfg(test)]
pub(crate) fn with_float_slots<R>(body: impl FnOnce() -> R) -> R {
    FLOAT_SLOTS_FOR_TEST.set(true);
    let result = body();
    FLOAT_SLOTS_FOR_TEST.set(false);
    result
}

/// `QUENCH_PROFILE_RETAIN_BYTECODE`: a function-level profile keeps every replayable function's
/// instruction records after compilation, where they would otherwise be released.
#[cfg(feature = "profile-aggregate")]
pub(crate) fn profile_retain_bytecode() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| named("QUENCH_PROFILE_RETAIN_BYTECODE"))
}

/// `QUENCH_CLIF_CENSUS`: prints each lane lowering's CLIF per bytecode op (the CLIF-per-op budget).
pub fn clif_census() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| named("QUENCH_CLIF_CENSUS"))
}

/// `QUENCH_CLIF_CENSUS=sample`: the census also prints the CLIF of the first few lowerings of
/// each op (`ClifCensus::sample`).
pub fn clif_census_sample() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("QUENCH_CLIF_CENSUS").is_ok_and(|value| value == "sample"))
}

/// `QUENCH_GROWTH_ALLOCATOR`: the allocator the growth primitive benchmark routes through,
/// `Some(true)` for `quench`, `Some(false)` for `system`, `None` where it names neither (the
/// benchmark then refuses to run).
#[cfg(all(test, feature = "bench-primitives", feature = "bench-alloc-routes"))]
pub(crate) fn growth_allocator() -> Option<bool> {
    match std::env::var("QUENCH_GROWTH_ALLOCATOR").as_deref() {
        Ok("quench") => Some(true),
        Ok("system") => Some(false),
        _ => None,
    }
}

/// `QUENCH_MEMORY_PAGE_SIZE`: the page size a memory snapshot reports its slabs in, where set
/// to a power of two.
#[cfg(feature = "profile-memory")]
pub(crate) fn memory_page_size() -> Option<usize> {
    std::env::var("QUENCH_MEMORY_PAGE_SIZE").ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|page_size| page_size.is_power_of_two())
}
