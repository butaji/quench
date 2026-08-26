use crate::{execute::VmError, value::Value};

pub(crate) fn construct(year: f64, month: f64) -> Result<Value, VmError> {
    construct_inner(year, month, None)
}

pub(crate) fn construct_with_reference(
    year: f64,
    month: f64,
    reference_iso_day: f64,
) -> Result<Value, VmError> {
    construct_inner(year, month, Some(reference_iso_day))
}

fn construct_inner(
    year: f64,
    month: f64,
    reference_iso_day: Option<f64>,
) -> Result<Value, VmError> {
    if !year.is_finite()
        || !(-271_821.0..=275_760.0).contains(&year)
        || !(1.0..=12.0).contains(&month)
        || (year == -271_821.0 && month < 4.0)
        || (year == 275_760.0 && month > 9.0)
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    }
    if let Some(day) = reference_iso_day {
        if !day.is_finite() || !(1.0..=31.0).contains(&day) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
        if day > iso_days_in_month(year, month)
            || (year == -271_821.0 && month == 4.0 && day < 19.0)
            || (year == 275_760.0 && month == 9.0 && day > 13.0)
        {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
    }
    let reference_iso_day = reference_iso_day.unwrap_or(1.0);
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("year".into(), Value::Number(year)),
            ("month".into(), Value::Number(month)),
            (
                "monthCode".into(),
                Value::String(format!("M{:02}", month as u32)),
            ),
            ("calendarId".into(), Value::String("iso8601".into())),
            ("referenceISODay".into(), Value::Number(reference_iso_day)),
            ("\0temporal-plain-year-month".into(), Value::Boolean(true)),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainYearMonthPrototype),
            ),
        ]),
    )))
}

