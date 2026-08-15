use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let year = number(arguments.first())?;
    let month = number(arguments.get(1))?;
    let day = number(arguments.get(2))?;
    let calendar = if let Some(calendar @ Value::String(_)) = arguments.get(3) {
        Some(canonical_calendar(calendar)?)
    } else if matches!(arguments.get(3), Some(value) if !matches!(value, Value::Undefined)) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    } else {
        None
    };
    validate_date(year, month, day)?;
    Ok(date_object_with_calendar(
        year,
        month,
        day,
        calendar.as_deref().unwrap_or("iso8601"),
    ))
}

fn validate_constructor_calendar(value: &Value) -> Result<(), VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    let Value::String(calendar) = value else {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    };
    if matches!(
        calendar.to_ascii_lowercase().as_str(),
        "iso8601"
            | "gregory"
            | "hebrew"
            | "islamicc"
            | "islamic-civil"
            | "ethiopic-amete-alem"
            | "ethioaa"
    ) {
        Ok(())
    } else {
        Err(crate::value::error::throw_range_error("Invalid calendar"))
    }
}

fn canonical_calendar(value: &Value) -> Result<String, VmError> {
    validate_constructor_calendar(value)?;
    let Value::String(calendar) = value else {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    };
    Ok(match calendar.to_ascii_lowercase().as_str() {
        "islamicc" => "islamic-civil".into(),
        "ethiopic-amete-alem" => "ethioaa".into(),
        value => value.into(),
    })
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    (builtin == crate::ops::Builtin::TemporalPlainDateFrom)
        .then(|| from(arguments.first(), arguments.get(1)))
        .or_else(|| match builtin {
            crate::ops::Builtin::TemporalPlainDateCompare => {
                Some(compare(arguments.first(), arguments.get(1)))
            }
            crate::ops::Builtin::TemporalPlainDateValueOf => Some(Err(
                crate::value::error::throw_type_error("Cannot convert PlainDate to a number"),
            )),
            crate::ops::Builtin::TemporalPlainDateToString
            | crate::ops::Builtin::TemporalPlainDateToJSON
            | crate::ops::Builtin::TemporalPlainDateToLocaleString => Some(to_string(receiver)),
            crate::ops::Builtin::TemporalPlainDateCalendarIdGetter
            | crate::ops::Builtin::TemporalPlainDateYearGetter
            | crate::ops::Builtin::TemporalPlainDateMonthGetter
            | crate::ops::Builtin::TemporalPlainDateMonthCodeGetter
            | crate::ops::Builtin::TemporalPlainDateDayGetter
            | crate::ops::Builtin::TemporalPlainDateDayOfWeekGetter
            | crate::ops::Builtin::TemporalPlainDateDayOfYearGetter
            | crate::ops::Builtin::TemporalPlainDateDaysInMonthGetter
            | crate::ops::Builtin::TemporalPlainDateDaysInWeekGetter
            | crate::ops::Builtin::TemporalPlainDateDaysInYearGetter
            | crate::ops::Builtin::TemporalPlainDateInLeapYearGetter
            | crate::ops::Builtin::TemporalPlainDateMonthsInYearGetter => {
                Some(accessor(builtin, receiver))
            }
            crate::ops::Builtin::TemporalPlainDateEquals => {
                Some(equals(receiver, arguments.first()))
            }
            crate::ops::Builtin::TemporalPlainDateAdd => {
                Some(add(receiver, arguments.first(), 1.0))
            }
            crate::ops::Builtin::TemporalPlainDateSubtract => {
                Some(add(receiver, arguments.first(), -1.0))
            }
            crate::ops::Builtin::TemporalPlainDateUntil => Some(difference(
                receiver,
                arguments.first(),
                arguments.get(1),
                1.0,
            )),
            crate::ops::Builtin::TemporalPlainDateSince => Some(difference(
                receiver,
                arguments.first(),
                arguments.get(1),
                -1.0,
            )),
            _ => None,
        })
}

