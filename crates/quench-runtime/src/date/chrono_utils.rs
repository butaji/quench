//! Chrono utility functions for Date implementation.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

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
/// on the last day of the prior month).
pub fn make_date_ms(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    ms: f64,
) -> f64 {
    time_clip(make_date_unclipped(
        year, month, day, hour, minute, second, ms,
    ))
}

fn make_date_unclipped(
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
    if !year_of.is_finite() || year_of.abs() > 10_000_000.0 || !day.is_finite() {
        return f64::NAN;
    }
    let month_of = month.rem_euclid(12.0) + 1.0;
    let first_day = days_from_civil(year_of as i64, month_of as i64, 1);
    let day_ms = first_day as f64 * MS_PER_DAY + (day - 1.0) * MS_PER_DAY;
    let time_ms = hour * MS_PER_HOUR + minute * MS_PER_MINUTE + second * MS_PER_SECOND + ms;
    day_ms + time_ms
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
    time_clip(make_date_unclipped(year, month, day, hour, minute, second, ms) - local_offset_ms())
}

fn local_offset_ms() -> f64 {
    (i64::from(local_tz_offset_minutes())).saturating_mul(60_000) as f64
}

/// Parse a date string according to ECMAScript Date.parse semantics.
pub fn parse_date_string(s: &str) -> f64 {
    if s.len() == 4 && s.bytes().all(|byte| byte.is_ascii_digit()) {
        let Some(year) = s.parse().ok() else {
            return f64::NAN;
        };
        return make_date_ms(year, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0);
    }
    if let Some(value) = parse_iso_utc(s) {
        return time_clip(value);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.timestamp_millis() as f64;
    }
    if let Some(value) = parse_local_iso(s) {
        return value;
    }
    if let Some(value) = parse_display_string(s) {
        return value;
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

fn parse_iso_utc(s: &str) -> Option<f64> {
    let (date, time) = s.strip_suffix('Z')?.split_once('T')?;
    let (year, rest) = split_iso_year(date)?;
    let (month, day) = split_iso_date(rest)?;
    let (hour, minute, second, ms) = split_iso_time(time)?;
    Some(make_date_ms(
        year,
        month - 1.0,
        day,
        hour,
        minute,
        second,
        ms,
    ))
}

fn split_iso_year(date: &str) -> Option<(f64, &str)> {
    let length = if date.starts_with('+') || date.starts_with('-') {
        7
    } else {
        4
    };
    let year = date.get(..length)?;
    if year == "-000000" {
        return None;
    }
    let rest = date.get(length..)?;
    Some((year.parse().ok()?, rest.strip_prefix('-')?))
}

fn split_iso_date(date: &str) -> Option<(f64, f64)> {
    let (month, day) = date.split_once('-')?;
    Some((month.parse().ok()?, day.parse().ok()?))
}

fn split_iso_time(time: &str) -> Option<(f64, f64, f64, f64)> {
    let (hour, rest) = time.split_once(':')?;
    let (minute, second) = rest.split_once(':').unwrap_or((rest, "0"));
    let (second, fraction) = second.split_once('.').unwrap_or((second, "0"));
    let ms = format!("{fraction:0<3}").get(..3)?.parse().ok()?;
    Some((
        hour.parse().ok()?,
        minute.parse().ok()?,
        second.parse().ok()?,
        ms,
    ))
}

fn parse_local_iso(s: &str) -> Option<f64> {
    let dt = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    Some(dt.and_utc().timestamp_millis() as f64 - local_offset_ms())
}

fn parse_display_string(s: &str) -> Option<f64> {
    let formats = ["%a %b %d %Y %H:%M:%S GMT%z", "%a, %d %b %Y %H:%M:%S GMT"];
    for format in formats {
        if let Ok(dt) = chrono::DateTime::parse_from_str(s, format) {
            return Some(dt.timestamp_millis() as f64);
        }
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, format) {
            return Some(dt.and_utc().timestamp_millis() as f64);
        }
    }
    None
}

/// Get local timezone offset in minutes.
pub fn local_tz_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

/// Extract date/time components from milliseconds in local time.
pub fn local_components(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    (!time_clip(ms).is_nan())
        .then(|| fields_from_ms(ms + local_offset_ms()))
        .flatten()
}

/// Extract date/time components from milliseconds in UTC.
pub fn utc_components(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    (!time_clip(ms).is_nan())
        .then(|| fields_from_ms(ms))
        .flatten()
}

/// Weekday number with Sunday represented by zero.
pub fn weekday(ms: f64) -> Option<usize> {
    (!time_clip(ms).is_nan())
        .then(|| ((ms.trunc() as i64).div_euclid(MS_PER_DAY as i64) + 4).rem_euclid(7) as usize)
}

fn fields_from_ms(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    if !ms.is_finite() {
        return None;
    }
    let whole = ms.trunc() as i64;
    let days = whole.div_euclid(MS_PER_DAY as i64);
    let time = whole.rem_euclid(MS_PER_DAY as i64);
    let (year, month, day) = civil_from_days(days);
    let hour = (time / MS_PER_HOUR as i64) as u32;
    let minute = ((time / MS_PER_MINUTE as i64) % 60) as u32;
    let second = ((time / MS_PER_SECOND as i64) % 60) as u32;
    Some((
        year as i32,
        month as u32,
        day as u32,
        hour,
        minute,
        second,
        (time % 1000) as u32,
    ))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month = (5 * doy + 2) / 153;
    (
        year + i64::from(month >= 10),
        month + if month < 10 { 3 } else { -9 },
        doy - (153 * month + 2) / 5 + 1,
    )
}
