pub(crate) fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    if let Some(value) = value.filter(|value| crate::value::is_object(value)) {
        if let Value::Object(object) = value {
            let hidden = ["year", "month", "day"].map(|name| {
                object
                    .iter()
                    .find(|(key, _)| key == &format!("\0temporal-slot:\0{name}"))
                    .and_then(|(_, value)| matches!(value, Value::Number(_)).then(|| value.clone()))
            });
            if let [Some(year), Some(month), Some(day)] = hidden {
                let _ = overflow_value(options)?;
                return construct(&[year, month, day]);
            }
            let direct = ["year", "month", "day"].map(|name| {
                object
                    .iter()
                    .find(|(key, value)| {
                        (key == name || key == &format!("\0temporal-slot:\0{name}"))
                            && matches!(value, Value::Number(_))
                    })
                    .map(|(_, value)| value.clone())
            });
            let has_month_code = object.iter().any(|(key, _)| key == "monthCode");
            let temporal_date = object.iter().any(|(key, value)| {
                key == "\0temporal-plain-date"
                    || key == "\0prototype"
                        && matches!(
                            value,
                            Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype)
                                | Value::Builtin(
                                    crate::ops::Builtin::TemporalZonedDateTimePrototype
                                )
                        )
            });
            if !has_month_code || temporal_date {
                if let [Some(year), Some(month), Some(day)] = direct {
                    let _ = overflow_value(options)?;
                    return construct(&[year, month, day]);
                }
            }
        }
        return from_property_bag(value, options);
    }
    let text = match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::StringUnits(_)) => crate::conversion::to_string(value.unwrap())?,
        _ => return Err(crate::value::error::throw_type_error("Invalid PlainDate")),
    };
    if text.starts_with("-000000") || text.contains('−') || has_empty_time_designator(&text) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if has_fractional_minutes(&text)
        || has_invalid_time(&text)
        || has_time_junk(&text)
        || has_annotation_junk(&text)
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if has_utc_designator(&text) || text.starts_with("-000000") {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if has_excess_fraction(&text) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    let calendar_count = text.matches("[u-ca=").count();
    if has_uppercase_annotation_key(&text) {
        return Err(crate::value::error::throw_range_error(
            "Invalid annotation key",
        ));
    }
    if has_invalid_calendar_annotation(&text) {
        return Err(crate::value::error::throw_range_error("Invalid calendar"));
    }
    if has_multiple_time_zones(&text) {
        return Err(crate::value::error::throw_range_error(
            "Multiple time zones",
        ));
    }
    if has_unknown_critical_annotation(&text) {
        return Err(crate::value::error::throw_range_error(
            "Unknown critical annotation",
        ));
    }
    if text.contains("[!u-ca=") && calendar_count > 0 {
        return Err(crate::value::error::throw_range_error("Multiple calendars"));
    }
    let date = date_part(&text);
    let parts = date.split('-').collect::<Vec<_>>();
    if parts.len() == 1 && date.len() == 8 {
        let year = date[..4]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let month = date[4..6]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[6..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let result = checked_date_object(year, month, day)?;
        let _ = overflow_value(options)?;
        return Ok(result);
    }
    if parts.len() == 1 && date.len() == 11 && matches!(date.as_bytes()[0], b'+' | b'-') {
        let year = date[1..7]
            .parse::<i32>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let year = if date.as_bytes()[0] == b'-' {
            -year
        } else {
            year
        };
        let month = date[7..9]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[9..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let result = checked_date_object(year, month, day)?;
        let _ = overflow_value(options)?;
        return Ok(result);
    }
    let (year, month, day) = parse_date_parts(&parts)?;
    let result = checked_date_object(year, month, day)?;
    let _ = overflow_value(options)?;
    Ok(result)
}

fn from_property_bag(value: &Value, options: Option<&Value>) -> Result<Value, VmError> {
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    let day = crate::execute::get_property_result(value, "day")?;
    let day = if matches!(day, Value::Undefined) {
        day
    } else {
        Value::Number(crate::conversion::to_number(&day)?.trunc())
    };
    let month = crate::execute::get_property_result(value, "month")?;
    let month = if matches!(month, Value::Undefined) {
        month
    } else {
        Value::Number(crate::conversion::to_number(&month)?.trunc())
    };
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    let month_code = if matches!(month_code, Value::Undefined) {
        month_code
    } else {
        Value::String(month_code_text(&month_code)?)
    };
    let year = crate::execute::get_property_result(value, "year")?;
    if matches!(day, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing day"));
    }
    if matches!(year, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing year"));
    }
    if matches!(month, Value::Undefined) && matches!(month_code, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing month"));
    }
    let month_code_number = if matches!(month_code, Value::Undefined) {
        None
    } else {
        Some(month_from_code(month_code.clone())?)
    };
    let year = Value::Number(crate::conversion::to_number(&year)?.trunc());
    if let Some(month_number) = &month_code_number {
        if !(1.0..=12.0).contains(&crate::conversion::to_number(month_number)?) {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        if matches!(&month_code, Value::String(value) if value.ends_with('L')) {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
    }
    let calendar = match calendar {
        Value::Undefined => calendar,
        Value::String(_) | Value::StringUnits(_) => {
            let text = crate::conversion::to_string(&calendar)?;
            if !is_iso_calendar_string(&text) {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
            Value::String("iso8601".into())
        }
        _ => return Err(crate::value::error::throw_type_error("Invalid calendar")),
    };
    let overflow = overflow_value(options)?;
    let month = if matches!(month, Value::Undefined) {
        month_code_number
            .clone()
            .ok_or_else(|| crate::value::error::throw_type_error("Missing month"))?
    } else {
        if let Some(month_code) = &month_code_number {
            if crate::conversion::to_number(&month)? != crate::conversion::to_number(month_code)? {
                return Err(crate::value::error::throw_range_error("Month mismatch"));
            }
        }
        month
    };
    let year_number = crate::conversion::to_number(&year)?.trunc();
    let year = Value::Number(year_number);
    let mut day = crate::conversion::to_number(&day)?.trunc();
    let month_number = crate::conversion::to_number(&month)?;
    if !month_number.is_finite() || month_number < 1.0 || !day.is_finite() || day < 1.0 {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    if overflow == "constrain" {
        let month_number = month_number.clamp(1.0, 12.0);
        day = day.clamp(1.0, days_in_month(year_number, month_number));
        return construct(&[
            year,
            Value::Number(month_number),
            Value::Number(day),
            calendar,
        ]);
    }
    construct(&[year, month, Value::Number(day), calendar])
}

fn overflow_value(options: Option<&Value>) -> Result<&'static str, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("constrain");
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "overflow")?;
    if matches!(value, Value::Undefined) {
        return Ok("constrain");
    }
    let value = crate::conversion::to_string(&value)?;
    if matches!(value.as_str(), "constrain" | "reject") {
        Ok(if value == "reject" {
            "reject"
        } else {
            "constrain"
        })
    } else {
        Err(crate::value::error::throw_range_error("Invalid overflow"))
    }
}

fn is_iso_calendar_string(value: &str) -> bool {
    if value.starts_with("-000000") || value.starts_with('\u{2212}') {
        return false;
    }
    if value.eq_ignore_ascii_case("iso8601") {
        return true;
    }
    let (base, annotation) = value
        .split_once('[')
        .map_or((value, None), |(base, annotation)| (base, Some(annotation)));
    if let Some(annotation) = annotation {
        if !annotation
            .strip_suffix(']')
            .is_some_and(|value| value.eq_ignore_ascii_case("u-ca=iso8601"))
        {
            return false;
        }
    }
    let date = base.split(['T', 't', ' ']).next().unwrap_or(base);
    let fields: Vec<_> = date.split('-').collect();
    match fields.as_slice() {
        [year, month, day] => year.len() >= 4 && month.len() == 2 && day.len() == 2,
        [year, month] if year.len() >= 4 => month.len() == 2,
        [month, day] => month.len() == 2 && day.len() == 2,
        _ => false,
    }
}

fn month_from_code(value: Value) -> Result<Value, VmError> {
    if !matches!(value, Value::String(_) | Value::StringUnits(_)) {
        return Err(crate::value::error::throw_type_error("Invalid monthCode"));
    }
    let text = month_code_text(&value)?;
    let text = text.strip_suffix('L').unwrap_or(&text);
    let month = text
        .strip_prefix('M')
        .filter(|value| value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u8>().ok())
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))?;
    Ok(Value::Number(f64::from(month)))
}

fn has_uppercase_annotation_key(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .filter(|annotation| annotation.contains('='))
        .any(|annotation| {
            annotation
                .split('=')
                .next()
                .is_some_and(|key| key.chars().any(|character| character.is_ascii_uppercase()))
        })
}

fn has_unknown_critical_annotation(text: &str) -> bool {
    text.split('[').skip(1).any(|annotation| {
        annotation.starts_with('!') && annotation.contains('=') && !annotation.starts_with("!u-ca=")
    })
}

fn has_invalid_calendar_annotation(text: &str) -> bool {
    let mut seen = false;
    for annotation in text.split('[').skip(1) {
        let Some(value) = ["u-ca=", "!u-ca="]
            .iter()
            .find_map(|prefix| annotation.strip_prefix(prefix))
            .and_then(|value| value.split(']').next())
        else {
            continue;
        };
        if seen {
            continue;
        }
        seen = true;
        return !value.eq_ignore_ascii_case("iso8601");
    }
    false
}

fn has_time_junk(text: &str) -> bool {
    let Some(base) = text.split('[').next() else {
        return false;
    };
    let Some((_, time)) = base.split_once(['T', 't']) else {
        return false;
    };
    time.chars()
        .any(|character| !character.is_ascii_digit() && !":.,+-Zz".contains(character))
}

fn has_annotation_junk(text: &str) -> bool {
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open + 1..].find(']') else {
            return true;
        };
        let after = &rest[open + 1 + close + 1..];
        if !after.is_empty() && !after.starts_with('[') {
            return true;
        }
        rest = after;
    }
    false
}

