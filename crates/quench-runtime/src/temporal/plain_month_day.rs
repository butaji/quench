use crate::{execute::VmError, value::Value};

pub(crate) fn construct(month: f64, day: f64) -> Result<Value, VmError> {
    construct_with_year(month, day, 1972.0)
}

pub(crate) fn construct_from_arguments(arguments: &[Value]) -> Result<Value, VmError> {
    let month = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if !month.is_finite() {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    let day = crate::conversion::to_number(arguments.get(1).unwrap_or(&Value::Undefined))?;
    if !day.is_finite() {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    let calendar = arguments.get(2).unwrap_or(&Value::Undefined);
    let calendar_id = if matches!(calendar, Value::Undefined) {
        "iso8601".to_string()
    } else {
        let text = crate::conversion::to_string(calendar)?;
        if !crate::temporal::plain_date::is_supported_calendar_name(&text) {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
        crate::temporal::plain_date::canonical_calendar_id(&text)
            .unwrap_or_else(|| "iso8601".into())
    };
    if !matches!(calendar, Value::Undefined) {
        if !matches!(calendar, Value::String(_) | Value::StringUnits(_)) {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
    }
    let default_year = Value::Number(1972.0);
    let year_arg = arguments.get(3).unwrap_or(&default_year);
    let year = if matches!(year_arg, Value::Undefined) {
        1972.0
    } else {
        crate::conversion::to_number(year_arg)?
    };
    if !year.is_finite() || year.fract() != 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    if !matches!(calendar_id.as_str(), "iso8601" | "gregory") {
        if let Some(fields) = crate::temporal::plain_date::calendar_fields_from_iso(
            year as i32,
            month as u32,
            day as u32,
            &calendar_id,
        ) {
            return construct_calendar_month_day(
                &fields.month_code,
                f64::from(fields.day),
                year,
                &calendar_id,
            );
        }
    }
    Ok(set_calendar_id(construct_with_year(month, day, year)?, &calendar_id))
}

fn set_calendar_id(mut value: Value, calendar: &str) -> Value {
    if let Value::Object(object) = &mut value {
        std::rc::Rc::make_mut(object)
            .set_property_in_place("calendarId", Value::String(calendar.to_string()));
    }
    value
}

fn construct_calendar_month_day(
    code: &str,
    day: f64,
    reference_year: f64,
    calendar: &str,
) -> Result<Value, VmError> {
    let month = code
        .trim_start_matches('M')
        .trim_end_matches('L')
        .parse::<f64>()
        .unwrap_or(0.0);
    if !(1.0..=13.0).contains(&month)
        || (month == 13.0 && !crate::temporal::plain_date::calendar_supports_month13(calendar))
        || !day.is_finite()
        || !(1.0..=31.0).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainMonthDay"));
    }
    if month > 12.0 {
        return Ok(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(vec![
                ("monthCode".into(), Value::String(code.to_string())),
                ("day".into(), Value::Number(day.trunc())),
                ("calendarId".into(), Value::String(calendar.to_string())),
                ("referenceISODay".into(), Value::Number(reference_year)),
                ("\0temporal-plain-month-day".into(), Value::Boolean(true)),
                (
                    "\0prototype".into(),
                    Value::Builtin(crate::ops::Builtin::TemporalPlainMonthDayPrototype),
                ),
            ]),
        )));
    }
    let value = construct_with_year(month, day, reference_year)?;
    Ok(set_month_code(set_calendar_id(value, calendar), code))
}

fn set_month_code(mut value: Value, code: &str) -> Value {
    if let Value::Object(object) = &mut value {
        let object = std::rc::Rc::make_mut(object);
        object.set_property_in_place("monthCode", Value::String(code.to_string()));
        object.set_property_in_place(
            "\0temporal-slot:\0monthCode",
            Value::String(code.to_string()),
        );
    }
    value
}

fn construct_with_year(month: f64, day: f64, year: f64) -> Result<Value, VmError> {
    let month = month.trunc();
    let day = day.trunc();
    if !month.is_finite()
        || !day.is_finite()
        || !(1.0..=12.0).contains(&month)
        || !(1.0..=31.0).contains(&day)
        || day > month_day_limit(year, month)
        || (year == 275_760.0 && (month > 9.0 || month == 9.0 && day > 13.0))
        || (year == -271_821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            (
                "monthCode".into(),
                Value::String(format!("M{:02}", month as u32)),
            ),
            ("day".into(), Value::Number(day)),
            ("calendarId".into(), Value::String("iso8601".into())),
            ("referenceISODay".into(), Value::Number(year)),
            ("\0temporal-plain-month-day".into(), Value::Boolean(true)),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainMonthDayPrototype),
            ),
        ]),
    )))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    Some(match builtin {
        crate::ops::Builtin::TemporalPlainMonthDay => Err(crate::value::error::throw_type_error(
            "Temporal.PlainMonthDay requires new",
        )),
        crate::ops::Builtin::TemporalPlainMonthDayFrom => from(arguments.first(), arguments.get(1)),
        crate::ops::Builtin::TemporalPlainMonthDayCompare => compare(arguments),
        crate::ops::Builtin::TemporalPlainMonthDayCalendarIdGetter => field(receiver, "calendarId"),
        crate::ops::Builtin::TemporalPlainMonthDayDayGetter => field(receiver, "day"),
        crate::ops::Builtin::TemporalPlainMonthDayMonthCodeGetter => field(receiver, "monthCode"),
        crate::ops::Builtin::TemporalPlainMonthDayEquals => equals(receiver, arguments.first()),
        crate::ops::Builtin::TemporalPlainMonthDayToString
        => {
            to_string(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainMonthDayToLocaleString => {
            to_locale_string(receiver, arguments)
        }
        crate::ops::Builtin::TemporalPlainMonthDayToJSON => to_string(receiver, None),
        crate::ops::Builtin::TemporalPlainMonthDayToPlainDate => {
            to_plain_date(receiver, arguments.first(), arguments.get(1))
        }
        crate::ops::Builtin::TemporalPlainMonthDayWith => {
            with(receiver, arguments.first(), arguments.get(1))
        }
        crate::ops::Builtin::TemporalPlainMonthDayValueOf => Err(
            crate::value::error::throw_type_error("Cannot convert PlainMonthDay to a number"),
        ),
        _ => return None,
    })
}

fn to_locale_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let value = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainMonthDay"))?
        .clone();
    crate::intl::datetime::format_temporal_value(&value, arguments, &["month", "day"])
}

fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let value =
        value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainMonthDay"))?;
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        let text = crate::conversion::to_string(value)?;
        let calendar_id = text
            .split_once("[u-ca=")
            .and_then(|(_, rest)| rest.split(']').next())
            .and_then(crate::temporal::plain_date::canonical_calendar_id)
            .unwrap_or_else(|| "iso8601".into());
        if text.contains(['\u{2212}', 'Z', 'z']) || text.starts_with("-000000") {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainMonthDay",
            ));
        }
        if let Some(index) = text.find(['.', ',']) {
            let fraction = text[index + 1..]
                .chars()
                .take_while(char::is_ascii_digit)
                .count();
            if fraction > 9 {
                return Err(crate::value::error::throw_range_error(
                    "Too many fractional second digits",
                ));
            }
        }
        let annotations = text
            .match_indices('[')
            .filter_map(|(start, _)| text[start + 1..].split(']').next())
            .collect::<Vec<_>>();
        let mut calendars = 0usize;
        let mut critical_calendar = false;
        let mut time_zones = 0usize;
        for annotation in annotations {
            let critical = annotation.starts_with('!');
            let annotation = annotation.strip_prefix('!').unwrap_or(annotation);
            if annotation.starts_with("u-ca=") {
                calendars += 1;
                critical_calendar |= critical;
                if calendars == 1
                    && !crate::temporal::plain_date::is_supported_calendar_name(&annotation[5..])
                {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid calendar annotation",
                    ));
                }
            } else if annotation.contains('=') {
                if critical {
                    return Err(crate::value::error::throw_range_error("Invalid annotation"));
                }
            } else {
                time_zones += 1;
            }
        }
        if (calendars > 1 && critical_calendar) || time_zones > 1 {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        }
        if text.split('[').skip(1).any(|part| {
            part.split(']')
                .next()
                .and_then(|annotation| annotation.split_once('=').map(|(key, _)| key))
                .is_some_and(|key| key.chars().any(|character| character.is_ascii_uppercase()))
        }) {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        }
        let base = text.split('[').next().unwrap_or(&text);
        let date = if let Some((date, time)) = base.split_once(['T', 't', ' ']) {
            let clock = time.find(['+', '-']).map_or(time, |offset| &time[..offset]);
            let parts = clock.split(':').collect::<Vec<_>>();
            if let Some(index) = clock.find(['.', ',']) {
                let fraction = clock[index + 1..].split(['+', '-']).next().unwrap_or("");
                if fraction.len() > 9 {
                    return Err(crate::value::error::throw_range_error(
                        "Too many fractional second digits",
                    ));
                }
            }
            if parts.len() == 1
                && parts.get(0).is_some_and(|part| part.contains(['.', ',']))
                && parts[0]
                    .split_once(['.', ','])
                    .is_some_and(|(whole, _)| whole.len() <= 2)
                || parts.get(1).is_some_and(|part| part.contains(['.', ',']))
            {
                return Err(crate::value::error::throw_range_error(
                    "Fractional minutes or hours are not allowed",
                ));
            }
            if parts.get(2).is_some_and(|part| {
                part.split_once(['.', ','])
                    .is_some_and(|(_, fraction)| fraction.len() > 9)
            }) {
                return Err(crate::value::error::throw_range_error(
                    "Too many fractional second digits",
                ));
            }
            date
        } else {
            base
        };
        let input_iso_year = date
            .strip_prefix("--")
            .unwrap_or(date)
            .split('-')
            .next()
            .filter(|value| value.len() >= 4)
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(1972);
        let date = date.strip_prefix("--").unwrap_or(date);
        let (month, day) = if !date.contains('-') {
            let digits = date.strip_prefix('+').unwrap_or(date);
            match digits.len() {
                4 => (&digits[..2], &digits[2..]),
                length if length >= 8 => (&digits[length - 4..length - 2], &digits[length - 2..]),
                _ => {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid PlainMonthDay",
                    ))
                }
            }
        } else {
            let parts = date.split('-').collect::<Vec<_>>();
            match parts.as_slice() {
                [month, day] => (*month, *day),
                [.., month, day] => (*month, *day),
                _ => {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid PlainMonthDay",
                    ))
                }
            }
        };
        if month.is_empty() || day.is_empty() {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainMonthDay",
            ));
        }
        let month = month.parse::<f64>().unwrap_or(0.0);
        let day = day.parse::<f64>().unwrap_or(0.0);
        if !month.is_finite()
            || !day.is_finite()
            || !(1.0..=12.0).contains(&month)
            || !(1.0..=31.0).contains(&day)
        {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainMonthDay",
            ));
        }
        let _ = overflow_reject(options)?;
        if !matches!(calendar_id.as_str(), "iso8601" | "gregory") {
            if let Some(fields) = crate::temporal::plain_date::calendar_fields_from_iso(
                input_iso_year,
                month as u32,
                day as u32,
                &calendar_id,
            ) {
                return construct_calendar_month_day(
                    &fields.month_code,
                    f64::from(fields.day),
                    f64::from(
                        crate::temporal::plain_date::calendar_reference_iso_year_for_code(
                            &fields.month_code,
                            fields.day,
                            &calendar_id,
                        )
                        .unwrap_or(1972),
                    ),
                    &calendar_id,
                );
            }
        }
        return Ok(set_calendar_id(construct(month, day)?, &calendar_id));
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainMonthDay",
        ));
    }
    if matches!(value, Value::Builtin(_)) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainMonthDay",
        ));
    }
    if is_plain_month_day(value) {
        let reject = overflow_reject(options)?;
        let (month_code, day) = fields(value)?;
        let month = month_code.trim_start_matches('M').parse().unwrap_or(0.0);
        let year = reference_year(value);
        let day = apply_overflow(year, month, day, reject)?;
        let calendar = crate::execute::get_property_result(value, "calendarId")
            .ok()
            .and_then(|value| crate::conversion::to_string(&value).ok())
            .unwrap_or_else(|| "iso8601".into());
        return Ok(set_calendar_id(construct_with_year(month, day, year)?, &calendar));
    }
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    validate_calendar(&calendar)?;
    let calendar_id = match &calendar {
        Value::String(text) => crate::temporal::plain_date::canonical_calendar_id(text)
            .unwrap_or_else(|| "iso8601".into()),
        Value::StringUnits(_) => crate::temporal::plain_date::canonical_calendar_id(
            &crate::conversion::to_string(&calendar)?,
        )
        .unwrap_or_else(|| "iso8601".into()),
        _ => "iso8601".into(),
    };
    let day_value = crate::execute::get_property_result(value, "day")?;
    if matches!(day_value, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing day"));
    }
    let day = crate::conversion::to_number(&day_value)?;
    let month_value = crate::execute::get_property_result(value, "month")?;
    let month_number = if matches!(month_value, Value::Undefined) {
        None
    } else {
        Some(crate::conversion::to_number(&month_value)?)
    };
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    let month_code_text = if matches!(month_code, Value::Undefined) {
        None
    } else {
        let text = crate::conversion::to_string(&month_code)?;
        if !month_code_syntax_valid(&text) {
            return Err(crate::value::error::throw_range_error("Invalid monthCode"));
        }
        Some(text)
    };
    if matches!(month_value, Value::Undefined) && matches!(month_code, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing month"));
    }
    let year_value = crate::execute::get_property_result(value, "year")?;
    let year_number = crate::conversion::to_number(&year_value)?;
    if !matches!(year_value, Value::Undefined) && !year_number.is_finite() {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    let year = if year_number.is_finite() {
        year_number.trunc()
    } else {
        1972.0
    };
    let reject = overflow_reject(options)?;
    let month = if let Some(month_code) = month_code_text.as_deref() {
        let parsed = parse_month_code(month_code)?;
        if let Some(month_number) = month_number {
            if month_number.trunc() != parsed {
                return Err(crate::value::error::throw_range_error(
                    "Conflicting month fields",
                ));
            }
        }
        parsed
    } else {
        month_number.unwrap_or(0.0)
    };
    if !month.is_finite() || !day.is_finite() || month <= 0.0 || day <= 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    let month = if reject {
        month
    } else {
        month.clamp(1.0, 12.0)
    };
    let day = apply_overflow(year, month, day, reject)?;
    Ok(set_calendar_id(construct(month, day)?, &calendar_id))
}

