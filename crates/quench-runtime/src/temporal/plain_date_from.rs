fn from(value: Option<&Value>) -> Result<Value, VmError> {
    if let Some(value) = value.filter(|value| crate::value::is_object(value)) {
        return from_property_bag(value);
    }
    let Some(Value::String(text)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    parse_date_string(text)
}

fn parse_date_string(text: &str) -> Result<Value, VmError> {
    if has_utc_designator(text) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if has_excess_fraction(text) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    let calendar_count = text.matches("[u-ca=").count();
    if has_uppercase_annotation_key(text) {
        return Err(crate::value::error::throw_range_error(
            "Invalid annotation key",
        ));
    }
    if has_invalid_calendar_annotation(text) {
        return Err(crate::value::error::throw_range_error("Invalid calendar"));
    }
    if has_multiple_time_zones(text) {
        return Err(crate::value::error::throw_range_error(
            "Multiple time zones",
        ));
    }
    if has_unknown_critical_annotation(text) {
        return Err(crate::value::error::throw_range_error(
            "Unknown critical annotation",
        ));
    }
    if text.contains("[!u-ca=") && calendar_count > 0 {
        return Err(crate::value::error::throw_range_error("Multiple calendars"));
    }
    let date = date_part(text);
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
        return checked_date_object(year, month, day);
    }
    if parts.len() == 1 && date.len() == 11 && matches!(date.as_bytes()[0], b'+' | b'-') {
        let year = date[1..7]
            .parse::<i32>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let year = if date.as_bytes()[0] == b'-' { -year } else { year };
        let month = date[7..9]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[9..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        return checked_date_object(year, month, day);
    }
    let (year, month, day) = parse_date_parts(&parts)?;
    checked_date_object(year, month, day)
}

fn from_property_bag(value: &Value) -> Result<Value, VmError> {
    let year = crate::execute::get_property_result(value, "year")?;
    let month = crate::execute::get_property_result(value, "month")?;
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    let day = crate::execute::get_property_result(value, "day")?;
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    let calendar = match calendar {
        Value::Undefined => calendar,
        Value::String(_) | Value::StringUnits(_) => {
            Value::String(crate::conversion::to_string(&calendar)?)
        }
        _ => return Err(crate::value::error::throw_type_error("Invalid calendar")),
    };
    let month = if matches!(month, Value::Undefined) {
        month_from_code(month_code)?
    } else {
        month
    };
    construct(&[year, month, day, calendar])
}

fn month_from_code(value: Value) -> Result<Value, VmError> {
    if !matches!(value, Value::String(_) | Value::StringUnits(_)) {
        return Err(crate::value::error::throw_type_error("Invalid monthCode"));
    }
    let text = crate::conversion::to_string(&value)?;
    let month = text
        .strip_prefix('M')
        .filter(|value| value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|month| (1..=12).contains(month))
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
    text.split('[').skip(1).any(|annotation| {
        ["u-ca=", "!u-ca="]
            .iter()
            .find_map(|prefix| annotation.strip_prefix(prefix))
            .and_then(|value| value.split(']').next())
            .is_some_and(|value| !value.eq_ignore_ascii_case("iso8601"))
    })
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

fn date_part(text: &str) -> &str {
    text.split(['T', 't', ' ', '[']).next().unwrap_or(text)
}

fn parse_date_parts(parts: &[&str]) -> Result<(i32, i32, i32), VmError> {
    let (year, month, day) = match parts {
        [year, month, day] => ((*year).to_owned(), (*month).to_owned(), (*day).to_owned()),
        ["", year, month, day] => (format!("-{year}"), (*month).to_owned(), (*day).to_owned()),
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