fn has_multiple_time_zones(text: &str) -> bool {
    text.split('[')
        .skip(1)
        .filter(|annotation| !annotation.contains('=') && !annotation.is_empty())
        .count()
        > 1
}

fn has_excess_fraction(text: &str) -> bool {
    text.split('[')
        .next()
        .and_then(|value| value.find('.').map(|index| &value[index + 1..]))
        .is_some_and(|fraction| fraction.bytes().take_while(u8::is_ascii_digit).count() > 9)
}

fn has_utc_designator(text: &str) -> bool {
    text.split('[')
        .next()
        .is_some_and(|value| value.ends_with('Z'))
}

fn has_empty_time_designator(text: &str) -> bool {
    text.split('[')
        .next()
        .is_some_and(|value| value.ends_with(['T', 't']))
}

fn has_fractional_minutes(text: &str) -> bool {
    let Some(time) = text
        .split('[')
        .next()
        .unwrap_or(text)
        .split(['T', 't', ' '])
        .nth(1)
        .and_then(|value| value.split('[').next())
    else {
        return false;
    };
    let time = time
        .get(1..)
        .and_then(|value| value.find(['+', '-']).map(|index| &time[..index + 1]))
        .unwrap_or(time);
    if !time.contains(':') {
        return false;
    }
    let mut fields = time.split(':');
    let Some(hours) = fields.next() else {
        return false;
    };
    let Some(minutes) = fields.next() else {
        return false;
    };
    hours.contains(['.', ',']) || minutes.contains(['.', ','])
}

