fn format_result(arguments: &[Value], slots: &[(String, Value)]) -> Result<Value, VmError> {
    let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
    let has_date = slot_string(slots, "year").is_some()
        || slot_string(slots, "month").is_some()
        || slot_string(slots, "day").is_some()
        || slot_string(slots, "weekday").is_some();
    let has_era = slot_string(slots, "era").is_some();
    if has_date && !has_era {
        if let Some(value) = date_format_result(slots, number) {
            return Ok(Value::String(value));
        }
    }
    if let Some(value) = hour_day_period_format(slots, number) {
        return Ok(Value::String(value));
    }
    if let Some(value) = day_period_format(slots, number) {
        return Ok(Value::String(value));
    }
    if let Some(value) = proleptic_year_format(slots, number) {
        return Ok(Value::String(value));
    }
    if let Some(value) = fractional_format(slots, number) {
        return Ok(Value::String(value));
    }
    Ok(Value::String(range_text(number)))
}

fn parts_result(arguments: &[Value], slots: &[(String, Value)]) -> Result<Value, VmError> {
    let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if let Some(value) = hour_day_period_parts(slots, number) {
        return Ok(make_array(value));
    }
    if let Some(value) = day_period_parts(slots, number) {
        return Ok(make_array(value));
    }
    if let Some(value) = fractional_parts(slots, number) {
        return Ok(make_array(value));
    }
    let value = range_text(number);
    Ok(make_array(vec![literal_part(&value)]))
}

fn fractional_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let digits = slot_number(slots, "fractionalSecondDigits").unwrap_or(0.0) as u32;
    slot_string(slots, "minute")?;
    slot_string(slots, "second")?;
    let date = DateTime::<Utc>::from_timestamp((number / 1_000.0).trunc() as i64, 0)?;
    let millis = number.rem_euclid(1_000.0) as u32;
    let fraction = millis / 10_u32.pow(3 - digits);
    let mut parts = vec![
        typed_part("minute", format!("{:02}", date.minute())),
        literal_part(":"),
        typed_part("second", format!("{:02}", date.second())),
    ];
    if digits > 0 {
        parts.push(literal_part("."));
        parts.push(typed_part(
            "fractionalSecond",
            format!("{fraction:0width$}", width = digits as usize),
        ));
    }
    Some(parts)
}

fn typed_part(kind: &str, value: String) -> Value {
    make_object(vec![
        ("type".to_string(), Value::String(kind.to_string())),
        ("value".to_string(), Value::String(value)),
    ])
}

fn range_parts(start: &str, end: &str) -> Vec<Value> {
    if start == end {
        return vec![literal_part(start)];
    }
    vec![literal_part(start), literal_part(" – "), literal_part(end)]
}

fn day_period_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    let style = slot_string(slots, "dayPeriod")?;
    let hour = crate::date::local_components(number)?.3;
    Some(match style.as_str() {
        "narrow" if hour == 12 => "n".to_string(),
        "narrow" => day_period_name(hour),
        "short" => day_period_name(hour),
        _ => day_period_name(hour),
    })
}

fn hour_day_period_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    slot_string(slots, "hour")?;
    let period = day_period_format(slots, number)?;
    let hour = crate::date::local_components(number)?.3;
    let display_hour = match hour % 12 {
        0 => 12,
        value => value,
    };
    Some(format!("{display_hour} {period}"))
}

fn hour_day_period_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let value = hour_day_period_format(slots, number)?;
    let (hour, period) = value.split_once(' ')?;
    Some(vec![
        typed_part("hour", hour.to_string()),
        literal_part(" "),
        typed_part("dayPeriod", period.to_string()),
    ])
}

fn day_period_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let value = day_period_format(slots, number)?;
    Some(vec![make_object(vec![
        ("type".to_string(), Value::String("dayPeriod".to_string())),
        ("value".to_string(), Value::String(value)),
    ])])
}

fn day_period_name(hour: u32) -> String {
    let name = match hour {
        0..=5 => "at night",
        6..=11 => "in the morning",
        12 => "noon",
        13..=17 => "in the afternoon",
        18..=20 => "in the evening",
        _ => "at night",
    };
    name.to_string()
}

fn range_values(arguments: &[Value]) -> Result<(String, String), VmError> {
    let Some(start_value) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "date value is undefined",
        ));
    };
    let Some(end_value) = arguments.get(1) else {
        return Err(crate::value::error::throw_type_error(
            "date value is undefined",
        ));
    };
    if matches!(start_value, Value::Undefined) || matches!(end_value, Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "date value is undefined",
        ));
    }
    let start = range_number(start_value)?;
    let end = range_number(end_value)?;
    Ok((range_text(start), range_text(end)))
}

