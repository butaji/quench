//! Chrono utility functions for Date implementation.

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Timelike, Utc};

/// Maximum and minimum time values per ECMAScript TimeClip spec (±8.64e15 ms).
const TIME_CLIP_LIMIT: f64 = 8.64e15;

/// Apply TimeClip per ECMAScript spec: clamp to ±8.64e15 ms, return NaN for out of range.
pub fn time_clip(ms: f64) -> f64 {
    if ms.is_nan() || ms.is_infinite() || ms.abs() > TIME_CLIP_LIMIT {
        f64::NAN
    } else {
        ms.trunc()
    }
}

/// Get current time in milliseconds since Unix epoch.
pub fn current_time_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(f64::NAN)
}

/// Convert milliseconds-since-epoch to NaiveDateTime (UTC).
pub fn ms_to_datetime(ms: f64) -> Option<NaiveDateTime> {
    if ms.is_nan() || ms.is_infinite() {
        return None;
    }
    let secs = (ms / 1000.0).trunc() as i64;
    let nanos = ((ms % 1000.0) * 1e6) as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos).map(|value| value.naive_utc())
}

const MS_PER_HOUR: f64 = 3_600_000.0;
const MS_PER_MINUTE: f64 = 60_000.0;
const MS_PER_SECOND: f64 = 1_000.0;
const MS_PER_DAY: f64 = 86_400_000.0;

/// Year values 0–99 are interpreted as 1900+year in the Date constructor and
/// Date.UTC (not in the setters).
pub fn normalize_constructor_year(year: f64) -> f64 {
    let year = year.trunc();
    if (0.0..=99.0).contains(&year) {
        1900.0 + year
    } else {
        year
    }
}

/// MakeDay + MakeTime + MakeDate in epoch milliseconds (month is 0-indexed).
/// Month/day/hour overflow rolls over per ECMAScript (e.g. `setDate(0)` lands
/// on the last day of the prior month); out-of-chrono-range results yield NaN.
pub fn make_date_ms(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    ms: f64,
) -> f64 {
    if year.is_nan()
        || month.is_nan()
        || day.is_nan()
        || hour.is_nan()
        || minute.is_nan()
        || second.is_nan()
        || ms.is_nan()
    {
        return f64::NAN;
    }
    let year = year.trunc();
    let month = month.trunc();
    let day = day.trunc();
    let hour = hour.trunc();
    let minute = minute.trunc();
    let second = second.trunc();
    let ms = ms.trunc();
    let year_of = year + (month / 12.0).floor();
    let month_of = month.rem_euclid(12.0) + 1.0;
    let Some(first) = NaiveDate::from_ymd_opt(year_of as i32, month_of as u32, 1) else {
        return f64::NAN;
    };
    let first_ms = first
        .and_hms_opt(0, 0, 0)
        .map_or(f64::NAN, |t| t.and_utc().timestamp_millis() as f64);
    let day_ms = first_ms + (day.trunc() - 1.0) * MS_PER_DAY;
    let time_ms = hour * MS_PER_HOUR + minute * MS_PER_MINUTE + second * MS_PER_SECOND + ms;
    time_clip(day_ms + time_ms)
}

/// Interpret calendar components in the current local time zone.
pub fn make_local_ms(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    ms: f64,
) -> f64 {
    time_clip(make_date_ms(year, month, day, hour, minute, second, ms) - local_offset_ms())
}

fn local_offset_ms() -> f64 {
    (i64::from(local_tz_offset_minutes())).saturating_mul(60_000) as f64
}

/// Parse a date string according to ECMAScript Date.parse semantics.
pub fn parse_date_string(s: &str) -> f64 {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.timestamp_millis() as f64;
    }
    let formats = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y/%m/%d %H:%M:%S",
        "%Y-%m-%d",
        "%b %d, %Y",
        "%B %d, %Y",
    ];
    for fmt in &formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return dt.and_utc().timestamp_millis() as f64;
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d") {
        return d
            .and_hms_opt(0, 0, 0)
            .map_or(f64::NAN, |value| value.and_utc().timestamp_millis() as f64);
    }
    f64::NAN
}

/// Get local timezone offset in minutes.
pub fn local_tz_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

/// Extract date/time components from milliseconds in local time.
pub fn local_components(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    let dt = ms_to_datetime(ms)?;
    let offset = chrono::Duration::minutes(local_tz_offset_minutes() as i64);
    let local_dt = dt + offset;
    Some((
        local_dt.year(),
        local_dt.month(),
        local_dt.day(),
        local_dt.hour(),
        local_dt.minute(),
        local_dt.second(),
        local_dt.nanosecond() / 1_000_000,
    ))
}

/// Extract date/time components from milliseconds in UTC.
pub fn utc_components(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    let dt = ms_to_datetime(ms)?;
    Some((
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        dt.nanosecond() / 1_000_000,
    ))
}