fn iso_days_in_month(year: f64, month: f64) -> f64 {
    match month as u32 {
        2 if year.rem_euclid(4.0) == 0.0
            && (year.rem_euclid(100.0) != 0.0 || year.rem_euclid(400.0) == 0.0) =>
        {
            29.0
        }
        2 => 28.0,
        4 | 6 | 9 | 11 => 30.0,
        _ => 31.0,
    }
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    Some(match builtin {
        crate::ops::Builtin::TemporalPlainYearMonthFrom => {
            from(arguments.first(), arguments.get(1))
        }
        crate::ops::Builtin::TemporalPlainYearMonthCompare => compare(arguments),
        crate::ops::Builtin::TemporalPlainYearMonthCalendarIdGetter => {
            field(receiver, "calendarId")
        }
        crate::ops::Builtin::TemporalPlainYearMonthYearGetter => field(receiver, "year"),
        crate::ops::Builtin::TemporalPlainYearMonthMonthGetter => field(receiver, "month"),
        crate::ops::Builtin::TemporalPlainYearMonthMonthCodeGetter => field(receiver, "monthCode"),
        crate::ops::Builtin::TemporalPlainYearMonthEquals => equals(receiver, arguments.first()),
        crate::ops::Builtin::TemporalPlainYearMonthToString
        | crate::ops::Builtin::TemporalPlainYearMonthToLocaleString => {
            to_string(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainYearMonthToJSON => to_string(receiver, None),
        crate::ops::Builtin::TemporalPlainYearMonthToPlainDate => {
            to_plain_date(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainYearMonthWith => with(receiver, arguments.first()),
        crate::ops::Builtin::TemporalPlainYearMonthAdd => add(receiver, arguments.first(), 1.0),
        crate::ops::Builtin::TemporalPlainYearMonthSubtract => {
            add(receiver, arguments.first(), -1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthUntil => {
            difference(receiver, arguments.first(), arguments.get(1), 1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthSince => {
            difference(receiver, arguments.first(), arguments.get(1), -1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthDaysInMonthGetter => days_in_month(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthDaysInYearGetter => days_in_year(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthInLeapYearGetter => in_leap_year(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthMonthsInYearGetter => {
            ensure_receiver(receiver).map(|_| Value::Number(12.0))
        }
        crate::ops::Builtin::TemporalPlainYearMonthEraGetter
        | crate::ops::Builtin::TemporalPlainYearMonthEraYearGetter => {
            ensure_receiver(receiver).map(|_| Value::Undefined)
        }
        crate::ops::Builtin::TemporalPlainYearMonthValueOf => Err(
            crate::value::error::throw_type_error("Cannot convert PlainYearMonth to a number"),
        ),
        _ => return None,
    })
}

fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let value =
        value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth"))?;
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        let text = crate::conversion::to_string(value)?;
        if text.contains(['\u{2212}', 'Z', 'z']) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
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
        let mut calendars = 0;
        let mut time_zones = 0;
        for annotation in text
            .match_indices('[')
            .filter_map(|(start, _)| text[start + 1..].split(']').next())
        {
            let critical = annotation.starts_with('!');
            let annotation = annotation.strip_prefix('!').unwrap_or(annotation);
            if let Some((key, _)) = annotation.split_once('=') {
                if key.chars().any(|character| character.is_ascii_uppercase()) {
                    return Err(crate::value::error::throw_range_error("Invalid annotation"));
                }
            }
            if annotation.starts_with("u-ca=") {
                calendars += 1;
            }
            if annotation.starts_with("u-ca=")
                && calendars == 1
                && !annotation[5..].eq_ignore_ascii_case("iso8601")
            {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
            if critical && annotation.contains('=') && !annotation.starts_with("u-ca=") {
                return Err(crate::value::error::throw_range_error("Invalid annotation"));
            }
            if !annotation.starts_with("u-ca=") && !annotation.contains('=') {
                time_zones += 1;
            }
        }
        if (calendars > 1 && text.contains("[!u-ca=")) || time_zones > 1 {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        }
        let base = text.split('[').next().unwrap_or(&text);
        if base.len() == 9 && base.starts_with('+') {
            let year = base[0..7].parse().unwrap_or(0.0);
            let month = base[7..9].parse().unwrap_or(0.0);
            let _ = overflow_option(options)?;
            return construct(year, month);
        }
        let date = if let Some((date, time)) = base.split_once(['T', 't', ' ']) {
            if time.contains('Z') || time.contains('z') {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainYearMonth",
                ));
            }
            date
        } else {
            base
        };
        if !base.contains(['T', 't', ' ']) {
            let date_tail = date.rsplit('-').next().unwrap_or(date);
            if date_tail.contains('+') || date_tail.contains(':') {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainYearMonth",
                ));
            }
        }
        let parts = date.split('-').collect::<Vec<_>>();
        let (year_text, month_text, day_text) = match parts.as_slice() {
            [year, month] => (*year, *month, None),
            [compact] if compact.len() == 6 => (&compact[..4], &compact[4..], None),
            [compact] if compact.len() == 8 => (&compact[..4], &compact[4..6], None),
            [compact] if compact.len() == 9 && compact.starts_with('+') => {
                (&compact[..7], &compact[7..9], None)
            }
            [compact] if compact.len() == 11 && compact.starts_with('+') => {
                (&compact[..7], &compact[7..9], Some(&compact[9..11]))
            }
            [year, month, day] if year.len() >= 4 => (*year, *month, Some(*day)),
            ["", year, month] => (&date[..1 + year.len()], *month, None),
            ["", year, month, day] => (&date[..1 + year.len()], *month, Some(*day)),
            _ => {
                return Err(crate::value::error::throw_range_error(
                    "Invalid PlainYearMonth",
                ))
            }
        };
        let year = year_text.parse().unwrap_or(0.0);
        let month = month_text.parse().unwrap_or(0.0);
        if (year == -271_821.0 && month < 4.0) || (year == 275_760.0 && month > 9.0) {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
        let _ = overflow_option(options)?;
        return construct(year, month);
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth",
        ));
    }
    if let Value::Builtin(_) = value {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth",
        ));
    }
    if is_plain_year_month(value) {
        let _ = overflow_option(options)?;
        let year = crate::conversion::to_number(&field(Some(value), "year")?)?;
        let month = crate::conversion::to_number(&field(Some(value), "month")?)?;
        let day = crate::conversion::to_number(&field(Some(value), "referenceISODay")?)?;
        return construct_with_reference(year, month, day);
    }
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    if !matches!(calendar, Value::Undefined) {
        if !matches!(calendar, Value::String(_) | Value::StringUnits(_)) {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
        let calendar = crate::conversion::to_string(&calendar)?;
        if !calendar.eq_ignore_ascii_case("iso8601") {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
    }
    let year_value = crate::execute::get_property_result(value, "year")?;
    if matches!(year_value, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing year"));
    }
    let year = crate::conversion::to_number(&year_value)?;
    let month_value = crate::execute::get_property_result(value, "month")?;
    let month_code_value = crate::execute::get_property_result(value, "monthCode")?;
    if matches!(month_value, Value::Undefined) && matches!(month_code_value, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Missing month"));
    }
    let month_code = if matches!(month_code_value, Value::Undefined) {
        None
    } else {
        let text = crate::conversion::to_string(&month_code_value)?;
        Some(parse_month_code(&text)?)
    };
    let month_number = if matches!(month_value, Value::Undefined) {
        None
    } else {
        Some(crate::conversion::to_number(&month_value)?)
    };
    if let (Some(month), Some(code)) = (month_number, month_code) {
        if month.trunc() != code {
            return Err(crate::value::error::throw_range_error(
                "Conflicting month fields",
            ));
        }
    }
    let month = month_number.or(month_code).unwrap_or(0.0);
    let constrain = overflow_option(options)?;
    let month = if month <= 0.0 || !month.is_finite() {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    } else if constrain {
        month.min(12.0)
    } else {
        month
    };
    construct(year, month)
}

fn parse_month_code(value: &str) -> Result<f64, VmError> {
    let bytes = value.as_bytes();
    if bytes.len() != 3
        || bytes[0] != b'M'
        || !bytes[1].is_ascii_digit()
        || !bytes[2].is_ascii_digit()
    {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    }
    let month = value[1..3].parse::<f64>().unwrap_or(0.0);
    if !(1.0..=12.0).contains(&month) {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    }
    Ok(month)
}

fn overflow_option(options: Option<&Value>) -> Result<bool, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(true);
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "overflow")?;
    if matches!(value, Value::Undefined) {
        return Ok(true);
    }
    match crate::conversion::to_string(&value)?.as_str() {
        "constrain" => Ok(true),
        "reject" => Ok(false),
        _ => Err(crate::value::error::throw_range_error("Invalid overflow")),
    }
}

fn field(receiver: Option<&Value>, name: &str) -> Result<Value, VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth receiver"))?;
    if !is_plain_year_month(receiver) {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth receiver",
        ));
    }
    crate::execute::get_property_result(receiver, name)
}

fn is_plain_year_month(value: &Value) -> bool {
    matches!(value, Value::Object(object) if object.iter().any(|(key, value)| {
        key == "\0temporal-plain-year-month" && matches!(value, Value::Boolean(true))
    }))
}

fn ensure_receiver(receiver: Option<&Value>) -> Result<(), VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth receiver"))?;
    if is_plain_year_month(receiver) {
        Ok(())
    } else {
        Err(crate::value::error::throw_type_error(
            "Invalid PlainYearMonth receiver",
        ))
    }
}

fn values(value: &Value) -> Result<(f64, f64), VmError> {
    Ok((
        crate::conversion::to_number(&field(Some(value), "year")?)?,
        crate::conversion::to_number(&field(Some(value), "month")?)?,
    ))
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left_value = from(arguments.first(), None)?;
    let right_value = from(arguments.get(1), None)?;
    let left = (values(&left_value)?, reference_day(&left_value));
    let right = (values(&right_value)?, reference_day(&right_value));
    let left = (left.0 .0, left.0 .1, left.1);
    let right = (right.0 .0, right.0 .1, right.1);
    Ok(Value::Number(match left.partial_cmp(&right) {
        Some(std::cmp::Ordering::Less) => -1.0,
        Some(std::cmp::Ordering::Greater) => 1.0,
        _ => 0.0,
    }))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let other = from(other, None)?;
    Ok(Value::Boolean(
        values(receiver)? == values(&other)? && reference_day(receiver) == reference_day(&other),
    ))
}

fn reference_day(value: &Value) -> f64 {
    crate::execute::get_property_result(value, "referenceISODay")
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(1.0)
}

fn to_string(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let (year, month) =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    let name = calendar_name(options)?;
    let year_text = if year < 0.0 {
        format!("-{0:06}", (-(year as i32)).unsigned_abs())
    } else if year > 9999.0 {
        format!("+{year:06}")
    } else {
        format!("{year:04}")
    };
    if matches!(name.as_str(), "always" | "critical") {
        let marker = if name == "critical" {
            "[!u-ca=iso8601]"
        } else {
            "[u-ca=iso8601]"
        };
        let day = reference_day(receiver.ok_or_else(|| {
            crate::value::error::throw_type_error("Invalid PlainYearMonth receiver")
        })?);
        return Ok(Value::String(format!(
            "{year_text}-{month:02}-{day:02}{marker}"
        )));
    }
    Ok(Value::String(format!("{year_text}-{month:02}")))
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

fn to_plain_date(receiver: Option<&Value>, day: Option<&Value>) -> Result<Value, VmError> {
    let (year, month) =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    let day = crate::conversion::to_number(
        day.ok_or_else(|| crate::value::error::throw_type_error("Missing day"))?,
    )?;
    crate::temporal::plain_date::construct(&[
        Value::Number(year),
        Value::Number(month),
        Value::Number(day),
    ])
}

fn with(receiver: Option<&Value>, changes: Option<&Value>) -> Result<Value, VmError> {
    let (year, month) =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    let changes = changes
        .filter(|v| crate::value::is_object(v))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid fields"))?;
    let year = match crate::execute::get_property_result(changes, "year")? {
        Value::Undefined => year,
        value => crate::conversion::to_number(&value)?,
    };
    let month = match crate::execute::get_property_result(changes, "month")? {
        Value::Undefined => month,
        value => crate::conversion::to_number(&value)?,
    };
    construct(year, month)
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let (year, month) =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    let duration = crate::temporal::duration::from(duration)?;
    let months = crate::execute::get_property_result(&duration, "years")
        .ok()
        .and_then(|v| crate::conversion::to_number(&v).ok())
        .unwrap_or(0.0)
        * 12.0
        + crate::execute::get_property_result(&duration, "months")
            .ok()
            .and_then(|v| crate::conversion::to_number(&v).ok())
            .unwrap_or(0.0);
    let total = year * 12.0 + month - 1.0 + months * direction;
    construct((total / 12.0).floor(), total.rem_euclid(12.0) + 1.0)
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    options: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let left =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    let right = values(&from(other, None)?)?;
    let largest = difference_largest(options)?;
    let total = ((right.0 - left.0) * 12.0 + right.1 - left.1) * direction;
    let (years, months) = if largest == "month" {
        (0.0, total)
    } else {
        let years = (total / 12.0).trunc();
        (years, total - years * 12.0)
    };
    crate::temporal::duration::construct(&[Value::Number(years), Value::Number(months)])
}

fn difference_largest(options: Option<&Value>) -> Result<&'static str, VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok("year");
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let value = crate::execute::get_property_result(options, "largestUnit")?;
    if matches!(value, Value::Undefined) {
        return Ok("year");
    }
    let value = crate::conversion::to_string(&value)?;
    match value.trim_end_matches('s') {
        "year" => Ok("year"),
        "month" => Ok("month"),
        _ => Err(crate::value::error::throw_range_error(
            "Invalid largestUnit",
        )),
    }
}

fn days_in_month(receiver: Option<&Value>) -> Result<Value, VmError> {
    let (year, month) =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, 1)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
    let next = if month == 12.0 {
        chrono::NaiveDate::from_ymd_opt(year as i32 + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(year as i32, month as u32 + 1, 1)
    }
    .ok_or_else(|| crate::value::error::throw_range_error("Invalid date"))?;
    Ok(Value::Number((next - date).num_days() as f64))
}

fn days_in_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let (year, _) =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    Ok(Value::Number(
        if chrono::NaiveDate::from_ymd_opt(year as i32, 2, 29).is_some() {
            366.0
        } else {
            365.0
        },
    ))
}
fn in_leap_year(receiver: Option<&Value>) -> Result<Value, VmError> {
    let (year, _) =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    Ok(Value::Boolean(
        chrono::NaiveDate::from_ymd_opt(year as i32, 2, 29).is_some(),
    ))
}