fn fractional_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    if slot_string(slots, "minute").is_none() || slot_string(slots, "second").is_none() {
        return None;
    }
    let digits = slot_number(slots, "fractionalSecondDigits").unwrap_or(0.0) as u32;
    let seconds = (number / 1_000.0).trunc() as i64;
    let date = DateTime::<Utc>::from_timestamp(seconds, 0)?;
    if digits == 0 {
        return Some(format!("{:02}:{:02}", date.minute(), date.second()));
    }
    let millis = number.rem_euclid(1_000.0) as u32;
    let fraction = if digits <= 3 {
        millis / 10_u32.pow(3 - digits)
    } else {
        millis * 10_u32.pow(digits - 3)
    };
    Some(format!(
        "{:02}:{:02}.{:0width$}",
        date.minute(),
        date.second(),
        fraction,
        width = digits as usize
    ))
}

fn proleptic_year_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    if slot_string(slots, "year").is_none() || slot_string(slots, "era").is_none() {
        return None;
    }
    let seconds = (number / 1_000.0).trunc() as i64;
    let year = DateTime::<Utc>::from_timestamp(seconds, 0)
        .map_or_else(|| civil_year(number), |date| i64::from(date.year()));
    if year <= 0 {
        Some(format!("{} BC", grouped_year(1 - year)))
    } else {
        Some(format!("{} AD", grouped_year(year)))
    }
}

fn civil_year(number: f64) -> i64 {
    let days = (number / 86_400_000.0).floor() as i64;
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted / 146_097
    } else {
        (shifted - 146_096) / 146_097
    };
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * day_of_year + 2) / 153;
    year + if month < 10 { 0 } else { 1 }
}

fn grouped_year(year: i64) -> String {
    year.to_string()
}

fn range_number(value: &Value) -> Result<f64, VmError> {
    if matches!(value, Value::Undefined) {
        // ECMA-402 12.1.3: format() with no argument formats the current
        // instant; ToNumber(undefined) would yield NaN and incorrectly
        // throw "date value is not finite".
        return Ok(crate::date::chrono_utils::current_time_ms());
    }
    let number = conversion::to_number(value)?;
    if !number.is_finite() || number.abs() > 8_640_000_000_000_000.0 {
        return Err(runtime_error("RangeError: date value is not finite"));
    }
    Ok(number.trunc())
}

fn range_text(number: f64) -> String {
    conversion::number_to_string(number)
}

fn slot_bool(slots: &[(String, Value)], key: &str) -> Option<bool> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::Boolean(value) => Some(*value),
            _ => None,
        })
}

fn date_format_result(slots: &[(String, Value)], number: f64) -> Option<String> {
    let has_year = slot_string(slots, "year").is_some();
    let has_month = slot_string(slots, "month").is_some();
    let has_day = slot_string(slots, "day").is_some();
    let has_weekday = slot_string(slots, "weekday").is_some();
    let has_hour = slot_string(slots, "hour").is_some();
    let has_minute = slot_string(slots, "minute").is_some();
    let has_second = slot_string(slots, "second").is_some();
    if !has_year && !has_month && !has_day && !has_weekday {
        return None;
    }
    let time_zone = slot_string(slots, "timeZone");
    let is_utc = time_zone.as_deref() == Some("UTC") || time_zone.is_none();
    let comps = if is_utc {
        crate::date::chrono_utils::utc_components(number)
    } else {
        crate::date::chrono_utils::local_components(number)
    };
    let (year, month, day, hour, minute, second, _ms) = comps?;
    let date_str = compose_date_string(slots, year, month, day, number, is_utc);
    let has_time = has_hour || has_minute || has_second;
    if !has_time {
        return Some(date_str);
    }
    let time_str = compose_time_string(slots, hour, minute, second);
    if date_str.is_empty() {
        return Some(time_str);
    }
    Some(format!("{} {}", date_str, time_str))
}

