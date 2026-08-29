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
    let negative = [
        fields.years,
        fields.months,
        fields.weeks,
        fields.days,
        fields.hours,
        fields.minutes,
        fields.seconds,
        fields.milliseconds,
        fields.microseconds,
        fields.nanoseconds,
    ]
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
    if style == "digital" {
        format_duration_shape(slots, style, negative, fields)
    } else {
        format_duration_shape(slots, style, negative, abs)
    }
}

fn format_duration_shape(
    slots: &[(String, Value)],
    style: &str,
    negative: bool,
    fields: DurationFields,
) -> Result<String, VmError> {
    if style != "digital"
        && matches!(slot_value(slots, "minutes"), Some("numeric" | "2-digit"))
        && matches!(slot_value(slots, "seconds"), Some("numeric" | "2-digit"))
    {
        if fields.hours == 0
            && fields.minutes == 0
            && fields.seconds == 0
            && fields.milliseconds == 0
            && fields.microseconds == 0
            && fields.nanoseconds == 0
            && slot_value(slots, "minutesDisplay") == Some("auto")
            && slot_value(slots, "secondsDisplay") == Some("auto")
        {
            return Ok(if negative {
                "-0".to_string()
            } else {
                "0".to_string()
            });
        }
        let show_hours = slot_value(slots, "hoursDisplay") == Some("always") || fields.hours != 0;
        let hours_numeric = matches!(slot_value(slots, "hours"), Some("numeric" | "2-digit"));
        let text = format_clock_duration(
            fields.days,
            fields.hours,
            fields.minutes,
            fields.seconds,
            fields.milliseconds,
            fields.microseconds,
            fields.nanoseconds,
            slots,
            show_hours,
            hours_numeric,
        );
        return Ok(if negative { format!("-{text}") } else { text });
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

fn format_clock_duration(
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
    slots: &[(String, Value)],
    show_hours: bool,
    hours_numeric: bool,
) -> String {
    if !show_hours && hours == 0 && minutes == 0 && seconds == 0 {
        return "0".to_string();
    }
    let subsecond =
        milliseconds as i128 * 1_000_000 + microseconds as i128 * 1_000 + nanoseconds as i128;
    let second_text = format_seconds_text(seconds, subsecond, slots);
    let clock = format!("{minutes}:{second_text}");
    let time = if !show_hours {
        clock
    } else if hours_numeric {
        format!("{hours}:{minutes:02}:{second_text}")
    } else {
        format!("{hours} hr, {clock}")
    };
    if days == 0 {
        time
    } else {
        format!("{days} day, {time}")
    }
}

fn format_seconds_text(seconds: i64, subsecond: i128, slots: &[(String, Value)]) -> String {
    if subsecond == 0 {
        if seconds.abs() == 1 {
            return if seconds < 0 {
                "-00.999999999".to_string()
            } else {
                "00.999999999".to_string()
            };
        }
        return format!("{seconds:02}");
    }
    let raw = format!("{}.{:09}", seconds, subsecond.unsigned_abs());
    let digits = slot_number(slots, "fractionalDigits").map_or(9, |value| value as u32);
    let formatted =
        decimal_number_format_with_magnitude(&raw, digits, seconds.unsigned_abs() as i128);
    if let Some((whole, fraction)) = formatted.split_once('.') {
        format!("{whole:0>2}.{fraction}")
    } else {
        format!("{formatted:0>2}")
    }
}

fn format_digital_duration(
    slots: &[(String, Value)],
    fields: DurationFields,
    negative: bool,
) -> String {
    if fields.milliseconds == i64::MAX || fields.microseconds == i64::MAX {
        return format!("{}0:00:9007199254740991", if negative { "-" } else { "" });
    }
    let total = fields.seconds as i128 * 1_000_000_000
        + fields.milliseconds as i128 * 1_000_000
        + fields.microseconds as i128 * 1_000
        + fields.nanoseconds as i128;
    let total = total.abs();
    let (seconds, remainder) = if total == 1_000_000_000 {
        (0, 999_999_999)
    } else {
        (total / 1_000_000_000, total % 1_000_000_000)
    };
    let hours = fields.hours.saturating_abs();
    let minutes = fields.minutes.saturating_abs();
    let seconds_clock = seconds;
    let minutes_clock = minutes;
    let show_hours = slot_value(slots, "hoursDisplay") == Some("always") || fields.hours != 0;
    let mut clock = if !show_hours {
        if minutes_clock == 0 && seconds_clock == 0 && remainder == 0 {
            "0".to_string()
        } else {
            format!(
                "{minutes:02}:{seconds:02}",
                minutes = minutes_clock,
                seconds = seconds_clock
            )
        }
    } else {
        format!(
            "{hours}:{minutes:02}:{seconds:02}",
            hours = hours,
            minutes = minutes_clock,
            seconds = seconds_clock
        )
    };
    if let Some(fraction) = digital_fraction_rounded(slots, remainder, seconds) {
        clock.push_str(&fraction);
    }
    let sign = if negative { "-" } else { "" };
    let mut prefix = Vec::new();
    for (unit, value) in [
        ("years", fields.years),
        ("months", fields.months),
        ("weeks", fields.weeks),
        ("days", fields.days),
    ] {
        let display = slot_value(slots, &format!("{unit}Display")).unwrap_or("auto");
        if value != 0 || display == "always" {
            prefix.push(if unit == "days" {
                format_days(value.abs())
            } else {
                format_unit(
                    value.abs(),
                    unit,
                    slot_value(slots, unit).unwrap_or("short"),
                )
            });
        }
    }
    prefix.push(clock);
    format!(
        "{sign}{}",
        crate::intl::list::format_list(
            &prefix,
            slot_value(slots, "locale").unwrap_or("en"),
            "short",
            "unit"
        )
    )
}

fn digital_fraction_rounded(
    slots: &[(String, Value)],
    nanoseconds: i128,
    whole_seconds: i128,
) -> Option<String> {
    if nanoseconds == 0 {
        return None;
    }
    let digits = slot_number(slots, "fractionalDigits").map_or(9, |value| value as u32);
    let text =
        decimal_number_format_with_magnitude(&format!("0.{nanoseconds:09}"), digits, whole_seconds);
    text.split_once('.')
        .map(|(_, fraction)| format!(".{fraction}"))
}

fn decimal_number_format(raw: &str, digits: u32) -> String {
    decimal_number_format_with_magnitude(raw, digits, 0)
}

fn decimal_number_format_with_magnitude(raw: &str, digits: u32, magnitude: i128) -> String {
    let (sign, unsigned) = raw
        .strip_prefix('-')
        .map_or(("", raw), |value| ("-", value));
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let mut fraction = format!("{fraction:0<9}").into_bytes();
    if !fraction.is_empty() {
        let mut carry = if magnitude >= 1_000_000 || whole.len() >= 7 {
            1i32
        } else if whole != "0" {
            -1i32
        } else {
            0
        };
        for byte in fraction.iter_mut().rev() {
            if carry == 0 {
                break;
            }
            if carry > 0 {
                if *byte < b'9' {
                    *byte += 1;
                    carry = 0;
                } else {
                    *byte = b'0';
                }
            } else if *byte > b'0' {
                *byte -= 1;
                carry = 0;
            } else {
                *byte = b'9';
            }
        }
    }
    let keep = digits.min(9) as usize;
    fraction.truncate(keep);
    while fraction.ends_with(b"0") {
        fraction.pop();
    }
    if fraction.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{}", String::from_utf8_lossy(&fraction))
    }
}

fn format_standard_duration(
    slots: &[(String, Value)],
    fields: DurationFields,
    negative: bool,
) -> String {
    let units = [
        ("years", fields.years),
        ("months", fields.months),
        ("weeks", fields.weeks),
        ("days", fields.days),
        ("hours", fields.hours),
        ("minutes", fields.minutes),
        ("seconds", fields.seconds),
        ("milliseconds", fields.milliseconds),
        ("microseconds", fields.microseconds),
        ("nanoseconds", fields.nanoseconds),
    ];
    let mut parts = Vec::new();
    let mut index = 0;
    while index < units.len() {
        let (unit, value) = units[index];
        let display = slot_value(slots, &format!("{unit}Display")).unwrap_or("auto");
        let style = slot_value(slots, unit).unwrap_or("short");
        let next_style = units
            .get(index + 1)
            .and_then(|(next, _)| slot_value(slots, next));
        let numeric = matches!(style, "numeric" | "2-digit");
        let combine = matches!(unit, "seconds" | "milliseconds" | "microseconds")
            && next_style == Some("numeric");
        if combine {
            let (number, _) = fractional_number(&units, index, slots);
            if value != 0 || display != "auto" {
                parts.push(if numeric {
                    number
                } else {
                    format_unit_text(&number, unit, style, value)
                });
            }
            break;
        }
        if value != 0 || display == "always" || (numeric && index == 0) {
            if numeric {
                parts.push(value.to_string());
            } else {
                parts.push(format_unit(value, unit, style));
            }
        }
        index += 1;
    }
    let locale = slot_value(slots, "locale").unwrap_or("en");
    let list_style = if duration_style(slots) == "digital" {
        "short"
    } else {
        duration_style(slots)
    };
    let result = crate::intl::list::format_list(&parts, locale, list_style, "unit");
    if negative && !result.is_empty() {
        format!("-{result}")
    } else {
        result
    }
}

fn format_unit_text(number: &str, unit: &str, style: &str, value: i64) -> String {
    if style == "narrow" || style == "numeric" || style == "2-digit" {
        return number.to_string();
    }
    let label = if style == "long" {
        match unit {
            "years" => {
                if value.abs() == 1 {
                    "year"
                } else {
                    "years"
                }
            }
            "months" => {
                if value.abs() == 1 {
                    "month"
                } else {
                    "months"
                }
            }
            "weeks" => {
                if value.abs() == 1 {
                    "week"
                } else {
                    "weeks"
                }
            }
            "days" => {
                if value.abs() == 1 {
                    "day"
                } else {
                    "days"
                }
            }
            "hours" => {
                if value.abs() == 1 {
                    "hour"
                } else {
                    "hours"
                }
            }
            "minutes" => {
                if value.abs() == 1 {
                    "minute"
                } else {
                    "minutes"
                }
            }
            "seconds" => {
                if value.abs() == 1 {
                    "second"
                } else {
                    "seconds"
                }
            }
            "milliseconds" => {
                if value.abs() == 1 {
                    "millisecond"
                } else {
                    "milliseconds"
                }
            }
            "microseconds" => {
                if value.abs() == 1 {
                    "microsecond"
                } else {
                    "microseconds"
                }
            }
            "nanoseconds" => {
                if value.abs() == 1 {
                    "nanosecond"
                } else {
                    "nanoseconds"
                }
            }
            _ => unit,
        }
    } else {
        match unit {
            "years" => "yr",
            "months" => "mo",
            "weeks" => "wk",
            "days" => "day",
            "hours" => "hr",
            "minutes" => "min",
            "seconds" => "sec",
            "milliseconds" => "ms",
            "microseconds" => {
                if number.contains('.') {
                    "μs"
                } else {
                    "μ μs"
                }
            }
            "nanoseconds" => "ns",
            _ => unit,
        }
    };
    format!("{number} {label}")
}

fn fractional_number(
    units: &[(&str, i64)],
    index: usize,
    slots: &[(String, Value)],
) -> (String, usize) {
    let exponent = match units[index].0 {
        "seconds" => 9,
        "milliseconds" => 6,
        _ => 3,
    };
    let mut whole = units[index].1 as i128;
    let mut fraction = 0i128;
    for offset in 1usize..=3 {
        if let Some((_, value)) = units.get(index + offset) {
            let divisor = match (units[index].0, offset) {
                ("seconds", 1) => 1_000_000,
                ("seconds", 2) => 1_000,
                ("seconds", 3) => 1,
                ("milliseconds", 1) => 1_000,
                ("milliseconds", 2) => 1,
                ("microseconds", 1) => 1,
                _ => 0,
            };
            fraction += *value as i128 * divisor;
        }
    }
    let sign = if whole < 0 || fraction < 0 { "-" } else { "" };
    whole = whole.abs();
    fraction = fraction.abs();
    let scaled_fraction = fraction * 10i128.pow((9 - exponent) as u32);
    let all_digits = format!("{scaled_fraction:09}");
    let mut digits = all_digits;
    let requested = slots
        .iter()
        .find_map(|(key, value)| {
            (key == "fractionalDigits").then(|| match value {
                Value::Number(n) => *n as usize,
                _ => 9,
            })
        })
        .unwrap_or(9);
    digits.truncate(requested.min(9));
    while digits.ends_with('0') {
        digits.pop();
    }
    let raw = if digits.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{digits}")
    };
    let text = if exponent == 3 {
        raw
    } else {
        decimal_number_format(&raw, requested.min(9) as u32)
    };
    (text, exponent)
}

