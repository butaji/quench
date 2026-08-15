use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let year = number(arguments.first())?;
    let month = number(arguments.get(1))?;
    let day = number(arguments.get(2))?;
    if let Some(calendar) = arguments.get(3) {
        if !matches!(calendar, Value::Undefined | Value::String(_)) {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
        if matches!(calendar, Value::String(value) if crate::conversion::is_symbol_string(value)) {
            return Err(crate::value::error::throw_type_error("Invalid calendar"));
        }
        if matches!(calendar, Value::String(value) if !value.eq_ignore_ascii_case("iso8601")) {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
    }
    if !(-271_821.0..=275_760.0).contains(&year)
        || !(1.0..=12.0).contains(&month)
        || !(1.0..=days_in_month(year, month)).contains(&day)
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    if (year == -271_821.0 && (month < 4.0 || month == 4.0 && day < 19.0))
        || (year == 275_760.0 && (month > 9.0 || month == 9.0 && day > 13.0))
    {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    Ok(date_object(year, month, day))
}

fn days_in_month(year: f64, month: f64) -> f64 {
    match month as u32 {
        2 if is_leap_year(year as i32) => 29.0,
        2 => 28.0,
        4 | 6 | 9 | 11 => 30.0,
        _ => 31.0,
    }
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    (builtin == crate::ops::Builtin::TemporalPlainDateFrom).then(|| from(arguments.first()))
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::String(text)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid PlainDate"));
    };
    let calendar_count = text.matches("[u-ca=").count();
    if calendar_count > 1 || text.contains("[!u-ca=") && calendar_count > 0 {
        return Err(crate::value::error::throw_range_error("Multiple calendars"));
    }
    let date = text.split('T').next().unwrap_or(text);
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
    checked_date_object(year, month, day)
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
    Ok(date_object(year, month, day))
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    let value = crate::conversion::to_number(value.unwrap_or(&Value::Undefined))?;
    if !value.is_finite() {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    Ok(value.trunc())
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