fn parse_month_code(value: &str) -> Result<f64, VmError> {
    let bytes = value.as_bytes();
    if bytes.len() < 3
        || bytes[0] != b'M'
        || !bytes[1].is_ascii_digit()
        || !bytes[2].is_ascii_digit()
    {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    }
    let month = value[1..3].parse::<f64>().unwrap_or(0.0);
    if bytes.len() != 3 || !(1.0..=12.0).contains(&month) {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    }
    Ok(month)
}

fn month_code_syntax_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0] == b'M'
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
        && (bytes.len() == 3 || bytes.len() == 4 && bytes[3] == b'L')
}

fn validate_calendar(value: &Value) -> Result<(), VmError> {
    if matches!(value, Value::Undefined) {
        return Ok(());
    }
    if matches!(value, Value::Object(object) if object.iter().any(|(key, value)| key == "\0prototype" && matches!(value, Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype | crate::ops::Builtin::TemporalPlainDateTimePrototype | crate::ops::Builtin::TemporalPlainMonthDayPrototype | crate::ops::Builtin::TemporalPlainYearMonthPrototype | crate::ops::Builtin::TemporalZonedDateTimePrototype))))
    {
        return Ok(());
    }
    if let Ok(calendar_id) = crate::execute::get_property_result(value, "calendarId") {
        if matches!(calendar_id, Value::String(ref id) if crate::temporal::plain_date::is_supported_calendar_name(id)) {
            return Ok(());
        }
    }
    crate::temporal::parse_calendar_identifier(value).map(|_| ())
}

