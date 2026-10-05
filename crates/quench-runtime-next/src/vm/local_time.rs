//! Local wall-time disambiguation shared by Date and Temporal.

use chrono::{DateTime, Duration, LocalResult, NaiveDateTime, Offset, TimeZone, Utc};

// Preserve the established Temporal offset probes around a skipped wall time.
const TRANSITION_OFFSET_PROBE_DAYS: i64 = 1;

pub(super) fn resolve_local_datetime<T: TimeZone>(
    local: NaiveDateTime,
    zone: &T,
    disambiguation: &str,
) -> Option<DateTime<Utc>> {
    match zone.from_local_datetime(&local) {
        LocalResult::Single(instant) => Some(instant.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, second) => {
            let first = first.with_timezone(&Utc);
            let second = second.with_timezone(&Utc);
            match disambiguation {
                "reject" => None,
                "later" => Some(first.max(second)),
                _ => Some(first.min(second)),
            }
        }
        LocalResult::None => {
            if disambiguation == "reject" {
                return None;
            }
            let probe = if disambiguation == "earlier" {
                local.checked_add_signed(Duration::days(TRANSITION_OFFSET_PROBE_DAYS))?
            } else {
                local.checked_sub_signed(Duration::days(TRANSITION_OFFSET_PROBE_DAYS))?
            };
            let offset = zone
                .offset_from_utc_datetime(&probe)
                .fix()
                .local_minus_utc();
            local
                .and_utc()
                .checked_sub_signed(Duration::seconds(i64::from(offset)))
        }
    }
}