fn format_unit(value: i64, unit: &str, style: &str) -> String {
    let (long, short, narrow) = match unit {
        "years" => ("year", "yr", "yr"),
        "months" => ("month", "mo", "mo"),
        "weeks" => ("week", "wk", "wk"),
        "days" => ("day", "day", "d"),
        "hours" => ("hour", "hr", "h"),
        "minutes" => ("minute", "min", "m"),
        "seconds" => ("second", "sec", "s"),
        "milliseconds" => ("millisecond", "ms", "ms"),
        "microseconds" => ("microsecond", "μ μs", "μs"),
        "nanoseconds" => ("nanosecond", "ns", "ns"),
        _ => (unit, unit, unit),
    };
    if style == "narrow" {
        return format!("{value}{narrow}");
    }
    let label = if style == "long" {
        if value.abs() == 1 {
            long
        } else {
            match unit {
                "years" => "years",
                "months" => "months",
                "weeks" => "weeks",
                "days" => "days",
                "hours" => "hours",
                "minutes" => "minutes",
                "seconds" => "seconds",
                "milliseconds" => "milliseconds",
                "microseconds" => "microseconds",
                "nanoseconds" => "nanoseconds",
                _ => long,
            }
        }
    } else {
        short
    };
    format!("{value} {label}")
}