fn has_invalid_time(text: &str) -> bool {
    let Some(time) = text
        .split('[')
        .next()
        .unwrap_or(text)
        .split(['T', 't', ' '])
        .nth(1)
        .and_then(|value| value.split('[').next())
    else {
        return false;
    };
    let time = time.trim_end_matches(['Z', 'z']);
    let clock = time
        .get(1..)
        .and_then(|value| value.find(['+', '-']).map(|index| &time[..index + 1]))
        .unwrap_or(time);
    let fields: Vec<_> = clock.split(':').collect();
    let parse = |value: &str| value.parse::<u32>().ok();
    if fields.len() == 1 {
        let has_fraction = fields[0].contains(['.', ',']);
        let compact = fields[0].split(['.', ',']).next().unwrap_or(fields[0]);
        if !matches!(compact.len(), 2 | 4 | 6)
            || !compact.bytes().all(|byte| byte.is_ascii_digit())
            || has_fraction && compact.len() != 6
        {
            return true;
        }
        let hour = compact[0..2].parse::<u32>().unwrap_or(99);
        let minute = if compact.len() >= 4 {
            compact[2..4].parse::<u32>().unwrap_or(99)
        } else {
            0
        };
        let second = if compact.len() == 6 {
            compact[4..6].parse::<u32>().unwrap_or(99)
        } else {
            0
        };
        return hour > 23 || minute > 59 || second > 60;
    }
    if fields.len() > 1 && (fields[0].len() != 2 || fields[1].len() != 2) {
        return true;
    }
    let Some(hour) = parse(fields[0].split(['.', ',']).next().unwrap_or(fields[0])) else {
        return true;
    };
    if hour > 23 {
        return true;
    }
    if fields.len() == 1 {
        return false;
    }
    let Some(minute) = parse(fields[1].split(['.', ',']).next().unwrap_or(fields[1])) else {
        return true;
    };
    if minute > 59 {
        return true;
    }
    fields.get(2).is_some_and(|second| {
        let second = second.split(['.', ',']).next().unwrap_or(second);
        second.len() != 2 || parse(second).is_none_or(|second| second > 60)
    })
}

