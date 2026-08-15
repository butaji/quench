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
            _ => None,
        })
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
            "\0prototype".into(),
            Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype),
        ),
    ])))
}
