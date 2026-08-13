//! Date string formatting builtins.

use crate::{execute::VmError, ops::Builtin, value::Value};

use super::{chrono_utils, extract_time, setter};

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn execute(builtin: Builtin, receiver: Option<&Value>) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::DateToString => {
            setter::time_value(receiver).map(|_| date_string(extract_time(receiver)))
        }
        Builtin::DateToDateString => {
            setter::time_value(receiver).map(|_| date_part(extract_time(receiver)))
        }
        Builtin::DateToTimeString => {
            setter::time_value(receiver).map(|_| time_part(extract_time(receiver)))
        }
        Builtin::DateToUTCString => {
            setter::time_value(receiver).map(|_| utc_string(extract_time(receiver)))
        }
        Builtin::DateToISOString => iso_string(receiver),
        _ => return None,
    };
    Some(result.map(Value::String))
}

pub fn date_string(ms: f64) -> String {
    if !valid(ms) {
        return "Invalid Date".to_string();
    }
    format!("{} {}", date_part(ms), time_part(ms))
}

fn date_part(ms: f64) -> String {
    local_fields(ms).map_or_else(invalid, |(y, m, d, _, _, _, day)| {
        format!(
            "{} {} {:02} {}",
            DAYS[day],
            MONTHS[(m - 1) as usize],
            d,
            display_year(y)
        )
    })
}

fn time_part(ms: f64) -> String {
    local_fields(ms).map_or_else(invalid, |(_, _, _, h, min, sec, _)| {
        let offset = chrono_utils::local_tz_offset_minutes();
        let sign = if offset >= 0 { '+' } else { '-' };
        format!(
            "{h:02}:{min:02}:{sec:02} GMT{sign}{:02}{:02}",
            offset.abs() / 60,
            offset.abs() % 60
        )
    })
}

fn utc_string(ms: f64) -> String {
    utc_fields(ms).map_or_else(invalid, |(y, m, d, h, min, sec, day, _)| {
        format!(
            "{}, {:02} {} {} {h:02}:{min:02}:{sec:02} GMT",
            DAYS[day],
            d,
            MONTHS[(m - 1) as usize],
            display_year(y)
        )
    })
}

fn iso_string(receiver: Option<&Value>) -> Result<String, VmError> {
    let ms = setter::time_value(receiver)?;
    utc_fields(ms).map_or_else(
        || Err(crate::value::error::throw_range_error("Invalid time value")),
        |(y, m, d, h, min, sec, _, milli)| {
            Ok(format!(
                "{}-{m:02}-{d:02}T{h:02}:{min:02}:{sec:02}.{milli:03}Z",
                iso_year(y)
            ))
        },
    )
}

fn local_fields(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, usize)> {
    let (y, m, d, h, min, sec, _) = chrono_utils::local_components(ms)?;
    let day =
        chrono_utils::weekday(ms + chrono_utils::local_tz_offset_minutes() as f64 * 60_000.0)?;
    Some((y, m, d, h, min, sec, day))
}

fn utc_fields(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, usize, u32)> {
    let (y, m, d, h, min, sec, milli) = chrono_utils::utc_components(ms)?;
    let day = chrono_utils::weekday(ms)?;
    Some((y, m, d, h, min, sec, day, milli))
}

fn valid(ms: f64) -> bool {
    utc_fields(ms).is_some()
}
fn invalid() -> String {
    "Invalid Date".to_string()
}
fn display_year(year: i32) -> String {
    if year < 0 {
        format!("-{:04}", year.unsigned_abs())
    } else {
        format!("{year:04}")
    }
}
fn iso_year(year: i32) -> String {
    if (0..=9999).contains(&year) {
        display_year(year)
    } else {
        format!("{year:+07}")
    }
}
