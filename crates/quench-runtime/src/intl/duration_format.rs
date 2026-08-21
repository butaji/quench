struct DurationFields {
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
}

fn format_duration_values(
    slots: &[(String, Value)],
    fields: DurationFields,
) -> Result<String, VmError> {
    let negative = [fields.days, fields.hours, fields.minutes, fields.seconds]
        .iter()
        .any(|value| *value < 0);
    let abs = DurationFields {
        years: fields.years.saturating_abs(),
        months: fields.months.saturating_abs(),
        weeks: fields.weeks.saturating_abs(),
        days: fields.days.saturating_abs(),
        hours: fields.hours.saturating_abs(),
        minutes: fields.minutes.saturating_abs(),
        seconds: fields.seconds.saturating_abs(),
        milliseconds: fields.milliseconds.saturating_abs(),
        microseconds: fields.microseconds.saturating_abs(),
        nanoseconds: fields.nanoseconds.saturating_abs(),
    };
    let style = duration_style(slots);
    format_duration_shape(slots, style, negative, abs)
}

fn format_duration_shape(
    slots: &[(String, Value)],
    style: &str,
    negative: bool,
    fields: DurationFields,
) -> Result<String, VmError> {
    if matches!(slot_value(slots, "minutes"), Some("numeric" | "2-digit"))
        && matches!(slot_value(slots, "seconds"), Some("numeric" | "2-digit"))
    {
        return Ok(format_clock_duration(fields.days, fields.hours, fields.minutes, fields.seconds));
    }
    if style == "digital" {
        return Ok(format_digital_duration(slots, fields, negative));
    }
    Ok(format_standard_duration(slots, fields, negative))
}

fn duration_style(slots: &[(String, Value)]) -> &str {
    slots
        .iter()
        .find_map(|(key, value)| (key == "style").then_some(value))
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("short")
}

fn duration_values(properties: &[(String, Value)]) -> [i64; 10] {
    [
        number(properties, "years"),
        number(properties, "months"),
        number(properties, "weeks"),
        number(properties, "days"),
        number(properties, "hours"),
        number(properties, "minutes"),
        number(properties, "seconds"),
        number(properties, "milliseconds"),
        number(properties, "microseconds"),
        number(properties, "nanoseconds"),
    ]
}

fn fields_from(values: [i64; 10]) -> DurationFields {
    DurationFields {
        years: values[0],
        months: values[1],
        weeks: values[2],
        days: values[3],
        hours: values[4],
        minutes: values[5],
        seconds: values[6],
        milliseconds: values[7],
        microseconds: values[8],
        nanoseconds: values[9],
    }
}

fn format_clock_duration(days: i64, hours: i64, minutes: i64, seconds: i64) -> String {
    let clock = format!("{minutes}:{seconds:02}");
    let time = if hours == 0 {
        clock
    } else {
        format!("{hours} hr, {clock}")
    };
    if days == 0 {
        time
    } else {
        format!("{days} day, {time}")
    }
}

fn format_digital_duration(
    slots: &[(String, Value)],
    fields: DurationFields,
    negative: bool,
) -> String {
    let subsecond = fields.milliseconds.abs() * 1_000_000
        + fields.microseconds.abs() * 1_000
        + fields.nanoseconds.abs();
    let seconds = fields.seconds + subsecond / 1_000_000_000;
    let remainder = subsecond % 1_000_000_000;
    let mut clock = if fields.hours == 0 {
        format!("{minutes:02}:{seconds:02}", minutes = fields.minutes)
    } else {
        format!("{hours}:{minutes:02}:{seconds:02}", hours = fields.hours, minutes = fields.minutes)
    };
    if let Some(fraction) = digital_fraction(slots, remainder) {
        clock.push_str(&fraction);
    }
    let sign = if negative { "-" } else { "" };
    if fields.days == 0 {
        format!("{sign}{clock}")
    } else {
        format!(
            "{sign}{}, {clock}",
            format_days(fields.days)
        )
    }
}

fn format_standard_duration(
    slots: &[(String, Value)],
    fields: DurationFields,
    negative: bool,
) -> String {
    let mut parts = Vec::new();
    if fields.years != 0 {
        parts.push(format!("{} yr", fields.years));
    }
    if fields.months != 0 {
        parts.push(format!("{} mo", fields.months));
    }
    if fields.weeks != 0 {
        parts.push(format!("{} wk", fields.weeks));
    }
    if fields.days != 0 {
        parts.push(format!("{} day", fields.days));
    }
    if fields.hours != 0 {
        parts.push(format!("{} hr", fields.hours));
    }
    if fields.minutes != 0 {
        parts.push(format!("{} min", fields.minutes));
    }
    if fields.seconds != 0 {
        parts.push(format!("{} sec", fields.seconds));
    }
    append_subsecond_parts(
        &mut parts,
        slots,
        fields.milliseconds,
        fields.microseconds,
        fields.nanoseconds,
    );
    let result = parts.join(", ");
    if negative && !result.is_empty() {
        format!("-{result}")
    } else {
        result
    }
}
