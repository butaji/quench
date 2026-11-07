//! The local time zone (`TZ`, else the system's), parsed once per value of `TZ`: every
//! local-time conversion reads it, and reading the environment and parsing the zone took a
//! fifth of a Date-heavy run.
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counts the host's changes of `TZ` (`local_timezone_changed`).
static GENERATION: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The zone this thread parsed, with the generation it read `TZ` in.
    static ZONE: Cell<Option<(u64, Option<chrono_tz::Tz>)>> = const { Cell::new(None) };
}

/// The zone `TZ` names, or without `TZ` the system's, if either names one.
pub(super) fn local_timezone() -> Option<chrono_tz::Tz> {
    let generation = GENERATION.load(Ordering::Acquire);
    ZONE.with(|zone| match zone.get() {
        Some((seen, parsed)) if seen == generation => parsed,
        _ => {
            let parsed = match std::env::var("TZ") {
                Ok(name) => name.strip_prefix(':').unwrap_or(&name).parse().ok(),
                Err(_) => system_timezone(),
            };
            zone.set(Some((generation, parsed)));
            parsed
        }
    })
}

/// The zone `/etc/localtime` links to, as a C library reads it without `TZ`. chrono's own
/// `Local` checks the clock and the file on every conversion.
fn system_timezone() -> Option<chrono_tz::Tz> {
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let target = target.to_str()?;
    target.rsplit_once("zoneinfo/").map_or(target, |(_, name)| name).parse().ok()
}

/// Tells every thread to read `TZ` again: the host changed it (`process.env.TZ`).
pub fn local_timezone_changed() {
    GENERATION.fetch_add(1, Ordering::Release);
}
