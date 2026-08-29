// Date component formatting for `Intl.DateTimeFormat.prototype.format`.
// Implements en-US patterns for the most common option combinations:
// - year/month/day (numeric/2-digit): `M/d/y`
// - year/month (short/long) + day: `MMM d, y` / `MMMM d, y`
// - weekday + date components: `WD, M/d/y` style
// - year + era: handled separately by the existing proleptic-year path.
// All times use a `h:mm:ss` style when minute/second are present.

use crate::value::Value as RuntimeValue;

pub(crate) fn date_format_result(slots: &[(String, RuntimeValue)], number: f64) -> Option<String> {
    let has_year = lookup_slot_string(slots, "year").is_some();
    let has_month = lookup_slot_string(slots, "month").is_some();
    let has_day = lookup_slot_string(slots, "day").is_some();
    let has_weekday = lookup_slot_string(slots, "weekday").is_some();
    let has_hour = lookup_slot_string(slots, "hour").is_some();
    let has_minute = lookup_slot_string(slots, "minute").is_some();
    let has_second = lookup_slot_string(slots, "second").is_some();
    let has_time = has_hour || has_minute || has_second;
    let time_zone = lookup_slot_string(slots, "timeZone");
    let is_utc = time_zone.as_deref() == Some("UTC") || time_zone.is_none();
    let comps = if is_utc {
        crate::date::chrono_utils::utc_components(number)
    } else {
        crate::date::chrono_utils::local_components(number)
    };
    let (year, month, day, hour, minute, second, ms) = comps?;
    if !has_year && !has_month && !has_day && !has_weekday {
        return has_time.then(|| {
            let mut text = compose_time_string(slots, hour, minute, second);
            append_fractional(&mut text, slots, ms);
            text
        });
    }
    let date_str = compose_date_string(slots, year, month, day, number, is_utc);
    if !has_time {
        return Some(date_str);
    }
    let time_str = compose_time_string(slots, hour, minute, second);
    if date_str.is_empty() {
        let mut text = time_str;
        append_fractional(&mut text, slots, ms);
        return Some(text);
    }
    let mut text = time_str;
    append_fractional(&mut text, slots, ms);
    Some(format!("{} {}", date_str, text))
}

/// Format Temporal plain values directly from their calendar fields. Unlike
/// Date values, these fields are not subject to TimeClip and must ignore the
/// formatter's time-zone conversion.
pub(crate) fn temporal_date_format_result(
    slots: &[(String, RuntimeValue)],
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
) -> Option<String> {
    let has_year = lookup_slot_string(slots, "year").is_some();
    let has_month = lookup_slot_string(slots, "month").is_some();
    let has_day = lookup_slot_string(slots, "day").is_some();
    let has_weekday = lookup_slot_string(slots, "weekday").is_some();
    let has_time = lookup_slot_string(slots, "hour").is_some()
        || lookup_slot_string(slots, "minute").is_some()
        || lookup_slot_string(slots, "second").is_some();
    if !has_year && !has_month && !has_day && !has_weekday && !has_time {
        return None;
    }
    let weekday = temporal_weekday(year, month, day);
    let date_str = compose_date_string_with_weekday(
        slots, year, month, day, weekday,
    );
    if !has_time {
        return Some(date_str);
    }
    let mut time_str = compose_time_string(slots, hour, minute, second);
    append_fractional(&mut time_str, slots, millis);
    if date_str.is_empty() {
        Some(time_str)
    } else {
        Some(format!("{date_str} {time_str}"))
    }
}

fn append_fractional(text: &mut String, slots: &[(String, RuntimeValue)], millis: u32) {
    let Some(digits) = slots
        .iter()
        .find_map(|(name, value)| {
            (name == "fractionalSecondDigits").then_some(match value {
                RuntimeValue::String(value) => value.parse::<u32>().ok(),
                RuntimeValue::Number(value) => Some(*value as u32),
                _ => None,
            })
        })
        .flatten()
    else {
        return;
    };
    if !(1..=3).contains(&digits) {
        return;
    }
    let fraction = millis / 10_u32.pow(3 - digits);
    let suffix_start = text
        .strip_suffix(" AM")
        .or_else(|| text.strip_suffix(" PM"))
        .map_or(text.len(), str::len);
    text.insert_str(
        suffix_start,
        &format!(".{fraction:0width$}", width = digits as usize),
    );
}

fn lookup_slot_string(slots: &[(String, RuntimeValue)], key: &str) -> Option<String> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            RuntimeValue::String(value) => Some(value.clone()),
            _ => None,
        })
}

fn compose_date_string(
    slots: &[(String, RuntimeValue)],
    year: i32,
    month: u32,
    day: u32,
    ms: f64,
    is_utc: bool,
) -> String {
    let weekday = crate::date::chrono_utils::weekday(ms).unwrap_or(0);
    compose_date_string_with_weekday(slots, year, month, day, weekday)
}

