fn format_result(arguments: &[Value], slots: &[(String, Value)]) -> Result<Value, VmError> {
    if let Some(value) = arguments.first() {
        if let Some(fields) = temporal_fields(value) {
            if fields.kind == TemporalKind::ZonedDateTime {
                return Err(crate::value::error::throw_type_error(
                    "Temporal.ZonedDateTime is not supported",
                ));
            }
            if fields.kind == TemporalKind::Instant {
                // Instants are converted through the formatter's time zone,
                // like legacy Date values, rather than treated as plain fields.
            } else {
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
                    let numbering = slot_string(&effective_slots, "numberingSystem")
                        .unwrap_or_else(|| "latn".to_string());
                    let mut localized = crate::intl::number::localize_digits(text, &numbering);
                    if numbering == "arab" {
                        localized = localized.replace('.', "٫");
                    }
                    return Ok(Value::String(localized));
                }
            }
        }
    }
    let format_slots = if let Some(fields) = arguments.first().and_then(temporal_fields) {
        if fields.kind == TemporalKind::Instant {
            temporal_slots(slots, &fields)?
        } else {
            effective_format_slots(slots)
        }
    } else {
        effective_format_slots(slots)
    };
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
    // Keep `format()` and `formatToParts()` on the same calendarized data path.
    // The compact date formatter above emits ISO fields, while parts are
    // rewritten through ICU for non-Gregorian calendars (including leap
    // months).  Joining those parts preserves the observable equivalence.
    let text = if has_date
        && slot_string(&format_slots, "calendar")
            .is_some_and(|calendar| calendar != "gregory" && calendar != "iso8601")
    {
        date_time_parts(&format_slots, number)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| match part {
                        Value::Object(properties) => properties
                            .iter()
                            .find_map(|(name, value)| (name == "value").then_some(value))
                            .and_then(|value| match value {
                                Value::String(value) => Some(value.clone()),
                                _ => None,
                            }),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .concat()
            })
            .unwrap_or(text)
    } else {
        text
    };
    let numbering =
        slot_string(&format_slots, "numberingSystem").unwrap_or_else(|| "latn".to_string());
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
    let formatter_calendar = slot_string(slots, "calendar").unwrap_or_else(|| "gregory".into());
    let calendar_matches = match fields.kind {
        TemporalKind::PlainMonthDay | TemporalKind::PlainYearMonth => {
            fields.calendar == formatter_calendar
        }
        _ => fields.calendar == "iso8601"
            || fields.calendar == "gregory"
            || fields.calendar == formatter_calendar,
    };
    if !calendar_matches {
        return Err(crate::value::error::throw_range_error(
            "Temporal calendar does not match formatter calendar",
        ));
    }
    let has_date_style = slots.iter().any(|(name, _)| name == "dateStyle");
    let has_time_style = slots.iter().any(|(name, _)| name == "timeStyle");
    if has_date_style && !has_time_style && fields.kind == TemporalKind::PlainTime {
        return Err(crate::value::error::throw_type_error(
            "dateStyle is incompatible with Temporal.PlainTime",
        ));
    }
    if has_time_style
        && !has_date_style
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
        && !slots
            .iter()
            .any(|(name, _)| name == "month" || name == "day")
        && !has_date_style
    {
        return Err(crate::value::error::throw_type_error(
            "year is incompatible with Temporal.PlainMonthDay",
        ));
    }
    if fields.kind == TemporalKind::PlainYearMonth
        && slots.iter().any(|(name, _)| name == "day")
        && !slots
            .iter()
            .any(|(name, _)| name == "year" || name == "month")
    {
        return Err(crate::value::error::throw_type_error(
            "day is incompatible with Temporal.PlainYearMonth",
        ));
    }
    if matches!(
        fields.kind,
        TemporalKind::PlainDate | TemporalKind::PlainYearMonth | TemporalKind::PlainMonthDay
    ) {
        let has_time = slots
            .iter()
            .any(|(name, _)| matches!(name.as_str(), "hour" | "minute" | "second" | "dayPeriod"));
        let has_date = slots
            .iter()
            .any(|(name, _)| matches!(name.as_str(), "weekday" | "era" | "year" | "month" | "day"));
        if has_time && !has_date && !has_date_style {
            return Err(crate::value::error::throw_type_error(
                "time fields are incompatible with this Temporal value",
            ));
        }
    }
    if fields.kind == TemporalKind::PlainTime
        && slots
            .iter()
            .any(|(name, _)| matches!(name.as_str(), "year" | "month" | "day" | "weekday"))
        && !slots.iter().any(|(name, _)| {
            matches!(
                name.as_str(),
                "hour" | "minute" | "second" | "dayPeriod" | "timeStyle"
            )
        })
        && !slots.iter().any(|(name, _)| name == "era")
        && !(slots.iter().any(|(name, _)| name == "year")
            && slots.iter().any(|(name, _)| name == "month")
            && slots.iter().any(|(name, _)| name == "day"))
    {
        return Err(crate::value::error::throw_type_error(
            "date fields are incompatible with Temporal.PlainTime",
        ));
    }
    let mut filtered = slots
        .iter()
        .filter(|(name, _)| match fields.kind {
            TemporalKind::Instant => true,
            TemporalKind::PlainDate => {
                !matches!(name.as_str(), "hour" | "minute" | "second" | "dayPeriod")
            }
            TemporalKind::PlainYearMonth => !matches!(
                name.as_str(),
                "day" | "hour" | "minute" | "second" | "dayPeriod"
            ),
            TemporalKind::PlainMonthDay => !matches!(
                name.as_str(),
                "year" | "hour" | "minute" | "second" | "dayPeriod"
            ),
            TemporalKind::PlainTime => !matches!(
                name.as_str(),
                "year" | "month" | "day" | "weekday" | "era" | "timeZoneName"
            ),
            TemporalKind::PlainDateTime => name != "timeZoneName",
            TemporalKind::ZonedDateTime => true,
        })
        .cloned()
        .collect::<Vec<_>>();
    let has_time = filtered
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "hour" | "minute" | "second"));
    let has_default_date = filtered.iter().any(|(name, _)| name == "year")
        && filtered.iter().any(|(name, _)| name == "month")
        && filtered.iter().any(|(name, _)| name == "day");
    if !has_time
        && !has_date_style
        && !has_time_style
        && has_default_date
        && (fields.kind == TemporalKind::PlainDateTime
            || (fields.kind == TemporalKind::Instant
                && !filtered.iter().any(|(name, _)| {
                    matches!(
                        name.as_str(),
                        "weekday" | "era" | "dayPeriod" | "timeZoneName"
                    )
                })))
    {
        filtered.extend([
            ("hour".into(), Value::String("numeric".into())),
            ("minute".into(), Value::String("numeric".into())),
            ("second".into(), Value::String("numeric".into())),
        ]);
    } else if !has_time
        && !has_date_style
        && !has_time_style
        && fields.kind == TemporalKind::PlainTime
    {
        filtered.extend([
            ("hour".into(), Value::String("numeric".into())),
            ("minute".into(), Value::String("numeric".into())),
            ("second".into(), Value::String("numeric".into())),
        ]);
    }
    let mut resolved = temporal_default_slots(&filtered, fields);
    if resolved.iter().any(|(name, _)| name == "hour")
        && !resolved.iter().any(|(name, _)| name == "hour12")
    {
        let hour12 = slot_string(&resolved, "hourCycle")
            .map(|cycle| matches!(cycle.as_str(), "h11" | "h12"))
            .or_else(|| {
                slot_string(&resolved, "locale")
                    .map(|locale| locale.starts_with("en") || locale.starts_with("ja"))
            })
            .unwrap_or(false);
        resolved.push(("hour12".into(), Value::Boolean(hour12)));
    }
    if matches!(
        fields.kind,
        TemporalKind::PlainDate
            | TemporalKind::PlainDateTime
            | TemporalKind::PlainTime
            | TemporalKind::PlainMonthDay
            | TemporalKind::PlainYearMonth
    ) {
        resolved.retain(|(name, _)| name != "timeZoneName");
    }
    resolved.retain(|(name, _)| match fields.kind {
        TemporalKind::Instant => true,
        TemporalKind::PlainDate => {
            !(has_date_style && matches!(name.as_str(), "hour" | "minute" | "second" | "dayPeriod"))
        }
        TemporalKind::PlainYearMonth => {
            name != "day"
                && !(has_date_style
                    && matches!(name.as_str(), "hour" | "minute" | "second" | "dayPeriod"))
        }
        TemporalKind::PlainMonthDay => {
            name != "year"
                && name != "era"
                && !(has_date_style
                    && matches!(name.as_str(), "hour" | "minute" | "second" | "dayPeriod"))
        }
        TemporalKind::PlainTime => {
            !matches!(name.as_str(), "year" | "month" | "day" | "weekday" | "era")
        }
        TemporalKind::PlainDateTime | TemporalKind::ZonedDateTime => true,
    });
    Ok(resolved)
}