fn overflow_reject(options: Option<&Value>) -> Result<bool, VmError> {
    crate::temporal::options::reject_overflow(options)
}

fn fields(value: &Value) -> Result<(String, f64), VmError> {
    if !is_plain_month_day(value) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainMonthDay receiver",
        ));
    }
    let month = crate::execute::get_property_result(value, "monthCode")?;
    let day = crate::conversion::to_number(&crate::execute::get_property_result(value, "day")?)?;
    Ok((crate::conversion::to_string(&month)?, day))
}

fn field(receiver: Option<&Value>, name: &str) -> Result<Value, VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainMonthDay receiver"))?;
    if !is_plain_month_day(receiver) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainMonthDay receiver",
        ));
    }
    crate::execute::get_property_result(receiver, name)
}

fn is_plain_month_day(value: &Value) -> bool {
    matches!(value, Value::Object(object) if object.iter().any(|(key, value)| {
        key == "\0temporal-plain-month-day" && matches!(value, Value::Boolean(true))
    }))
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = fields(&from(arguments.first(), None)?)?;
    let right = fields(&from(arguments.get(1), None)?)?;
    Ok(Value::Number(
        match (left.0.cmp(&right.0), left.1.partial_cmp(&right.1)) {
            (std::cmp::Ordering::Less, _) | (_, Some(std::cmp::Ordering::Less)) => -1.0,
            (std::cmp::Ordering::Greater, _) | (_, Some(std::cmp::Ordering::Greater)) => 1.0,
            _ => 0.0,
        },
    ))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let other = from(other, None)?;
    let receiver_calendar = crate::execute::get_property_result(receiver, "calendarId")
        .ok()
        .and_then(|value| crate::conversion::to_string(&value).ok())
        .unwrap_or_else(|| "iso8601".into());
    let other_calendar = crate::execute::get_property_result(&other, "calendarId")
        .ok()
        .and_then(|value| crate::conversion::to_string(&value).ok())
        .unwrap_or_else(|| "iso8601".into());
    let receiver_calendar = crate::temporal::plain_date::canonical_calendar_id(&receiver_calendar)
        .unwrap_or(receiver_calendar);
    let other_calendar = crate::temporal::plain_date::canonical_calendar_id(&other_calendar)
        .unwrap_or(other_calendar);
    Ok(Value::Boolean(
        fields(receiver)? == fields(&other)?
            && reference_year(receiver) == reference_year(&other)
            && receiver_calendar == other_calendar,
    ))
}

