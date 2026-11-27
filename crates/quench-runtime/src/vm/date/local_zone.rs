//! The local time zone (`TZ`, else the system's), parsed once per value of `TZ`: every
//! local-time conversion reads it, and reading the environment and parsing the zone took a
//! fifth of a Date-heavy run.
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counts the host's changes of `TZ` (`local_timezone_changed`).
static GENERATION: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The zone this thread parsed, with the generation it read `TZ` in.
    #[cfg(feature = "tzdb")]
    static ZONE: Cell<Option<(u64, Option<chrono_tz::Tz>)>> = const { Cell::new(None) };
}

/// The zone `TZ` names, or without `TZ` the system's, if either names one.
#[cfg(feature = "tzdb")]
pub(super) fn local_timezone() -> Option<chrono_tz::Tz> {
    crate::clock::note_input();
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
#[cfg(feature = "tzdb")]
fn system_timezone() -> Option<chrono_tz::Tz> {
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let target = target.to_str()?;
    target.rsplit_once("zoneinfo/").map_or(target, |(_, name)| name).parse().ok()
}

/// Tells every thread to read `TZ` again: the host changed it (`process.env.TZ`).
pub fn local_timezone_changed() {
    GENERATION.fetch_add(1, Ordering::Release);
    // Without the time zone database, `chrono::Local` reads the C library's zone: reset it so its
    // next conversion observes the new `TZ` (`tzset` is what `localtime_r` requires after a change).
    #[cfg(all(not(feature = "tzdb"), unix))]
    unsafe {
        unsafe extern "C" {
            fn tzset();
        }
        tzset();
    }
}

/// The longest gap between two lookups of one offset across which the offset is taken to
/// hold: no zone changes its offset and changes it back within a day (V8's date cache takes
/// 19 days, `kDefaultDSTDeltaInSec`).
const OFFSET_SPAN_STEP_MS: i64 = 86_400_000;

thread_local! {
    /// The span of timestamps the last lookups found one offset at: the generation of `TZ`
    /// they read, the first and last timestamp, and the offset.
    static OFFSET_SPAN: Cell<(u64, i64, i64, i32)> = const { Cell::new((u64::MAX, 0, -1, 0)) };
}

/// The local offset at `timestamp` that `lookup` finds, read from the span of timestamps
/// earlier lookups found it constant over where `timestamp` falls in it: each lookup of the
/// same offset within `OFFSET_SPAN_STEP_MS` of the span widens it. Only a parsed zone's
/// offsets are kept; chrono's `Local` rereads the system's on each conversion.
pub(super) fn cached_offset(timestamp: i64, lookup: fn(i64) -> Option<i32>) -> Option<i32> {
    let generation = GENERATION.load(Ordering::Acquire);
    let (seen, first, last, offset) = OFFSET_SPAN.get();
    let current = seen == generation;
    if current && first <= timestamp && timestamp <= last {
        return Some(offset);
    }
    let found = lookup(timestamp)?;
    // Only a parsed zone's offsets are kept (chrono's `Local` rereads the system's each time).
    #[cfg(feature = "tzdb")]
    if local_timezone().is_none() {
        return Some(found);
    }
    let extends = current && found == offset;
    let (first, last) = if extends && timestamp > last && timestamp - last <= OFFSET_SPAN_STEP_MS {
        (first, timestamp)
    } else if extends && timestamp < first && first - timestamp <= OFFSET_SPAN_STEP_MS {
        (timestamp, last)
    } else {
        (timestamp, timestamp)
    };
    OFFSET_SPAN.set((generation, first, last, found));
    Some(found)
}
