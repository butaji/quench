fn format_result(arguments: &[Value], slots: &[(String, Value)]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        if let Some(fields) = temporal_fields(value) {
            if fields.kind == TemporalKind::ZonedDateTime {
                return Err(crate::value::error::throw_type_error(
                    "Temporal.ZonedDateTime is not supported",
                ));
            }
            let effective_slots = temporal_default_slots(slots, &fields);
            if let Some(text) = temporal_date_format_result(
                &effective_slots,
                fields.year,
                fields.month,
                fields.day,
                fields.hour,
                fields.minute,
                fields.second,
                fields.millisecond,
            ) {
                let numbering =
                    slot_string(&effective_slots, "numberingSystem")
                        .unwrap_or_else(|| "latn".to_string());
                let mut localized = crate::intl::number::localize_digits(text, &numbering);
                if numbering == "arab" {
                    localized = localized.replace('.', "٫");
                }
                return Ok(Value::String(localized));
            }
        }
    }
    let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
    let has_era = slot_string(slots, "era").is_some();
    let has_date = slot_string(slots, "year").is_some()
        || slot_string(slots, "month").is_some()
        || slot_string(slots, "day").is_some()
        || slot_string(slots, "weekday").is_some();
    let has_time = slot_string(slots, "hour").is_some()
        || slot_string(slots, "minute").is_some()
        || slot_string(slots, "second").is_some();
    let text = if has_date && !has_era {
        date_format_result(slots, number)
    } else {
        None
    }
    .or_else(|| hour_day_period_format(slots, number))
    .or_else(|| day_period_format(slots, number))
    .or_else(|| {
        if has_time && !has_era {
            date_format_result(slots, number)
        } else {
            None
        }
    })
    .or_else(|| proleptic_year_format(slots, number))
    .or_else(|| fractional_format(slots, number))
    .unwrap_or_else(|| range_text(number));
    let numbering = slot_string(slots, "numberingSystem").unwrap_or_else(|| "latn".to_string());
    let mut localized = crate::intl::number::localize_digits(text, &numbering);
    if numbering == "arab" {
        localized = localized.replace('.', "٫");
    }
    Ok(Value::String(localized))
}

fn temporal_default_slots(
    slots: &[(String, Value)],
    fields: &TemporalFields,
) -> Vec<(String, Value)> {
    if slots.iter().any(|(name, _)| {
        matches!(
            name.as_str(),
            "year" | "month" | "day" | "weekday" | "hour" | "minute" | "second"
        )
    }) {
        return slots.to_vec();
    }
    let mut result = slots.to_vec();
    let has_date_style = slots.iter().any(|(name, _)| name == "dateStyle");
    let has_time_style = slots.iter().any(|(name, _)| name == "timeStyle");
    if has_date_style {
        result.extend([
            ("year".to_string(), Value::String("numeric".to_string())),
            ("month".to_string(), Value::String("numeric".to_string())),
            ("day".to_string(), Value::String("numeric".to_string())),
        ]);
    }
    if has_time_style {
        result.extend([
            ("hour".to_string(), Value::String("numeric".to_string())),
            ("minute".to_string(), Value::String("numeric".to_string())),
        ]);
    }
    if has_date_style || has_time_style {
        return result;
    }
    match fields.kind {
        TemporalKind::PlainTime => {
            result.push(("hour".to_string(), Value::String("numeric".to_string())));
            result.push(("hour12".to_string(), Value::Boolean(true)));
        }
        TemporalKind::PlainMonthDay => {
            result.push(("month".to_string(), Value::String("numeric".to_string())));
            result.push(("day".to_string(), Value::String("numeric".to_string())));
        }
        TemporalKind::PlainYearMonth => {
            result.push(("year".to_string(), Value::String("numeric".to_string())));
            result.push(("month".to_string(), Value::String("numeric".to_string())));
        }
        TemporalKind::PlainDate | TemporalKind::PlainDateTime => {
            result.push(("year".to_string(), Value::String("numeric".to_string())));
            result.push(("month".to_string(), Value::String("numeric".to_string())));
            result.push(("day".to_string(), Value::String("numeric".to_string())));
        }
        TemporalKind::ZonedDateTime => {}
    }
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TemporalKind {
    PlainDate,
    PlainDateTime,
    PlainTime,
    PlainMonthDay,
    PlainYearMonth,
    ZonedDateTime,
}

struct TemporalFields {
    kind: TemporalKind,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
}

fn temporal_fields(value: &Value) -> Option<TemporalFields> {
    let Value::Object(properties) = value else {
        return None;
    };
    let prototype = properties
        .iter()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value))?;
    let kind = match prototype {
        Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype) => {
            TemporalKind::PlainDate
        }
        Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype) => {
            TemporalKind::PlainDateTime
        }
        Value::Builtin(crate::ops::Builtin::TemporalPlainTimePrototype) => TemporalKind::PlainTime,
        Value::Builtin(crate::ops::Builtin::TemporalPlainMonthDayPrototype) => {
            TemporalKind::PlainMonthDay
        }
        Value::Builtin(crate::ops::Builtin::TemporalPlainYearMonthPrototype) => {
            TemporalKind::PlainYearMonth
        }
        Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype) => {
            TemporalKind::ZonedDateTime
        }
        _ => return None,
    };
    Some(TemporalFields {
        kind,
        year: numeric_field(properties, "year").unwrap_or(1970.0) as i32,
        month: numeric_field(properties, "month")
            .or_else(|| month_code_field(properties))
            .unwrap_or(1.0) as u32,
        day: numeric_field(properties, "day").unwrap_or(1.0) as u32,
        hour: numeric_field(properties, "hour").unwrap_or(0.0) as u32,
        minute: numeric_field(properties, "minute").unwrap_or(0.0) as u32,
        second: numeric_field(properties, "second").unwrap_or(0.0) as u32,
        millisecond: numeric_field(properties, "millisecond").unwrap_or(0.0) as u32,
    })
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
    vec![
        literal_part(start),
        make_object(vec![
            ("type".to_string(), Value::String("literal".to_string())),
            ("value".to_string(), Value::String(" – ".to_string())),
            ("source".to_string(), Value::String("shared".to_string())),
        ]),
        literal_part(end),
    ]
}