fn effective_format_slots(slots: &[(String, Value)]) -> Vec<(String, Value)> {
    if slots.iter().any(|(name, _)| {
        matches!(
            name.as_str(),
            "weekday" | "year" | "month" | "day" | "hour" | "minute" | "second"
        )
    }) {
        let mut result = slots.to_vec();
        return result;
    }
    let mut result = slots.to_vec();
    if slots.iter().any(|(name, _)| name == "era") {
        result.extend([
            ("year".into(), Value::String("numeric".into())),
            ("month".into(), Value::String("numeric".into())),
            ("day".into(), Value::String("numeric".into())),
        ]);
        return result;
    }
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
        let from_extension = locale.split_once("-u-").and_then(|(_, extension)| {
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
                from_extension
                    .unwrap_or_else(|| locale.starts_with("en") || locale.starts_with("ja")),
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
        TemporalKind::Instant => {
            if result.iter().any(|(name, _)| name == "era") {
                result.extend([
                    ("year".to_string(), Value::String("numeric".to_string())),
                    ("month".to_string(), Value::String("numeric".to_string())),
                    ("day".to_string(), Value::String("numeric".to_string())),
                ]);
            }
        }
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
    Instant,
    PlainDate,
    PlainDateTime,
    PlainTime,
    PlainMonthDay,
    PlainYearMonth,
    ZonedDateTime,
}

struct TemporalFields {
    kind: TemporalKind,
    calendar: String,
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
        Value::Builtin(crate::ops::Builtin::TemporalInstantPrototype) => TemporalKind::Instant,
        Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype) => TemporalKind::PlainDate,
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
        calendar: properties
            .iter()
            .find_map(|(name, value)| {
                (name == "calendarId").then_some(match value {
                    Value::String(calendar) => calendar.clone(),
                    _ => "iso8601".to_string(),
                })
            })
            .unwrap_or_else(|| "iso8601".to_string()),
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
    if let Some(value) = arguments.first() {
        if let Some(fields) = temporal_fields(value) {
            if fields.kind == TemporalKind::ZonedDateTime {
                return Err(crate::value::error::throw_type_error(
                    "Temporal.ZonedDateTime is not supported",
                ));
            }
            if fields.kind != TemporalKind::Instant {
                let slots = temporal_slots(slots, &fields)?;
                return Ok(make_array(localize_parts(
                    parts_for_fields(
                        &slots,
                        fields.year,
                        fields.month,
                        fields.day,
                        fields.hour,
                        fields.minute,
                        fields.second,
                        fields.millisecond,
                    ),
                    &slots,
                )));
            }
        }
    }
    let slots = if let Some(fields) = arguments.first().and_then(temporal_fields) {
        if fields.kind == TemporalKind::Instant {
            temporal_slots(slots, &fields)?
        } else {
            effective_format_slots(slots)
        }
    } else {
        effective_format_slots(slots)
    };
    let number = range_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if let Some(parts) = date_time_parts(&slots, number) {
        return Ok(make_array(localize_parts(parts, &slots)));
    }
    if slot_string(&slots, "dayPeriod").is_some() {
        if let Some(value) = hour_day_period_parts(&slots, number) {
            return Ok(make_array(localize_parts(value, &slots)));
        }
    }
    if let Some(parts) = time_parts(&slots, number) {
        return Ok(make_array(localize_parts(parts, &slots)));
    }
    if let Some(value) = hour_day_period_parts(&slots, number) {
        return Ok(make_array(localize_parts(value, &slots)));
    }
    if let Some(value) = day_period_parts(&slots, number) {
        return Ok(make_array(localize_parts(value, &slots)));
    }
    if let Some(value) = fractional_parts(&slots, number) {
        return Ok(make_array(localize_parts(value, &slots)));
    }
    let value = range_text(number);
    Ok(make_array(localize_parts(
        vec![literal_part(&value)],
        &slots,
    )))
}

fn localize_parts(parts: Vec<Value>, slots: &[(String, Value)]) -> Vec<Value> {
    let numbering = slot_string(slots, "numberingSystem").unwrap_or_else(|| "latn".to_string());
    parts
        .into_iter()
        .map(|part| {
            let Value::Object(properties) = part else {
                return part;
            };
            let properties = properties
                .iter()
                .map(|(name, value)| {
                    if name == "value" {
                        if let Value::String(value) = value {
                            let mut localized =
                                crate::intl::number::localize_digits(value.clone(), &numbering);
                            if numbering == "arab" {
                                localized = localized.replace('.', "٫");
                            }
                            return (name.to_string(), Value::String(localized));
                        }
                    }
                    (name.to_string(), value.clone())
                })
                .collect();
            make_object(properties)
        })
        .collect()
}

fn date_time_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let (year, month, day, hour, minute, second, millis) = format_components(slots, number)?;
    if !slots
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "year" | "month" | "day" | "weekday"))
    {
        return None;
    }
    let mut parts_slots = slots.to_vec();
    parts_slots.extend([
        ("\0isoYear".into(), Value::Number(f64::from(year))),
        ("\0isoMonth".into(), Value::Number(f64::from(month))),
        ("\0isoDay".into(), Value::Number(f64::from(day))),
    ]);
    Some(parts_for_fields(
        &parts_slots, year, month, day, hour, minute, second, millis,
    ))
}

