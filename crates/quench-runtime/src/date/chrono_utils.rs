//! Chrono utility functions for Date implementation.

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

/// Maximum and minimum time values per ECMAScript TimeClip spec (±8.64e15 ms).
const TIME_CLIP_LIMIT: f64 = 8.64e15;

/// Apply TimeClip per ECMAScript spec: clamp to ±8.64e15 ms, return NaN for out of range.
pub fn time_clip(ms: f64) -> f64 {
    if ms.is_nan() || ms.is_infinite() || ms.abs() > TIME_CLIP_LIMIT {
        f64::NAN
    } else {
        ms
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
#[allow(deprecated)]
pub fn ms_to_datetime(ms: f64) -> Option<NaiveDateTime> {
    if ms.is_nan() || ms.is_infinite() {
        return None;
    }
    let secs = (ms / 1000.0).trunc() as i64;
    let nanos = ((ms % 1000.0) * 1e6) as u32;
    NaiveDateTime::from_timestamp_opt(secs, nanos)
}

/// Create UTC milliseconds from components.
pub fn make_utc_ms(y: f64, m: f64, d: f64, h: f64, min: f64, s: f64, ms: f64) -> f64 {
    let year = y as i32;
    let month = (m as i32).saturating_sub(1).clamp(0, 11);
    let day = d as i32;
    let hour = h as i32;
    let minute = min as i32;
    let second = s as i32;
    let millis = ms as i32;

    let date = NaiveDate::from_ymd_opt(year, (month + 1) as u32, day as u32);
    let time = NaiveTime::from_hms_milli_opt(
        hour.clamp(0, 23) as u32,
        minute.clamp(0, 59) as u32,
        second.clamp(0, 59) as u32,
        millis.clamp(0, 999) as u32,
    );

    match (date, time) {
        (Some(d), Some(t)) => {
            let ndt = NaiveDateTime::new(d, t);
            ndt.and_utc().timestamp_millis() as f64
        }
        _ => f64::NAN,
    }
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
        return d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64;
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
