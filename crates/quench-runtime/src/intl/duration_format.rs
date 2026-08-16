fn format_duration_values(
    slots: &[(String, Value)],
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) -> Result<String, VmError> {
    let negative = [days, hours, minutes, seconds]
        .iter()
        .any(|value| *value < 0);
    let days = days.abs();
    let hours = hours.abs();
    let minutes = minutes.abs();
    let seconds = seconds.abs();
    let style = duration_style(slots);
    format_duration_shape(
        slots,
        style,
        negative,
        days,
        hours,
        minutes,
        seconds,
        milliseconds,
        microseconds,
        nanoseconds,
    )
}

fn format_duration_shape(
    slots: &[(String, Value)],
    style: &str,
    negative: bool,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) -> Result<String, VmError> {
    if matches!(slot_value(slots, "minutes"), Some("numeric" | "2-digit"))
        && matches!(slot_value(slots, "seconds"), Some("numeric" | "2-digit"))
    {
        return Ok(format_clock_duration(days, hours, minutes, seconds));
    }
    if style == "digital" {
        return Ok(format_digital_duration(
            slots,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
            negative,
        ));
    }
    Ok(format_standard_duration(
        slots,
        hours,
        minutes,
        seconds,
        milliseconds,
        microseconds,
        nanoseconds,
    ))
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
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
    negative: bool,
) -> String {
    let subsecond = milliseconds.abs() * 1_000_000 + microseconds.abs() * 1_000 + nanoseconds.abs();
    let seconds = seconds + subsecond / 1_000_000_000;
    let remainder = subsecond % 1_000_000_000;
    let mut clock = if hours == 0 {
        format!("{minutes:02}:{seconds:02}")
    } else {
        format!("{hours}:{minutes:02}:{seconds:02}")
    };
    if let Some(fraction) = digital_fraction(slots, remainder) {
        clock.push_str(&fraction);
    }
    let sign = if negative { "-" } else { "" };
    if days == 0 {
        format!("{sign}{clock}")
    } else {
        format!("{sign}{}, {clock}", format_days(days))
    }
}

fn format_standard_duration(
    slots: &[(String, Value)],
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) -> String {
    let mut parts = Vec::new();
    if hours != 0 {
        parts.push(format!("{hours} hr"));
    }
    if minutes != 0 {
        parts.push(format!("{minutes} min"));
    }
    if seconds != 0 {
        parts.push(format!("{seconds} sec"));
    }
    append_subsecond_parts(&mut parts, slots, milliseconds, microseconds, nanoseconds);
    parts.join(", ")
}