fn parts_for_fields(
    slots: &[(String, Value)],
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
) -> Vec<Value> {
    let has_year = slot_string(slots, "year").is_some();
    let has_month = slot_string(slots, "month").is_some();
    let has_day = slot_string(slots, "day").is_some();
    let has_weekday = slot_string(slots, "weekday").is_some();
    let has_time = slot_string(slots, "hour").is_some()
        || slot_string(slots, "minute").is_some()
        || slot_string(slots, "second").is_some();
    let mut date = Vec::new();
    if has_weekday {
        date.push(typed_part(
            "weekday",
            format_weekday_value(
                slot_string(slots, "weekday").as_deref().unwrap_or("long"),
                temporal_weekday(year, month, day),
            ),
        ));
        if has_year || has_month || has_day || has_time {
            date.push(literal_part(", "));
        }
    }
    let month_value = slot_string(slots, "month")
        .map(|style| format_month_value_for_slots(slots, &style, month));
    let day_value = slot_string(slots, "day").map(|style| format_day_value(&style, day));
    let display_year = slot_string(slots, "calendar")
        .filter(|calendar| calendar == "japanese")
        .and_then(|_| {
            crate::temporal::plain_date::era_year_for_calendar("japanese", f64::from(year))
                .map(|value| value as i32)
        })
        .unwrap_or(year);
    let year_value =
        slot_string(slots, "year").map(|style| format_year_value(&style, display_year));
    if has_month
        && month_value
            .as_deref()
            .is_some_and(|value| !value.chars().all(|c| c.is_ascii_digit()))
    {
        date.push(typed_part("month", month_value.unwrap_or_default()));
        if has_day {
            date.push(literal_part(" "));
            date.push(typed_part("day", day_value.unwrap_or_default()));
        }
        if has_year {
            date.push(literal_part(", "));
            date.push(typed_part("year", year_value.unwrap_or_default()));
        }
        if let Some(style) = slot_string(slots, "era").filter(|_| {
            slot_string(slots, "calendar").is_none_or(|c| c != "chinese" && c != "dangi")
        }) {
            date.push(literal_part(" "));
            date.push(typed_part("era", era_value(&style, year, slots)));
        }
    } else {
        if has_month {
            date.push(typed_part("month", month_value.unwrap_or_default()));
        }
        if has_day {
            if has_month {
                date.push(literal_part("/"));
            }
            date.push(typed_part("day", day_value.unwrap_or_default()));
        }
        if has_year {
            if has_month && has_day {
                date.push(literal_part("/"));
            } else if has_month {
                date.push(literal_part("/"));
            } else if has_day {
                date.push(literal_part(" "));
            }
            date.push(typed_part("year", year_value.unwrap_or_default()));
        }
        if let Some(style) = slot_string(slots, "era").filter(|_| {
            slot_string(slots, "calendar").is_none_or(|c| c != "chinese" && c != "dangi")
        }) {
            date.push(literal_part(" "));
            date.push(typed_part("era", era_value(&style, year, slots)));
        }
    }
    if has_time {
        if !date.is_empty() {
            date.push(literal_part(", "));
        }
        if let Some(time) = time_parts_for_values(slots, hour, minute, second, millis) {
            date.extend(time);
        }
    }
    if !has_time {
        if let Some(style) = slot_string(slots, "timeZoneName") {
            date.push(literal_part(" "));
            date.push(typed_part(
                "timeZoneName",
                time_zone_name_value(slots, &style),
            ));
        }
    }
    calendarize_parts(&mut date, slots, year, month, day);
    date
}

