//! The boolean `QUENCH_*` A/B and diagnostic switches, each read from the environment once and
//! named here, so a run's configuration is one list. A switch is on when its variable is set to
//! anything but `0`; every one defaults to off. (`trace` owns the named diagnostic channels and
//! development experiments, `QUENCH_TRACE` and `QUENCH_DEV`.)

use std::sync::OnceLock;

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
/// Inline: every slot decode reads it (`value_vec::read_type`), so its set flag is one load there.
#[inline]
pub(crate) fn float_slots() -> bool {
    #[cfg(test)]
    if FLOAT_SLOTS_FOR_TEST.get() { return true; }
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| named("QUENCH_FLOAT_SLOTS"))
}

/// Runs `body` with `float_slots` on for this thread.
#[cfg(test)]
pub(crate) fn with_float_slots<R>(body: impl FnOnce() -> R) -> R {
    FLOAT_SLOTS_FOR_TEST.set(true);
    let result = body();
    FLOAT_SLOTS_FOR_TEST.set(false);
    result
}