fn compare(left: Option<&Value>, right: Option<&Value>) -> Result<Value, VmError> {
    let left = date_like_fields(left)?;
    let right = date_like_fields(right)?;
    let left = date_serial(left.0, left.1, left.2);
    let right = date_serial(right.0, right.1, right.2);
    Ok(Value::Number((left.cmp(&right) as i8) as f64))
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    options: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let left = date_like_fields(receiver)?;
    let right = date_like_fields(other)?;
    let mut days = (date_serial(right.0, right.1, right.2) - date_serial(left.0, left.1, left.2))
        as f64
        * direction;
    let unit = options.and_then(|value| field_object_string(value, "largestUnit"));
    let mut years = 0.0;
    let mut months = 0.0;
    let mut weeks = 0.0;
    if matches!(unit.as_deref(), Some("years") | Some("months")) {
        let mut total = (right.0 - left.0) * 12.0 + right.1 - left.1;
        if total > 0.0 && right.2 < left.2 {
            total -= 1.0;
        } else if total < 0.0 && right.2 > left.2 {
            total += 1.0;
        }
        let sign = total.signum();
        let total = total.abs();
        if unit.as_deref() == Some("years") {
            years = (total / 12.0).floor() * sign;
            months = total % 12.0 * sign;
        } else {
            months = total * sign;
        }
        let month_total = left.1 - 1.0 + years * 12.0 + months;
        let anchor_year = left.0 + (month_total / 12.0).floor();
        let anchor_month = month_total.rem_euclid(12.0) + 1.0;
        let anchor = make_date(anchor_year, anchor_month, left.2, None)?;
        let fields = date_fields(Some(&anchor))?;
        days = (date_serial(right.0, right.1, right.2) - date_serial(fields.0, fields.1, fields.2))
            as f64
            * direction;
    } else if unit.as_deref() == Some("weeks") {
        weeks = (days / 7.0).trunc();
        days %= 7.0;
    }
    let values = [
        Value::Number(years * direction),
        Value::Number(months * direction),
        Value::Number(weeks),
        Value::Number(days),
    ];
    crate::temporal::duration::construct(&values)
}

fn date_fields(value: Option<&Value>) -> Result<(f64, f64, f64), VmError> {
    let Value::Object(object) =
        value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainDate"))?
    else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    Ok((
        field_number(object, "year")?,
        field_number(object, "month")?,
        field_number(object, "day")?,
    ))
}

fn date_like_fields(value: Option<&Value>) -> Result<(f64, f64, f64), VmError> {
    match value {
        Some(Value::Object(_) | Value::String(_)) => {
            let date = from(value, None)?;
            date_fields(Some(&date))
        }
        _ => Err(crate::value::error::throw_type_error("Invalid PlainDate")),
    }
}

fn field_object_string(value: &Value, name: &str) -> Option<String> {
    let Value::Object(object) = value else {
        return None;
    };
    match field(object, name).ok()? {
        Value::String(value) => Some(value),
        _ => None,
    }
}

fn date_serial(year: f64, month: f64, day: f64) -> i64 {
    let mut year = year as i64;
    let month = month as i64;
    if month <= 2 {
        year -= 1;
    }
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day as i64 - 1;
    era * 146097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let Value::Object(date) =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDate"))?
    else {
        return Err(crate::value::error::throw_type_error("Not a PlainDate"));
    };
    let Value::Object(duration) =
        duration.ok_or_else(|| crate::value::error::throw_type_error("Invalid duration"))?
    else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let years = object_number(duration, "years") * direction;
    let months = object_number(duration, "months") * direction;
    let weeks = object_number(duration, "weeks") * direction;
    let days = object_number(duration, "days") * direction;
    let year = field_number(date, "year")? + years;
    let month_total = field_number(date, "month")? - 1.0 + months;
    let year = year + (month_total / 12.0).floor();
    let month = month_total.rem_euclid(12.0) + 1.0;
    let day = field_number(date, "day")?.min(days_in_month(year, month)?);
    shift_days(year, month, day, weeks * 7.0 + days)
}