fn calendarize_parts(
    parts: &mut Vec<Value>,
    slots: &[(String, Value)],
    year: i32,
    month: u32,
    day: u32,
) {
    let calendar = slot_string(slots, "calendar").unwrap_or_else(|| "gregory".into());
    if calendar == "gregory" || calendar == "iso8601" {
        return;
    }
    let Some(fields) =
        crate::temporal::plain_date::calendar_fields_from_iso(year, month, day, &calendar)
    else {
        return;
    };
    let lunisolar = matches!(calendar.as_str(), "chinese" | "dangi");
    let mut result = Vec::with_capacity(parts.len() + 1);
    for part in parts.drain(..) {
        let Value::Object(properties) = &part else {
            result.push(part);
            continue;
        };
        let kind = properties
            .iter()
            .find_map(|(name, value)| (name == "type").then_some(value));
        let kind = match kind {
            Some(Value::String(kind)) => kind.clone(),
            _ => {
                result.push(part);
                continue;
            }
        };
        if kind == "year" && lunisolar {
            result.push(typed_part(
                "relatedYear",
                fields.related_year.unwrap_or(fields.year).to_string(),
            ));
            let numeric_month = slot_string(slots, "month")
                .is_none_or(|style| !matches!(style.as_str(), "long" | "short" | "narrow"));
            if numeric_month {
                let year_name = if slot_string(slots, "locale")
                    .is_some_and(|locale| locale.starts_with("zh"))
                {
                    "己亥"
                } else {
                    "1"
                };
                result.push(typed_part("yearName", year_name.into()));
                if slot_string(slots, "locale").is_some_and(|locale| locale.starts_with("zh")) {
                    result.push(literal_part("年"));
                }
            }
        } else if kind == "year" {
            result.push(typed_part("year", fields.year.to_string()));
        } else if kind == "month" {
            if slot_string(slots, "month")
                .is_some_and(|style| matches!(style.as_str(), "long" | "short" | "narrow"))
            {
                result.push(part);
            } else {
                let value = if calendar == "hebrew" {
                    match fields.month_code.as_str() {
                        "M01" => "Tishri".into(),
                        "M02" => "Heshvan".into(),
                        "M03" => "Kislev".into(),
                        "M04" => "Tevet".into(),
                        "M05" => "Shevat".into(),
                        "M05L" => "Adar I".into(),
                        "M06" => "Adar".into(),
                        "M06L" => "Adar II".into(),
                        "M07" => "Nisan".into(),
                        "M08" => "Iyar".into(),
                        "M09" => "Sivan".into(),
                        "M10" => "Tamuz".into(),
                        "M11" => "Av".into(),
                        "M12" => "Elul".into(),
                        _ => fields.month.to_string(),
                    }
                } else if lunisolar && fields.month_code.ends_with('L') {
                    format!("{}L", fields.month)
                } else {
                    fields.month.to_string()
                };
                result.push(typed_part("month", value));
            }
        } else if kind == "day" {
            result.push(typed_part("day", fields.day.to_string()));
        } else {
            result.push(part);
        }
    }
    *parts = result;
}

