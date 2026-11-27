//! The boolean `QUENCH_*` A/B and diagnostic switches, each read from the environment once and
//! named here, so a run's configuration is one list. A switch is on when its variable is set to
//! anything but `0`; every one defaults to off. (`trace` owns the named diagnostic channels and
//! development experiments, `QUENCH_TRACE` and `QUENCH_DEV`.)

/// Whether the environment variable `name` switches something on: set, and not `0`.
fn named(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| value != "0")
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