fn date_part(text: &str) -> &str {
    text.split(['T', 't', ' ', '[']).next().unwrap_or(text)
}

fn parse_date_parts(parts: &[&str]) -> Result<(i32, i32, i32), VmError> {
    let (year, month, day) = match parts {
        [year, month, day]
            if year.len() == 4
                && year.bytes().all(|byte| byte.is_ascii_digit())
                && month.len() == 2
                && month.bytes().all(|byte| byte.is_ascii_digit())
                && day.len() == 2
                && day.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            ((*year).to_owned(), (*month).to_owned(), (*day).to_owned())
        }
        [year, month, day]
            if year.len() == 7
                && year.starts_with('+')
                && year.as_bytes()[1..]
                    .iter()
                    .all(|byte| byte.is_ascii_digit())
                && month.len() == 2
                && month.bytes().all(|byte| byte.is_ascii_digit())
                && day.len() == 2
                && day.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            ((*year).to_owned(), (*month).to_owned(), (*day).to_owned())
        }
        ["", year, month, day]
            if year.len() == 6
                && year.bytes().all(|byte| byte.is_ascii_digit())
                && month.len() == 2
                && month.bytes().all(|byte| byte.is_ascii_digit())
                && day.len() == 2
                && day.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            (format!("-{year}"), (*month).to_owned(), (*day).to_owned())
        }
        _ => return Err(crate::value::error::throw_range_error("Invalid ISO date")),
    };
    Ok((
        year.parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
        month
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
        day.parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?,
    ))
}

fn checked_date_object(year: i32, month: i32, day: i32) -> Result<Value, VmError> {
    let year = f64::from(year);
    let month = f64::from(month);
    let day = f64::from(day);
    if !(-271_821.0..=275_760.0).contains(&year)
        || !(1.0..=12.0).contains(&month)
        || !(1.0..=days_in_month(year, month)).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if (year == -271_821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275_760.0 && (month > 9.0 || month == 9.0 && day > 13.0))
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    Ok(date_object(year, month, day))
}