fn day_period_format(slots: &[(String, Value)], number: f64) -> Option<String> {
    let style = slot_string(slots, "dayPeriod")?;
    let hour = format_components(slots, number)?.3;
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
    let hour = format_components(slots, number)?.3;
    let display_hour = match hour % 12 {
        0 => 12,
        value => value,
    };
    Some(format!("{display_hour} {period}"))
}

fn format_components(slots: &[(String, Value)], number: f64) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    let is_utc = slot_string(slots, "timeZone").as_deref() == Some("UTC")
        || slot_string(slots, "timeZone").is_none();
    if is_utc {
        crate::date::chrono_utils::utc_components(number)
    } else {
        crate::date::chrono_utils::local_components(number)
    }
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

fn range_values(
    arguments: &[Value],
    slots: &[(String, Value)],
) -> Result<(String, String), VmError> {
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
    let start_str = if let Some(v) = date_format_result(slots, start) {
        v
    } else if let Some(v) = hour_day_period_format(slots, start) {
        v
    } else if let Some(v) = day_period_format(slots, start) {
        v
    } else if let Some(v) = proleptic_year_format(slots, start) {
        v
    } else if let Some(v) = fractional_format(slots, start) {
        v
    } else {
        range_text(start)
    };
    let end_str = if let Some(v) = date_format_result(slots, end) {
        v
    } else if let Some(v) = hour_day_period_format(slots, end) {
        v
    } else if let Some(v) = day_period_format(slots, end) {
        v
    } else if let Some(v) = proleptic_year_format(slots, end) {
        v
    } else if let Some(v) = fractional_format(slots, end) {
        v
    } else {
        range_text(end)
    };
    Ok((start_str, end_str))
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
    if let Some(number) = temporal_number(value) {
        return Ok(number);
    }
    let number = conversion::to_number(value)?;
    if !number.is_finite() || number.abs() > 8_640_000_000_000_000.0 {
        return Err(runtime_error("RangeError: date value is not finite"));
    }
    Ok(number.trunc())
}

fn temporal_number(value: &Value) -> Option<f64> {
    let Value::Object(properties) = value else {
        return None;
    };
    let prototype = properties
        .iter()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value))?;
    let kind = match prototype {
        Value::Builtin(crate::ops::Builtin::TemporalInstantPrototype)
        | Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype) => "instant",
        Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype) => "date",
        Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype) => "datetime",
        Value::Builtin(crate::ops::Builtin::TemporalPlainTimePrototype) => "time",
        Value::Builtin(crate::ops::Builtin::TemporalPlainMonthDayPrototype) => "monthday",
        Value::Builtin(crate::ops::Builtin::TemporalPlainYearMonthPrototype) => "yearmonth",
        _ => return None,
    };
    if kind == "instant" {
        let epoch = bigint_field(properties, "epochNanoseconds")?;
        return Some(epoch / 1_000_000.0);
    }
    let year = numeric_field(properties, "year").unwrap_or(1970.0);
    let month = numeric_field(properties, "month").unwrap_or(1.0);
    let day = numeric_field(properties, "day").unwrap_or(1.0);
    let hour = numeric_field(properties, "hour").unwrap_or(0.0);
    let minute = numeric_field(properties, "minute").unwrap_or(0.0);
    let second = numeric_field(properties, "second").unwrap_or(0.0);
    let millisecond = numeric_field(properties, "millisecond").unwrap_or(0.0);
    Some(crate::date::chrono_utils::make_date_ms(
        year,
        month - 1.0,
        day,
        hour,
        minute,
        second,
        millisecond,
    ))
}

fn bigint_field(properties: &crate::value::ObjectData, key: &str) -> Option<f64> {
    properties.iter().rev().find_map(|(name, value)| {
        if name != key {
            return None;
        }
        match value {
            Value::BigInt(number) => number.parse::<f64>().ok(),
            Value::BindingCell(cell) => match &*cell.borrow() {
                Value::BigInt(number) => number.parse::<f64>().ok(),
                _ => None,
            },
            _ => None,
        }
    })
}

fn numeric_field(properties: &crate::value::ObjectData, key: &str) -> Option<f64> {
    properties.iter().rev().find_map(|(name, value)| {
        if name != key {
            return None;
        }
        match value {
            Value::Number(number) => Some(number),
            Value::BindingCell(cell) => match &*cell.borrow() {
                Value::Number(number) => Some(*number),
                _ => None,
            },
            _ => None,
        }
    })
}

fn month_code_field(properties: &crate::value::ObjectData) -> Option<f64> {
    properties.iter().rev().find_map(|(name, value)| {
        if name != "monthCode" {
            return None;
        }
        let Value::String(code) = value else {
            return None;
        };
        code.strip_prefix('M')?.parse::<f64>().ok()
    })
}

fn range_text(number: f64) -> String {
    conversion::number_to_string(number)
}
include!("datetime_format_date.rs");
