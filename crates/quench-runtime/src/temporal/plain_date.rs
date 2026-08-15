use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let year = number(arguments.first())?;
    let month = number(arguments.get(1))?;
    let day = number(arguments.get(2))?;
    Ok(date_object(year, month, day))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    (builtin == crate::ops::Builtin::TemporalPlainDateFrom).then(|| from(arguments.first()))
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
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
        return Ok(date_object(year, month, day));
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
        return Ok(date_object(year, month, day));
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
        return Ok(date_object(year, month, day));
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
    Ok(date_object(year, month, day))
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