fn reference_year(value: &Value) -> f64 {
    crate::execute::get_property_result(value, "referenceISODay")
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .filter(|year| year.is_finite())
        .unwrap_or(1972.0)
}

fn month_day_limit(year: f64, month: f64) -> f64 {
    if month == 2.0 {
        let leap = year.rem_euclid(4.0) == 0.0
            && (year.rem_euclid(100.0) != 0.0 || year.rem_euclid(400.0) == 0.0);
        if leap {
            29.0
        } else {
            28.0
        }
    } else if matches!(month as u32, 4 | 6 | 9 | 11) {
        30.0
    } else {
        31.0
    }
}

fn apply_overflow(year: f64, month: f64, day: f64, reject: bool) -> Result<f64, VmError> {
    if !month.is_finite() || !day.is_finite() || month < 1.0 || month > 12.0 || day < 1.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    let limit = month_day_limit(year, month);
    if reject && day > limit {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    Ok(day.min(limit).trunc())
}

fn to_string(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (month, day) = fields(receiver)?;
    let name = calendar_name(options)?;
    let calendar = crate::execute::get_property_result(receiver, "calendarId")
        .ok()
        .and_then(|value| crate::conversion::to_string(&value).ok())
        .unwrap_or_else(|| "iso8601".into());
    let month_code = month.clone();
    let month = month.trim_start_matches('M');
    let year = crate::execute::get_property_result(receiver, "referenceISODay")
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(1972.0) as i32;
    if calendar == "iso8601" && matches!(name.as_str(), "auto" | "never") {
        return Ok(Value::String(format!("{month}-{day:02}")));
    }
    let year_text = if year < 0 {
        format!("-{0:06}", year.unsigned_abs())
    } else if year > 9999 {
        format!("+{year:06}")
    } else {
        format!("{year:04}")
    };
    let (display_month, display_day) = if !matches!(calendar.as_str(), "iso8601" | "gregory") {
        crate::temporal::plain_date::calendar_iso_date_for_code(
            year,
            &month_code,
            day as u32,
            &calendar,
        )
        .map_or((month.parse::<u32>().unwrap_or(1), day as u32), |(month, day)| {
            (month, day)
        })
    } else {
        (month.parse::<u32>().unwrap_or(1), day as u32)
    };
    let date = format!("{year_text}-{display_month:02}-{display_day:02}");
    let suffix = match name.as_str() {
        "never" => "".to_string(),
        "critical" => format!("[!u-ca={calendar}]"),
        _ => format!("[u-ca={calendar}]"),
    };
    Ok(Value::String(format!("{date}{suffix}")))
}

fn calendar_name(options: Option<&Value>) -> Result<String, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("auto".into());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "calendarName")?;
    if matches!(value, Value::Undefined) {
        return Ok("auto".into());
    }
    let value = crate::conversion::to_string(&value)?;
    if matches!(value.as_str(), "auto" | "always" | "never" | "critical") {
        Ok(value)
    } else {
        Err(crate::value::error::throw_range_error(
            "Invalid calendarName",
        ))
    }
}

