fn format_result(arguments: &[Value], slots: &[(String, Value)]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        if let Some(fields) = temporal_fields(value) {
            if fields.kind == TemporalKind::ZonedDateTime {
                return Err(crate::value::error::throw_type_error(
                    "Temporal.ZonedDateTime is not supported",
                ));
            }
            let effective_slots = temporal_slots(slots, &fields)?;
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
    let format_slots = effective_format_slots(slots);
    let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
    let has_era = slot_string(&format_slots, "era").is_some();
    let has_date = slot_string(&format_slots, "year").is_some()
        || slot_string(&format_slots, "month").is_some()
        || slot_string(&format_slots, "day").is_some()
        || slot_string(&format_slots, "weekday").is_some();
    let has_time = slot_string(&format_slots, "hour").is_some()
        || slot_string(&format_slots, "minute").is_some()
        || slot_string(&format_slots, "second").is_some();
    let text = if has_date && !has_era {
        date_format_result(&format_slots, number)
    } else {
        None
    }
    .or_else(|| hour_day_period_format(&format_slots, number))
    .or_else(|| day_period_format(&format_slots, number))
    .or_else(|| {
        if has_time && !has_era {
            date_format_result(&format_slots, number)
        } else {
            None
        }
    })
    .or_else(|| proleptic_year_format(&format_slots, number))
    .or_else(|| fractional_format(&format_slots, number))
    .unwrap_or_else(|| range_text(number));
    let numbering = slot_string(&format_slots, "numberingSystem")
        .unwrap_or_else(|| "latn".to_string());
    let mut localized = crate::intl::number::localize_digits(text, &numbering);
    if numbering == "arab" {
        localized = localized.replace('.', "٫");
    }
    Ok(Value::String(localized))
}

fn temporal_slots(
    slots: &[(String, Value)],
    fields: &TemporalFields,
) -> Result<Vec<(String, Value)>, VmError> {
    let has_date_style = slots.iter().any(|(name, _)| name == "dateStyle");
    let has_time_style = slots.iter().any(|(name, _)| name == "timeStyle");
    if has_date_style && fields.kind == TemporalKind::PlainTime {
        return Err(crate::value::error::throw_type_error(
            "dateStyle is incompatible with Temporal.PlainTime",
        ));
    }
    if has_time_style
        && matches!(
            fields.kind,
            TemporalKind::PlainDate | TemporalKind::PlainMonthDay | TemporalKind::PlainYearMonth
        )
    {
        return Err(crate::value::error::throw_type_error(
            "timeStyle is incompatible with this Temporal value",
        ));
    }
    if fields.kind == TemporalKind::PlainMonthDay
        && slots.iter().any(|(name, _)| name == "year")
        && !has_date_style
    {
        return Err(crate::value::error::throw_type_error(
            "year is incompatible with Temporal.PlainMonthDay",
        ));
    }
    if fields.kind == TemporalKind::PlainYearMonth
        && slots.iter().any(|(name, _)| name == "day")
    {
        return Err(crate::value::error::throw_type_error(
            "day is incompatible with Temporal.PlainYearMonth",
        ));
    }
    if fields.kind == TemporalKind::PlainTime
        && slots.iter().any(|(name, _)| {
            matches!(name.as_str(), "year" | "month" | "day" | "weekday" | "era")
        })
    {
        return Err(crate::value::error::throw_type_error(
            "date fields are incompatible with Temporal.PlainTime",
        ));
    }
    let mut filtered = slots
        .iter()
        .filter(|(name, _)| match fields.kind {
            TemporalKind::PlainDate => {
                !matches!(name.as_str(), "hour" | "minute" | "second" | "dayPeriod")
            }
            TemporalKind::PlainYearMonth => {
                !matches!(name.as_str(), "day" | "hour" | "minute" | "second" | "dayPeriod")
            }
            TemporalKind::PlainMonthDay => {
                !matches!(name.as_str(), "year" | "hour" | "minute" | "second" | "dayPeriod")
            }
            TemporalKind::PlainTime => {
                !matches!(name.as_str(), "year" | "month" | "day" | "weekday" | "era")
            }
            TemporalKind::PlainDateTime | TemporalKind::ZonedDateTime => true,
        })
        .cloned()
        .collect::<Vec<_>>();
    Ok(temporal_default_slots(&filtered, fields))
}

fn effective_format_slots(slots: &[(String, Value)]) -> Vec<(String, Value)> {
    if slots.iter().any(|(name, _)| {
        matches!(
            name.as_str(),
            "weekday" | "era" | "year" | "month" | "day" | "hour" | "minute" | "second"
        )
    }) {
        return slots.to_vec();
    }
    let mut result = slots.to_vec();
    let date_style = slot_string(slots, "dateStyle");
    let time_style = slot_string(slots, "timeStyle");
    match date_style.as_deref() {
        Some("full") => result.extend([
            ("weekday".into(), Value::String("long".into())),
            ("month".into(), Value::String("long".into())),
            ("day".into(), Value::String("numeric".into())),
            ("year".into(), Value::String("numeric".into())),
        ]),
        Some("long") => result.extend([
            ("month".into(), Value::String("long".into())),
            ("day".into(), Value::String("numeric".into())),
            ("year".into(), Value::String("numeric".into())),
        ]),
        Some("medium") => result.extend([
            ("month".into(), Value::String("short".into())),
            ("day".into(), Value::String("numeric".into())),
            ("year".into(), Value::String("numeric".into())),
        ]),
        Some("short") => result.extend([
            ("month".into(), Value::String("numeric".into())),
            ("day".into(), Value::String("numeric".into())),
            ("year".into(), Value::String("2-digit".into())),
        ]),
        _ => {}
    }
    match time_style.as_deref() {
        Some("short") => result.extend([
            ("hour".into(), Value::String("numeric".into())),
            ("minute".into(), Value::String("numeric".into())),
        ]),
        Some("medium" | "long" | "full") => result.extend([
            ("hour".into(), Value::String("numeric".into())),
            ("minute".into(), Value::String("numeric".into())),
            ("second".into(), Value::String("numeric".into())),
        ]),
        _ => {}
    }
    if let Some(style) = time_style.as_deref() {
        if style == "long" {
            result.push(("timeZoneName".into(), Value::String("short".into())));
        } else if style == "full" {
            result.push(("timeZoneName".into(), Value::String("long".into())));
        }
    }
    if result.iter().any(|(name, _)| name == "hour")
        && !result.iter().any(|(name, _)| name == "hour12")
    {
        let locale = slot_string(slots, "locale").unwrap_or_default();
        let from_extension = locale
            .split_once("-u-")
            .and_then(|(_, extension)| {
                let parts: Vec<_> = extension.split('-').collect();
                let index = parts.iter().position(|part| *part == "hc")? + 1;
                match parts.get(index).copied()? {
                    "h11" | "h12" => Some(true),
                    "h23" | "h24" => Some(false),
                    _ => None,
                }
            });
        result.push((
            "hour12".into(),
            Value::Boolean(
                from_extension.unwrap_or_else(|| locale.starts_with("en") || locale.starts_with("ja")),
            ),
        ));
    }
    result
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
    let mut result = effective_format_slots(slots);
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
    let slots = effective_format_slots(slots);
    let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if let Some(parts) = time_parts(&slots, number) {
        return Ok(make_array(parts));
    }
    if let Some(value) = hour_day_period_parts(&slots, number) {
        return Ok(make_array(value));
    }
    if let Some(value) = day_period_parts(&slots, number) {
        return Ok(make_array(value));
    }
    if let Some(value) = fractional_parts(&slots, number) {
        return Ok(make_array(value));
    }
    let value = range_text(number);
    Ok(make_array(vec![literal_part(&value)]))
}

fn time_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let hour_style = slot_string(slots, "hour")?;
    let (_, _, _, hour, minute, second, millis) = format_components(slots, number)?;
    let hour12 = slots
        .iter()
        .find_map(|(name, value)| {
            (name == "hour12").then_some(matches!(value, Value::Boolean(true)))
        })
        .unwrap_or(false);
    let mut parts = vec![typed_part("hour", format_hour_value(&hour_style, hour, hour12))];
    if slot_string(slots, "minute").is_some() {
        parts.push(literal_part(":"));
        parts.push(typed_part("minute", format!("{minute:02}")));
    }
    if slot_string(slots, "second").is_some() {
        parts.push(literal_part(":"));
        parts.push(typed_part("second", format!("{second:02}")));
    }
    if let Some(digits) = slot_number(slots, "fractionalSecondDigits") {
        let digits = digits as u32;
        if digits > 0 {
            let fraction = millis / 10_u32.pow(3 - digits);
            parts.push(literal_part("."));
            parts.push(typed_part(
                "fractionalSecond",
                format!("{fraction:0width$}", width = digits as usize),
            ));
        }
    }
    if hour12 {
        parts.push(literal_part(" "));
        parts.push(typed_part(
            "dayPeriod",
            if hour < 12 { "AM" } else { "PM" }.into(),
        ));
    }
    Some(parts)
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
    let start_temporal = arguments.first().and_then(temporal_fields);
    let end_temporal = arguments.get(1).and_then(temporal_fields);
    if start_temporal.is_some() || end_temporal.is_some() {
        let (Some(start_temporal), Some(end_temporal)) = (start_temporal, end_temporal) else {
            return Err(crate::value::error::throw_type_error(
                "formatRange requires matching date kinds",
            ));
        };
        if start_temporal.kind != end_temporal.kind
            || start_temporal.kind == TemporalKind::ZonedDateTime
        {
            return Err(crate::value::error::throw_type_error(
                "formatRange requires matching date kinds",
            ));
        }
        let start_slots = temporal_slots(slots, &start_temporal)?;
        let end_slots = temporal_slots(slots, &end_temporal)?;
        let start = temporal_date_format_result(
            &start_slots,
            start_temporal.year,
            start_temporal.month,
            start_temporal.day,
            start_temporal.hour,
            start_temporal.minute,
            start_temporal.second,
            start_temporal.millisecond,
        )
        .unwrap_or_default();
        let end = temporal_date_format_result(
            &end_slots,
            end_temporal.year,
            end_temporal.month,
            end_temporal.day,
            end_temporal.hour,
            end_temporal.minute,
            end_temporal.second,
            end_temporal.millisecond,
        )
        .unwrap_or_default();
        return Ok((start, end));
    }
    let slots = effective_format_slots(slots);
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
    let start_str = if let Some(v) = date_format_result(&slots, start) {
        v
    } else if let Some(v) = hour_day_period_format(&slots, start) {
        v
    } else if let Some(v) = day_period_format(&slots, start) {
        v
    } else if let Some(v) = proleptic_year_format(&slots, start) {
        v
    } else if let Some(v) = fractional_format(&slots, start) {
        v
    } else {
        range_text(start)
    };
    let end_str = if let Some(v) = date_format_result(&slots, end) {
        v
    } else if let Some(v) = hour_day_period_format(&slots, end) {
        v
    } else if let Some(v) = day_period_format(&slots, end) {
        v
    } else if let Some(v) = proleptic_year_format(&slots, end) {
        v
    } else if let Some(v) = fractional_format(&slots, end) {
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