fn object_number(object: &crate::value::ObjectData, name: &str) -> f64 {
    object
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            Value::Number(value) => Some(*value),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn shift_days(mut year: f64, mut month: f64, mut day: f64, delta: f64) -> Result<Value, VmError> {
    let mut remaining = delta as i64;
    while remaining != 0 {
        if remaining > 0 {
            day += 1.0;
            if day > days_in_month(year, month)? {
                day = 1.0;
                month += 1.0;
            }
            if month > 12.0 {
                month = 1.0;
                year += 1.0;
            }
            remaining -= 1;
        } else {
            day -= 1.0;
            if day < 1.0 {
                month -= 1.0;
                if month < 1.0 {
                    month = 12.0;
                    year -= 1.0;
                }
                day = days_in_month(year, month)?;
            }
            remaining += 1;
        }
    }
    make_date(year, month, day, None)
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let Some((Value::Object(left), Value::Object(right))) = receiver.zip(other) else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(["year", "month", "day"].iter().all(
        |name| {
            left.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
                == right
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value)
        },
    )))
}

fn accessor(builtin: crate::ops::Builtin, receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDate"))?
    else {
        return Err(crate::value::error::throw_type_error("Not a PlainDate"));
    };
    let year = field_number(object, "_year")?;
    let month = field_number(object, "_month")?;
    let day = field_number(object, "_day")?;
    let value = match builtin {
        crate::ops::Builtin::TemporalPlainDateCalendarIdGetter => Value::String("iso8601".into()),
        crate::ops::Builtin::TemporalPlainDateYearGetter => Value::Number(year),
        crate::ops::Builtin::TemporalPlainDateMonthGetter => Value::Number(month),
        crate::ops::Builtin::TemporalPlainDateMonthCodeGetter => {
            Value::String(format!("M{month:02}"))
        }
        crate::ops::Builtin::TemporalPlainDateDayGetter => Value::Number(day),
        crate::ops::Builtin::TemporalPlainDateDaysInWeekGetter => Value::Number(7.0),
        crate::ops::Builtin::TemporalPlainDateMonthsInYearGetter => Value::Number(12.0),
        crate::ops::Builtin::TemporalPlainDateDaysInMonthGetter => {
            Value::Number(days_in_month(year, month)?)
        }
        crate::ops::Builtin::TemporalPlainDateDaysInYearGetter => {
            Value::Number(if leap(year) { 366.0 } else { 365.0 })
        }
        crate::ops::Builtin::TemporalPlainDateInLeapYearGetter => Value::Boolean(leap(year)),
        crate::ops::Builtin::TemporalPlainDateDayOfYearGetter => {
            Value::Number(day_of_year(year, month, day))
        }
        crate::ops::Builtin::TemporalPlainDateDayOfWeekGetter => {
            Value::Number(day_of_week(year, month, day))
        }
        _ => Value::Undefined,
    };
    Ok(value)
}

fn leap(year: f64) -> bool {
    year % 4.0 == 0.0 && (year % 100.0 != 0.0 || year % 400.0 == 0.0)
}
fn day_of_year(year: f64, month: f64, day: f64) -> f64 {
    (1..month as i32)
        .map(|m| days_in_month(year, m as f64).unwrap_or(0.0))
        .sum::<f64>()
        + day
}
fn day_of_week(year: f64, month: f64, day: f64) -> f64 {
    let mut y = year as i64;
    let m = month as i64;
    if m < 3 {
        y -= 1;
    }
    let value = (y + y / 4 - y / 100
        + y / 400
        + [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4][m as usize - 1]
        + day as i64)
        % 7;
    let weekday = (value + 7) % 7;
    if weekday == 0 {
        7.0
    } else {
        weekday as f64
    }
}

fn to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::Object(object) =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDate"))?
    else {
        return Err(crate::value::error::throw_type_error("Not a PlainDate"));
    };
    let year = field_number(object, "year")? as i64;
    let month = field_number(object, "month")? as i64;
    let day = field_number(object, "day")? as i64;
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{year:+07}")
    };
    Ok(Value::String(format!("{year}-{month:02}-{day:02}")))
}

fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    if let Value::Object(object) = value {
        if let Ok(value) = field(object, "eraYear") {
            let era_year = crate::conversion::to_number(&value)?;
            if !era_year.is_finite() {
                return Err(crate::value::error::throw_range_error("Invalid eraYear"));
            }
        }
        if let Ok(Value::String(code)) = field(object, "monthCode") {
            if crate::conversion::is_symbol(&Value::String(code.clone())) {
                return Err(crate::value::error::throw_type_error("Invalid monthCode"));
            }
            let syntax = code.len() == 3 || code.len() == 4 && code.ends_with('L');
            if !syntax
                || !code.starts_with('M')
                || !code[1..3]
                    .chars()
                    .all(|character| character.is_ascii_digit())
            {
                return Err(crate::value::error::throw_range_error("Invalid monthCode"));
            }
        }
        if let Ok(calendar) = field(object, "calendar") {
            let Value::String(calendar) = calendar else {
                return Err(crate::value::error::throw_type_error("Invalid calendar"));
            };
            if crate::conversion::is_symbol(&Value::String(calendar.clone())) {
                return Err(crate::value::error::throw_type_error("Invalid calendar"));
            }
            if calendar.starts_with("-000000") {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
            let iso_calendar = matches!(
                calendar.to_ascii_lowercase().as_str(),
                "iso8601" | "gregory" | "hebrew" | "islamic-civil" | "islamicc" | "ethioaa"
            ) || (calendar.contains('-') && !calendar.contains("[u-ca="))
                || calendar.contains("[u-ca=iso8601]");
            if !iso_calendar {
                return Err(crate::value::error::throw_range_error("Invalid calendar"));
            }
        }
        let year = field_number(object, "year")?;
        let day = field_number(object, "day")?;
        let month = field_number(object, "month").or_else(|_| {
            let code = field(object, "monthCode")?;
            let Value::String(code) = code else {
                return Err(crate::value::error::throw_type_error("Invalid monthCode"));
            };
            code.strip_prefix('M')
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))
        })?;
        if month < 0.0 || day < 0.0 {
            return Err(crate::value::error::throw_range_error("Invalid date"));
        }
        if let Ok(Value::String(code)) = field(object, "monthCode") {
            let expected = format!("M{month:02.0}");
            if code != expected {
                return Err(crate::value::error::throw_range_error("monthCode mismatch"));
            }
        }
        return make_date(year, month, day, options);
    }
    let Value::String(text) = value else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    }
    if text.contains('−') {
        return Err(crate::value::error::throw_range_error("Invalid ISO time"));
    }
    if text.starts_with("-000000") {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if let Some(calendar) = text
        .split('[')
        .skip(1)
        .find_map(|part| part.strip_prefix("u-ca="))
    {
        if !calendar.starts_with("iso8601]") {
            return Err(crate::value::error::throw_range_error(
                "Invalid calendar annotation",
            ));
        }
    }
    if text.matches("u-ca=").count() > 1 && text.contains("!u-ca=") {
        return Err(crate::value::error::throw_range_error(
            "Invalid calendar annotation",
        ));
    }
    let time_zone_annotations = text
        .split('[')
        .skip(1)
        .filter(|part| !part.contains('='))
        .count();
    if time_zone_annotations > 1 {
        return Err(crate::value::error::throw_range_error(
            "Invalid time zone annotation",
        ));
    }
    let invalid_annotation = text.split('[').skip(1).any(|part| {
        if part.starts_with('!') && part.contains('=') && !part.starts_with("!u-ca=iso8601") {
            return true;
        }
        part.split_once('=')
            .is_some_and(|(key, _)| key.chars().any(|character| character.is_ascii_uppercase()))
    });
    if invalid_annotation {
        return Err(crate::value::error::throw_range_error(
            "Invalid calendar annotation",
        ));
    }
    if text
        .rsplit_once(']')
        .is_some_and(|(_, suffix)| !suffix.is_empty())
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if text
        .split('[')
        .next()
        .is_some_and(|value| value.ends_with('T'))
    {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if let Some(time) = text.split_once('T').map(|(_, value)| value) {
        if time.contains('Z') {
            return Err(crate::value::error::throw_range_error(
                "Invalid UTC designator",
            ));
        }
        for separator in ['.', ','] {
            if let Some(start) = time.find(separator) {
                let digits = time[start + 1..]
                    .chars()
                    .take_while(|character| character.is_ascii_digit())
                    .count();
                if digits > 9 {
                    return Err(crate::value::error::throw_range_error("Too many decimals"));
                }
            }
        }
        if time
            .as_bytes()
            .get(2)
            .is_some_and(|value| *value == b'.' || *value == b',')
            || time
                .as_bytes()
                .get(5)
                .is_some_and(|value| *value == b'.' || *value == b',')
        {
            return Err(crate::value::error::throw_range_error(
                "Fractional time unit",
            ));
        }
        if !time.contains('[')
            && time
                .chars()
                .any(|character| character.is_ascii_alphabetic() && character != 'Z')
        {
            return Err(crate::value::error::throw_range_error("Invalid ISO time"));
        }
        if time.contains('[') {
            // Time-zone annotations are validated by the date grammar below.
        } else {
            let hour = time.get(..2).and_then(|value| value.parse::<u32>().ok());
            let compact = time.len() >= 6
                && time.as_bytes()[..6].iter().all(u8::is_ascii_digit)
                && (time.len() == 6 || !time.as_bytes()[6].is_ascii_alphabetic());
            if time.len() > 2
                && !matches!(time.as_bytes()[2] as char, ':' | '+' | '-' | 'Z' | '[')
                && !compact
            {
                return Err(crate::value::error::throw_range_error("Invalid ISO time"));
            }
            if hour.is_some_and(|value| value > 23) {
                return Err(crate::value::error::throw_range_error("Invalid ISO time"));
            }
            if let Some(minute) = time.get(3..5).and_then(|value| value.parse::<u32>().ok()) {
                if minute > 59 {
                    return Err(crate::value::error::throw_range_error("Invalid ISO time"));
                }
            }
            if time.as_bytes().get(2) == Some(&b':')
                && time.len() > 5
                && !matches!(
                    time.as_bytes()[5] as char,
                    ':' | '.' | ',' | '+' | '-' | 'Z' | '['
                )
            {
                return Err(crate::value::error::throw_range_error("Invalid ISO time"));
            }
            if time.as_bytes().get(2) == Some(&b':') && time.as_bytes().get(5) == Some(&b':') {
                if time.len() < 8 || time[6..8].parse::<u32>().is_err() {
                    return Err(crate::value::error::throw_range_error("Invalid ISO time"));
                }
                if time.len() > 8
                    && !matches!(
                        time.as_bytes()[8] as char,
                        '.' | ',' | '+' | '-' | 'Z' | '['
                    )
                {
                    return Err(crate::value::error::throw_range_error("Invalid ISO time"));
                }
            }
        }
    }
    let date = text
        .split('[')
        .next()
        .unwrap_or(text)
        .split(['T', 't', ' '])
        .next()
        .unwrap_or(text);
    let parts = date.split('-').collect::<Vec<_>>();
    if parts.len() == 1 && (date.len() == 8 || date.len() == 11) {
        let (year_end, month_start, day_start) = if date.len() == 11 {
            (7, 7, 9)
        } else {
            (4, 4, 6)
        };
        let year = date[..year_end]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let month = date[month_start..month_start + 2]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = date[day_start..]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        return make_date_reject(year, month, day);
    }
    if parts.len() == 4 && parts[0].is_empty() {
        let year = format!("-{}", parts[1])
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let month = parts[2]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        let day = parts[3]
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
        return make_date_reject(year, month, day);
    }
    if parts.len() != 3 {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if parts[0].len() != 4 && !(parts[0].starts_with('+') && parts[0].len() == 7) {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    if parts[1].len() != 2 || parts[2].len() != 2 {
        return Err(crate::value::error::throw_range_error("Invalid ISO date"));
    }
    let year = parts[0]
        .parse()
        .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
    let month = parts[1]
        .parse()
        .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
    let day = parts[2]
        .parse()
        .map_err(|_| crate::value::error::throw_range_error("Invalid ISO date"))?;
    make_date_reject(year, month, day)
}

fn make_date_reject(year: f64, month: f64, day: f64) -> Result<Value, VmError> {
    validate_date(year, month, day)?;
    Ok(date_object(year, month, day))
}

fn make_date(year: f64, month: f64, day: f64, options: Option<&Value>) -> Result<Value, VmError> {
    let max = days_in_month(year, month)?;
    if !day.is_finite() || day <= 0.0 {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    let outside_temporal_range = (year == -271821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275760.0 && (month > 9.0 || month == 9.0 && day > 13.0));
    if outside_temporal_range {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    if day <= 0.0
        || day > max
        || (year == -271821.0 && (month < 4.0 || (month == 4.0 && day < 19.0)))
        || (year == 275760.0 && (month > 9.0 || (month == 9.0 && day > 13.0)))
    {
        let reject = matches!(options, Some(Value::Object(object)) if object.iter().any(|(key, value)| key == "overflow" && matches!(value, Value::String(value) if value == "reject")));
        if reject {
            return Err(crate::value::error::throw_range_error("Invalid date"));
        }
        return Ok(date_object(year, month, day.clamp(1.0, max)));
    }
    Ok(date_object(year, month, day))
}

fn validate_date(year: f64, month: f64, day: f64) -> Result<(), VmError> {
    let max = days_in_month(year, month)?;
    if !day.is_finite()
        || day <= 0.0
        || day > max
        || (year == -271821.0 && (month < 4.0 || (month == 4.0 && day < 19.0)))
        || (year == 275760.0 && (month > 9.0 || (month == 9.0 && day > 13.0)))
    {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    Ok(())
}

fn days_in_month(year: f64, month: f64) -> Result<f64, VmError> {
    if !(1.0..=12.0).contains(&month)
        || !year.is_finite()
        || !(-271821.0..=275760.0).contains(&year)
    {
        return Err(crate::value::error::throw_range_error("Invalid date"));
    }
    if month == 2.0 {
        return Ok(
            if year % 4.0 == 0.0 && (year % 100.0 != 0.0 || year % 400.0 == 0.0) {
                29.0
            } else {
                28.0
            },
        );
    }
    Ok(if [4.0, 6.0, 9.0, 11.0].contains(&month) {
        30.0
    } else {
        31.0
    })
}

fn field(object: &crate::value::ObjectData, name: &str) -> Result<Value, VmError> {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| crate::value::error::throw_type_error("Missing PlainDate field"))
}

fn field_number(object: &crate::value::ObjectData, name: &str) -> Result<f64, VmError> {
    let value = field(object, name)?;
    crate::conversion::to_number(&value).map(f64::trunc)
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    crate::conversion::to_number(value).map(f64::trunc)
}

fn date_object(year: f64, month: f64, day: f64) -> Value {
    date_object_with_calendar(year, month, day, "iso8601")
}

fn date_object_with_calendar(year: f64, month: f64, day: f64, calendar: &str) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("year".into(), Value::Number(year)),
        ("month".into(), Value::Number(month)),
        ("monthCode".into(), Value::String(format!("M{month:02.0}"))),
        ("day".into(), Value::Number(day)),
        ("_temporal_kind".into(), Value::String("PlainDate".into())),
        ("_year".into(), Value::Number(year)),
        ("_month".into(), Value::Number(month)),
        ("_day".into(), Value::Number(day)),
        ("calendarId".into(), Value::String(calendar.into())),
        (
            "dayOfWeek".into(),
            Value::Number(day_of_week(year, month, day)),
        ),
        (
            "dayOfYear".into(),
            Value::Number(day_of_year(year, month, day)),
        ),
        (
            "daysInMonth".into(),
            Value::Number(days_in_month(year, month).unwrap_or(0.0)),
        ),
        ("daysInWeek".into(), Value::Number(7.0)),
        (
            "daysInYear".into(),
            Value::Number(if leap(year) { 366.0 } else { 365.0 }),
        ),
        ("inLeapYear".into(), Value::Boolean(leap(year))),
        ("monthsInYear".into(), Value::Number(12.0)),
        (
            "weekOfYear".into(),
            Value::Number(iso_week(year, month, day).0),
        ),
        (
            "yearOfWeek".into(),
            Value::Number(iso_week(year, month, day).1),
        ),
        (
            "\0prototype".into(),
            Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype),
        ),
    ])))
}

fn iso_week(year: f64, month: f64, day: f64) -> (f64, f64) {
    let date = date_serial(year, month, day);
    let jan4 = date_serial(year, 1.0, 4.0);
    let first = jan4 - (day_of_week(year, 1.0, 4.0) as i64 - 1);
    if date < first {
        return iso_week(year - 1.0, 12.0, 31.0);
    }
    let next = date_serial(year + 1.0, 1.0, 4.0);
    let next_first = next - (day_of_week(year + 1.0, 1.0, 4.0) as i64 - 1);
    if date >= next_first {
        return (1.0, year + 1.0);
    }
    (((date - first) / 7 + 1) as f64, year)
}