fn to_plain_date(
    receiver: Option<&Value>,
    year_fields: Option<&Value>,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let date_fields = year_fields
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid fields"))?;
    let year_value = crate::execute::get_property_result(date_fields, "year")?;
    if matches!(year_value, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing year"));
    }
    let year = crate::conversion::to_number(&year_value)?;
    let (month, day) = fields(receiver)?;
    let month = month.trim_start_matches('M').parse().unwrap_or(0.0);
    let mut day = day;
    let boundary = (year == -271_821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275_760.0 && (month > 9.0 || month == 9.0 && day > 13.0));
    while !boundary
        && crate::temporal::plain_date::construct(&[
            Value::Number(year),
            Value::Number(month),
            Value::Number(day),
        ])
        .is_err()
        && day > 1.0
    {
        day -= 1.0;
    }
    let _ = options;
    crate::temporal::plain_date::construct(&[
        Value::Number(year),
        Value::Number(month),
        Value::Number(day),
    ])
}

fn with(
    receiver: Option<&Value>,
    changes: Option<&Value>,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let changes = changes
        .filter(|v| crate::value::is_object(v))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid fields"))?;
    let calendar = crate::execute::get_property_result(changes, "calendar")?;
    let time_zone = crate::execute::get_property_result(changes, "timeZone")?;
    if !matches!(calendar, Value::Undefined) || !matches!(time_zone, Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "PlainMonthDay fields cannot include calendar or timeZone",
        ));
    }
    if matches!(changes, Value::Object(object) if object.iter().any(|(key, value)| key == "\0prototype" && matches!(value, Value::Builtin(_))))
    {
        return Err(crate::value::error::throw_type_error("Invalid fields"));
    }
    let (original_month, original_day) = fields(receiver)?;
    let day_value = crate::execute::get_property_result(changes, "day")?;
    let day = match &day_value {
        Value::Undefined => original_day,
        value => crate::conversion::to_number(&value)?,
    };
    if !day.is_finite() || day < 1.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainMonthDay",
        ));
    }
    let month_value = crate::execute::get_property_result(changes, "month")?;
    let month_number = if matches!(month_value, Value::Undefined) {
        None
    } else {
        Some(crate::conversion::to_number(&month_value)?)
    };
    let month_code = crate::execute::get_property_result(changes, "monthCode")?;
    let month_code = if matches!(month_code, Value::Undefined) {
        None
    } else {
        Some(crate::conversion::to_string(&month_code)?)
    };
    let year_value = crate::execute::get_property_result(changes, "year")?;
    let year = if matches!(year_value, Value::Undefined) {
        reference_year(receiver)
    } else {
        crate::conversion::to_number(&year_value)?
    };
    let month = month_code
        .as_deref()
        .map(|code| code.trim_start_matches('M').parse().unwrap_or(0.0))
        .or(month_number)
        .unwrap_or_else(|| {
            original_month
                .trim_start_matches('M')
                .parse()
                .unwrap_or(0.0)
        });
    if month_code.is_none()
        && month_number.is_none()
        && matches!(year_value, Value::Undefined)
        && matches!(day_value, Value::Undefined)
    {
        return Err(crate::value::error::throw_type_error("Invalid fields"));
    }
    let reject = overflow_reject(options)?;
    let month = if reject {
        month
    } else {
        month.clamp(1.0, 12.0)
    };
    let parsed_code = month_code.as_deref().map(parse_month_code).transpose()?;
    if let (Some(month_number), Some(code_month)) = (month_number, parsed_code) {
        if month_number.trunc() != code_month {
            return Err(crate::value::error::throw_range_error(
                "Conflicting month fields",
            ));
        }
    }
    let month = parsed_code.unwrap_or(month);
    let day = apply_overflow(year, month, day, reject)?;
    construct_with_year(month, day, reference_year(receiver))
}