fn compose_date_string_with_weekday(
    slots: &[(String, RuntimeValue)],
    year: i32,
    month: u32,
    day: u32,
    weekday: usize,
) -> String {
    let year_style = lookup_slot_string(slots, "year");
    let month_style = lookup_slot_string(slots, "month");
    let day_style = lookup_slot_string(slots, "day");
    let weekday_style = lookup_slot_string(slots, "weekday");
    let month_str = month_style.as_deref().map(|s| format_month_value(s, month));
    let day_str = day_style.as_deref().map(|s| format_day_value(s, day));
    let year_str = year_style.as_deref().map(|s| format_year_value(s, year));
    let weekday_str = weekday_style
        .as_deref()
        .map(|s| format_weekday_value(s, weekday));
    let month_is_name = month_str
        .as_deref()
        .is_some_and(|m| !m.chars().all(|c| c.is_ascii_digit()));
    let mut parts: Vec<String> = Vec::new();
    if let Some(wd) = weekday_str {
        parts.push(wd);
    }
    if let Some(m) = month_str {
        parts.push(m);
    }
    if let Some(d) = day_str {
        parts.push(d);
    }
    if let Some(y) = year_str {
        parts.push(y);
    }
    if month_is_name {
        // Component options are independent; never assume a month implies
        // both day and year. Preserve the locale order without indexing
        // absent components.
        let mut out = String::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                if index == 2 && parts.len() >= 3 {
                    out.push_str(", ");
                } else {
                    out.push(' ');
                }
            }
            out.push_str(part);
        }
        return out;
    }
    if !month_is_name
        && parts.len() >= 3
        && year_style.is_some()
        && month_style.is_some()
        && day_style.is_some()
    {
        return format!("{}/{}/{}", parts[0], parts[1], parts[2]);
    }
    let mut out = String::new();
    let mut first = true;
    for p in &parts {
        if !first {
            out.push(' ');
        }
        out.push_str(p);
        first = false;
    }
    out
}

fn format_year_value(style: &str, year: i32) -> String {
    let abs = if year <= 0 { 1 - year } else { year };
    if style == "2-digit" {
        format!("{:02}", abs % 100)
    } else {
        abs.to_string()
    }
}

fn format_month_value(style: &str, month: u32) -> String {
    let idx = (month.saturating_sub(1) as usize).min(11);
    match style {
        "2-digit" => format!("{:02}", month),
        "numeric" => month.to_string(),
        "short" => [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ][idx]
            .to_string(),
        "long" => [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ][idx]
            .to_string(),
        "narrow" => ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"][idx].to_string(),
        _ => month.to_string(),
    }
}

fn format_day_value(style: &str, day: u32) -> String {
    if style == "2-digit" {
        format!("{:02}", day)
    } else {
        day.to_string()
    }
}

fn format_weekday_value(style: &str, wd: usize) -> String {
    match style {
        "short" => ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][wd].to_string(),
        "long" => [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ][wd]
            .to_string(),
        "narrow" => ["S", "M", "T", "W", "T", "F", "S"][wd].to_string(),
        _ => String::new(),
    }
}

fn temporal_weekday(year: i32, month: u32, day: u32) -> usize {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let month_index = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_index + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    ((days.rem_euclid(7) + 4) % 7) as usize
}

fn compose_time_string(
    slots: &[(String, RuntimeValue)],
    hour: u32,
    minute: u32,
    second: u32,
) -> String {
    let hour_style = lookup_slot_string(slots, "hour");
    let minute_style = lookup_slot_string(slots, "minute");
    let second_style = lookup_slot_string(slots, "second");
    let day_period_style = lookup_slot_string(slots, "dayPeriod");
    let hour12 = slots
        .iter()
        .find(|(name, _)| name == "hour12")
        .and_then(|(_, value)| match value {
            RuntimeValue::Boolean(value) => Some(*value),
            _ => None,
        })
        .unwrap_or(false);
    let has_hour = hour_style.is_some();
    let has_minute = minute_style.is_some();
    let has_second = second_style.is_some();
    let hour_str = hour_style
        .as_deref()
        .map(|s| format_hour_value(s, hour, hour12));
    let minute_str = minute_style
        .as_deref()
        .map(|s| format_minute_value(s, minute, has_hour || has_second));
    let second_str = second_style
        .as_deref()
        .map(|s| format_second_value(s, second, has_hour || has_minute));
    let mut parts: Vec<String> = Vec::new();
    if has_hour {
        if let Some(h) = hour_str {
            parts.push(h);
        }
    }
    if has_minute {
        if let Some(m) = minute_str {
            parts.push(m);
        }
    }
    if has_second {
        if let Some(s) = second_str {
            parts.push(s);
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut first = true;
    for p in &parts {
        if !first {
            out.push(':');
        }
        out.push_str(p);
        first = false;
    }
    if let Some(dp_style) = day_period_style {
        if let Some(dp) = day_period_name_from_style(&dp_style, hour) {
            out.push(' ');
            out.push_str(&dp);
        }
    } else if hour12 {
        out.push(' ');
        out.push_str(if hour < 12 { "AM" } else { "PM" });
    }
    out
}

fn format_hour_value(style: &str, hour: u32, hour12: bool) -> String {
    if hour12 {
        let h12 = if hour == 0 {
            12
        } else if hour > 12 {
            hour - 12
        } else {
            hour
        };
        if style == "2-digit" {
            format!("{:02}", h12)
        } else {
            h12.to_string()
        }
    } else {
        if style == "2-digit" {
            format!("{:02}", hour)
        } else {
            hour.to_string()
        }
    }
}

fn format_minute_value(style: &str, minute: u32, pad: bool) -> String {
    if style == "2-digit" || pad {
        format!("{:02}", minute)
    } else {
        minute.to_string()
    }
}

fn format_second_value(style: &str, second: u32, pad: bool) -> String {
    if style == "2-digit" || pad {
        format!("{:02}", second)
    } else {
        second.to_string()
    }
}

fn day_period_name_from_style(style: &str, hour: u32) -> Option<String> {
    let name = match style {
        "short" => match hour {
            0..=5 | 18..=23 => "at night",
            6..=11 => "in the morning",
            12 => "noon",
            13..=17 => "in the afternoon",
            _ => return None,
        },
        "long" => match hour {
            0..=5 | 18..=23 => "at night",
            6..=11 => "in the morning",
            12 => "noon",
            13..=17 => "in the afternoon",
            _ => return None,
        },
        "narrow" => match hour {
            0..=11 => "AM",
            _ => "PM",
        },
        _ => return None,
    };
    Some(name.to_string())
}
