use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let year = number(arguments.first())?;
    let month = number(arguments.get(1))?;
    let day = number(arguments.get(2))?;
    validate_date(year, month, day)?;
    Ok(date_object(year, month, day))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    (builtin == crate::ops::Builtin::TemporalPlainDateFrom)
        .then(|| from(arguments.first(), arguments.get(1)))
        .or_else(|| match builtin {
            crate::ops::Builtin::TemporalPlainDateToString
            | crate::ops::Builtin::TemporalPlainDateToJSON => Some(to_string(receiver)),
            crate::ops::Builtin::TemporalPlainDateCalendarIdGetter
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
            _ => None,
        })
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
    let year = field_number(object, "year")?;
    let month = field_number(object, "month")?;
    let day = field_number(object, "day")?;
    let value = match builtin {
        crate::ops::Builtin::TemporalPlainDateCalendarIdGetter => Value::String("iso8601".into()),
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
    let year = if year >= 0 && year <= 9999 {
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
    let date = text.split('T').next().unwrap_or(text);
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
        return make_date(year, month, day, options);
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
        return make_date(year, month, day, options);
    }
    if parts.len() != 3 {
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
    make_date(year, month, day, options)
}

fn make_date(year: f64, month: f64, day: f64, options: Option<&Value>) -> Result<Value, VmError> {
    let max = days_in_month(year, month)?;
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
    if day <= 0.0
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
    match field(object, name)? {
        Value::Number(value) => Ok(value),
        _ => Err(crate::value::error::throw_type_error(
            "Invalid PlainDate field",
        )),
    }
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    match value {
        Some(Value::Number(value)) => Ok(*value),
        _ => Err(crate::value::error::throw_type_error("Invalid PlainDate")),
    }
}

fn date_object(year: f64, month: f64, day: f64) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("year".into(), Value::Number(year)),
        ("month".into(), Value::Number(month)),
        ("monthCode".into(), Value::String(format!("M{month:02.0}"))),
        ("day".into(), Value::Number(day)),
        ("calendarId".into(), Value::String("iso8601".into())),
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
            "\0prototype".into(),
            Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype),
        ),
    ])))
}