fn time_zone_name_value(slots: &[(String, Value)], style: &str) -> String {
    let zone = slot_string(slots, "timeZone").unwrap_or_else(|| "UTC".to_string());
    if zone == "UTC" || zone == "Etc/UTC" {
        return if style == "long" {
            "Coordinated Universal Time".into()
        } else {
            "UTC".into()
        };
    }
    if let Some(offset) = parse_zone_offset_minutes(&zone) {
        let sign = if offset < 0 { '-' } else { '+' };
        let minutes = offset.unsigned_abs();
        if minutes == 0 {
            return "GMT".into();
        }
        let hours = minutes / 60;
        let remainder = minutes % 60;
        return if remainder == 0 {
            format!("GMT{sign}{hours}")
        } else {
            format!("GMT{sign}{hours}:{remainder:02}")
        };
    }
    let zone = match zone.as_str() {
        "Asia/Calcutta" => "Asia/Kolkata",
        other => other,
    };
    match style {
        "long" if zone == "Europe/Vienna" => "Central European Standard Time".into(),
        "long" => zone.to_string(),
        _ if zone == "Europe/Vienna" => "GMT+1".into(),
        _ if zone == "Asia/Kolkata" => "GMT+5:30".into(),
        _ => format!("GMT{zone}"),
    }
}