fn compose_date_string(
    slots: &[(String, Value)],
    year: i32,
    month: u32,
    day: u32,
    ms: f64,
    is_utc: bool,
) -> String {
    let year_style = slot_string(slots, "year");
    let month_style = slot_string(slots, "month");
    let day_style = slot_string(slots, "day");
    let weekday_style = slot_string(slots, "weekday");
    let month_str = month_style.as_deref().map(|s| format_month_value(s, month));
    let day_str = day_style.as_deref().map(|s| format_day_value(s, day));
    let year_str = year_style.as_deref().map(|s| format_year_value(s, year));
    let weekday_str = weekday_style.as_deref().map(|s| format_weekday_value(s, ms, is_utc));
    let month_is_name = month_str.as_deref().is_some_and(|m| !m.chars().all(|c| c.is_ascii_digit()));
    let mut parts: Vec<String> = Vec::new();
    if let Some(wd) = weekday_str {
        parts.push(wd);
    }
    if month_is_name {
        if let Some(m) = month_str {
            parts.push(m);
        }
        if let Some(d) = day_str {
            parts.push(d);
        }
        if let Some(y) = year_str {
            parts.push(y);
        }
    } else {
        if let Some(m) = month_str {
            parts.push(m);
        }
        if let Some(d) = day_str {
            parts.push(d);
        }
        if let Some(y) = year_str {
            parts.push(y);
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    if !month_is_name && parts.len() >= 3 && year_style.is_some() && month_style.is_some() && day_style.is_some() {
        return format!("{}/{}/{}", parts[0], parts[1], parts[2]);
    }
    let mut out = String::new();
    let mut first = true;
    for p in &parts {
        if !first { out.push(' '); }
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
        "short" => ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"][idx].to_string(),
        "long" => ["January","February","March","April","May","June","July","August","September","October","November","December"][idx].to_string(),
        "narrow" => ["J","F","M","A","M","J","J","A","S","O","N","D"][idx].to_string(),
        _ => month.to_string(),
    }
}

fn format_day_value(style: &str, day: u32) -> String {
    if style == "2-digit" { format!("{:02}", day) } else { day.to_string() }
}

fn format_weekday_value(style: &str, ms: f64, _is_utc: bool) -> String {
    let wd = crate::date::chrono_utils::weekday(ms).unwrap_or(0);
    match style {
        "short" => ["Sun","Mon","Tue","Wed","Thu","Fri","Sat"][wd].to_string(),
        "long" => ["Sunday","Monday","Tuesday","Wednesday","Thursday","Friday","Saturday"][wd].to_string(),
        "narrow" => ["S","M","T","W","T","F","S"][wd].to_string(),
        _ => String::new(),
    }
}

fn compose_time_string(
    slots: &[(String, Value)],
    hour: u32,
    minute: u32,
    second: u32,
) -> String {
    let hour_style = slot_string(slots, "hour");
    let minute_style = slot_string(slots, "minute");
    let second_style = slot_string(slots, "second");
    let day_period_style = slot_string(slots, "dayPeriod");
    let hour12 = slot_bool(slots, "hour12").unwrap_or(false);
    let has_hour = hour_style.is_some();
    let has_minute = minute_style.is_some();
    let has_second = second_style.is_some();
    let hour_str = hour_style.as_deref().map(|s| format_hour_value(s, hour, hour12));
    let minute_str = minute_style.as_deref().map(|s| format_minute_value(s, minute));
    let second_str = second_style.as_deref().map(|s| format_second_value(s, second));
    let mut parts: Vec<String> = Vec::new();
    if has_hour { if let Some(h) = hour_str { parts.push(h); } }
    if has_minute { if let Some(m) = minute_str { parts.push(m); } }
    if has_second { if let Some(s) = second_str { parts.push(s); } }
    if parts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut first = true;
    for p in &parts {
        if !first { out.push(':'); }
        out.push_str(p);
        first = false;
    }
    if let Some(dp_style) = day_period_style {
        if let Some(dp) = day_period_name_from_style(&dp_style, hour) {
            out.push(' ');
            out.push_str(&dp);
        }
    }
    out
}

fn format_hour_value(style: &str, hour: u32, hour12: bool) -> String {
    if hour12 {
        let h12 = if hour == 0 { 12 } else if hour > 12 { hour - 12 } else { hour };
        if style == "2-digit" { format!("{:02}", h12) } else { h12.to_string() }
    } else {
        if style == "2-digit" { format!("{:02}", hour) } else { hour.to_string() }
    }
}

fn format_minute_value(style: &str, minute: u32) -> String {
    if style == "2-digit" { format!("{:02}", minute) } else { minute.to_string() }
}

fn format_second_value(style: &str, second: u32) -> String {
    if style == "2-digit" { format!("{:02}", second) } else { second.to_string() }
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
        "narrow" => match hour { 0..=11 => "AM", _ => "PM" },
        _ => return None,
    };
    Some(name.to_string())
}