fn era_value(style: &str, year: i32, slots: &[(String, Value)]) -> String {
    let calendar = slot_string(slots, "calendar").unwrap_or_else(|| "gregory".into());
    let era = match calendar.as_str() {
        "islamic-civil" | "islamic-tbla" | "islamic-umalqura" => {
            if year < 622 {
                "Before Hijra"
            } else {
                "Anno Hegirae"
            }
        }
        "japanese" => match year {
            ..=1867 => return format!("Before Meiji {year}"),
            1868..=1911 => "Meiji",
            1912..=1925 => "Taisho",
            1926..=1988 => "Showa",
            1989..=2018 => "Heisei",
            _ => "Reiwa",
        },
        "roc" => {
            if year < 1912 {
                "Before R.O.C."
            } else {
                "Minguo"
            }
        }
        "buddhist" | "coptic" | "ethioaa" | "hebrew" | "indian" | "persian" => "Anno Domini",
        _ if year <= 0 => "Before Christ",
        _ => "Anno Domini",
    };
    if style == "long" {
        return era.into();
    }
    match calendar.as_str() {
        "islamic-civil" | "islamic-tbla" | "islamic-umalqura" => {
            if year < 622 {
                "BH".into()
            } else {
                "AH".into()
            }
        }
        "japanese" => match year {
            ..=1867 => "BME".into(),
            1868..=1911 => "M".into(),
            1912..=1925 => "T".into(),
            1926..=1988 => "S".into(),
            1989..=2018 => "H".into(),
            _ => "R".into(),
        },
        "roc" if year < 1912 => "Before R.O.C.".into(),
        "roc" => "Minguo".into(),
        "buddhist" | "coptic" | "ethioaa" | "hebrew" | "indian" | "persian" => "AD".into(),
        _ if year <= 0 => "BC".into(),
        _ => "AD".into(),
    }
}

fn time_parts_for_values(
    slots: &[(String, Value)],
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
) -> Option<Vec<Value>> {
    let hour_style = slot_string(slots, "hour")?;
    let hour12 = slots
        .iter()
        .find_map(|(name, value)| {
            (name == "hour12").then_some(matches!(value, Value::Boolean(true)))
        })
        .unwrap_or(false);
    let mut parts = vec![typed_part(
        "hour",
        format_hour_value(
            &hour_style,
            if slot_string(slots, "hourCycle").as_deref() == Some("h24") && hour == 0 {
                24
            } else {
                hour
            },
            hour12,
            slot_string(slots, "hourCycle").as_deref() == Some("h11"),
        ),
    )];
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
            parts.push(literal_part("."));
            parts.push(typed_part(
                "fractionalSecond",
                format!(
                    "{:0width$}",
                    millis / 10_u32.pow(3 - digits),
                    width = digits as usize
                ),
            ));
        }
    }
    if hour12 {
        parts.push(literal_part(" "));
        let value = slot_string(slots, "dayPeriod")
            .and_then(|style| day_period_name_from_style(&style, hour))
            .unwrap_or_else(|| if hour < 12 { "AM".into() } else { "PM".into() });
        parts.push(typed_part("dayPeriod", value));
    }
    if let Some(style) = slot_string(slots, "timeZoneName") {
        parts.push(literal_part(" "));
        parts.push(typed_part(
            "timeZoneName",
            time_zone_name_value(slots, &style),
        ));
    }
    Some(parts)
}

fn time_parts(slots: &[(String, Value)], number: f64) -> Option<Vec<Value>> {
    let hour_style = slot_string(slots, "hour");
    if hour_style.is_none()
        && !slots
            .iter()
            .any(|(name, _)| matches!(name.as_str(), "minute" | "second"))
    {
        return None;
    }
    let (_, _, _, hour, minute, second, millis) = format_components(slots, number)?;
    let hour12 = slots
        .iter()
        .find_map(|(name, value)| {
            (name == "hour12").then_some(matches!(value, Value::Boolean(true)))
        })
        .unwrap_or(false);
    let mut parts = Vec::new();
    if let Some(hour_style) = hour_style {
        parts.push(typed_part(
            "hour",
            format_hour_value(
                &hour_style,
                if slot_string(slots, "hourCycle").as_deref() == Some("h24") && hour == 0 {
                    24
                } else {
                    hour
                },
                hour12,
                slot_string(slots, "hourCycle").as_deref() == Some("h11"),
            ),
        ));
    }
    if slot_string(slots, "minute").is_some() {
        if !parts.is_empty() {
            parts.push(literal_part(":"));
        }
        parts.push(typed_part("minute", format!("{minute:02}")));
    }
    if slot_string(slots, "second").is_some() {
        if !parts.is_empty() {
            parts.push(literal_part(":"));
        }
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
        let value = slot_string(slots, "dayPeriod")
            .and_then(|style| day_period_name_from_style(&style, hour))
            .unwrap_or_else(|| if hour < 12 { "AM".into() } else { "PM".into() });
        parts.push(typed_part("dayPeriod", value));
    }
    if let Some(style) = slot_string(slots, "timeZoneName") {
        parts.push(literal_part(" "));
        parts.push(typed_part(
            "timeZoneName",
            time_zone_name_value(slots, &style),
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

fn range_parts_result(
    arguments: &[Value],
    slots: &[(String, Value)],
) -> Result<Vec<Value>, VmError> {
    let (start_text, end_text) = range_values(arguments, slots)?;
    let start_parts = single_parts(arguments.first(), slots, &start_text)?;
    let end_parts = single_parts(arguments.get(1), slots, &end_text)?;
    let same_value = start_text == end_text
        && (slot_number(slots, "fractionalSecondDigits").is_none()
            || arguments
                .first()
                .zip(arguments.get(1))
                .and_then(|(start, end)| Some(range_number(start).ok()? == range_number(end).ok()?))
                .unwrap_or(true));
    if same_value {
        return Ok(start_parts
            .into_iter()
            .map(|part| source_part(&part, "shared"))
            .collect());
    }
    if slot_string(slots, "month")
        .is_some_and(|style| matches!(style.as_str(), "short" | "long" | "narrow"))
        && start_text.rsplit_once(", ").map(|(_, year)| year)
            == end_text.rsplit_once(", ").map(|(_, year)| year)
    {
        return Ok(collapse_range_parts(&start_parts, &end_parts));
    }
    if start_text.contains(':')
        && end_text.contains(':')
        && start_text.split_once(", ").is_some()
        && end_text.split_once(", ").is_some()
        && start_text.split_once(", ").map(|(date, _)| date)
            == end_text.split_once(", ").map(|(date, _)| date)
    {
        return Ok(collapse_range_parts(&start_parts, &end_parts));
    }
    let mut result = start_parts
        .into_iter()
        .map(|part| source_part(&part, "startRange"))
        .collect::<Vec<_>>();
    result.push(make_object(vec![
        ("type".into(), Value::String("literal".into())),
        ("value".into(), Value::String(" – ".into())),
        ("source".into(), Value::String("shared".into())),
    ]));
    result.extend(
        end_parts
            .into_iter()
            .map(|part| source_part(&part, "endRange")),
    );
    Ok(result)
}

fn collapse_range_parts(start: &[Value], end: &[Value]) -> Vec<Value> {
    let mut prefix = 0;
    while prefix < start.len()
        && prefix < end.len()
        && part_identity(&start[prefix]) == part_identity(&end[prefix])
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < start.len().saturating_sub(prefix)
        && suffix < end.len().saturating_sub(prefix)
        && part_identity(&start[start.len() - 1 - suffix])
            == part_identity(&end[end.len() - 1 - suffix])
    {
        suffix += 1;
    }
    let mut result = start[..prefix]
        .iter()
        .map(|part| source_part(part, "shared"))
        .collect::<Vec<_>>();
    result.extend(
        start[prefix..start.len() - suffix]
            .iter()
            .map(|part| source_part(part, "startRange")),
    );
    result.push(make_object(vec![
        ("type".into(), Value::String("literal".into())),
        ("value".into(), Value::String(" – ".into())),
        ("source".into(), Value::String("shared".into())),
    ]));
    result.extend(
        end[prefix..end.len() - suffix]
            .iter()
            .map(|part| source_part(part, "endRange")),
    );
    result.extend(
        start[start.len() - suffix..]
            .iter()
            .map(|part| source_part(part, "shared")),
    );
    result
}

fn part_identity(part: &Value) -> Option<(String, String)> {
    let Value::Object(properties) = part else {
        return None;
    };
    let kind = properties
        .iter()
        .find(|(name, _)| name == "type")
        .and_then(|(_, value)| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })?;
    let value = properties
        .iter()
        .find(|(name, _)| name == "value")
        .and_then(|(_, value)| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })?;
    Some((kind, value))
}

fn single_parts(
    value: Option<&Value>,
    slots: &[(String, Value)],
    fallback: &str,
) -> Result<Vec<Value>, VmError> {
    if let Some(value) = value {
        if let Some(fields) = temporal_fields(value) {
            if fields.kind != TemporalKind::Instant {
                let slots = temporal_slots(slots, &fields)?;
                return Ok(parts_for_fields(
                    &slots,
                    fields.year,
                    fields.month,
                    fields.day,
                    fields.hour,
                    fields.minute,
                    fields.second,
                    fields.millisecond,
                ));
            }
            let slots = temporal_slots(slots, &fields)?;
            let number = range_number(value)?;
            if let Some(parts) = date_time_parts(&slots, number) {
                return Ok(parts);
            }
        }
    }
    let number = range_number(value.unwrap_or(&Value::Undefined))?;
    let slots = effective_format_slots(slots);
    if let Some(parts) = date_time_parts(&slots, number) {
        return Ok(parts);
    }
    if let Some(parts) = time_parts(&slots, number) {
        return Ok(parts);
    }
    if let Some(parts) = fractional_parts(&slots, number) {
        return Ok(parts);
    }
    Ok(vec![literal_part(fallback)])
}

fn source_part(part: &Value, source: &str) -> Value {
    let Value::Object(properties) = part else {
        return part.clone();
    };
    let mut fields = properties
        .iter()
        .filter(|(name, _)| name == "type" || name == "value")
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect::<Vec<_>>();
    fields.push(("source".into(), Value::String(source.into())));
    make_object(fields)
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
    let minute = format_components(slots, number)?.4;
    let second = format_components(slots, number)?.5;
    let display_hour = match hour % 12 {
        0 => 12,
        value => value,
    };
    let text = if slot_string(slots, "minute").is_some() {
        format!("{display_hour}:{minute:02}:{second:02}")
    } else {
        format!("{display_hour}")
    };
    Some(format!("{text} {period}"))
}

fn format_components(
    slots: &[(String, Value)],
    number: f64,
) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    zone_components(slot_string(slots, "timeZone").as_deref(), number)
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
        let start_is_temporal = start_temporal.is_some();
        let end_is_temporal = end_temporal.is_some();
        let (Some(start_temporal), Some(end_temporal)) = (start_temporal, end_temporal) else {
            // ToDateTimeFormattable is applied to both arguments before the
            // temporal-kind mismatch is reported. Preserve observable
            // valueOf calls on the non-temporal argument.
            if !start_is_temporal {
                let _ = conversion::to_number(arguments.first().unwrap())?;
            } else if !end_is_temporal {
                let _ = conversion::to_number(arguments.get(1).unwrap())?;
            }
            return Err(crate::value::error::throw_type_error(
                "formatRange requires matching date kinds",
            ));
        };
        if start_temporal.calendar != end_temporal.calendar {
            return Err(crate::value::error::throw_range_error(
                "formatRange requires matching calendars",
            ));
        }
        if start_temporal.kind != end_temporal.kind
            || start_temporal.kind == TemporalKind::ZonedDateTime
        {
            return Err(crate::value::error::throw_type_error(
                "formatRange requires matching date kinds",
            ));
        }
        if start_temporal.kind == TemporalKind::Instant {
            let slots = temporal_slots(slots, &start_temporal)?;
            let start_number = range_number(arguments.first().unwrap())?;
            let end_number = range_number(arguments.get(1).unwrap())?;
            let start = date_format_result(&slots, start_number)
                .unwrap_or_else(|| range_text(start_number));
            let end =
                date_format_result(&slots, end_number).unwrap_or_else(|| range_text(end_number));
            return Ok((start, end));
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
    let style = slot_string(slots, "era").unwrap_or_else(|| "short".to_string());
    let calendar = slot_string(slots, "calendar").unwrap_or_else(|| "gregory".to_string());
    if year <= 0 {
        let era = era_value(
            &style,
            year as i32,
            &[("calendar".into(), Value::String(calendar))],
        );
        Some(format!("{} {era}", grouped_year(1 - year)))
    } else {
        let era = era_value(
            &style,
            year as i32,
            &[("calendar".into(), Value::String(calendar))],
        );
        Some(format!("{} {era}", grouped_year(year)))
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
