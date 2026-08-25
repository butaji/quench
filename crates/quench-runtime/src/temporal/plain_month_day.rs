use crate::{execute::VmError, value::Value};

pub(crate) fn construct(month: f64, day: f64) -> Result<Value, VmError> {
    if !(1.0..=12.0).contains(&month) || !(1.0..=31.0).contains(&day) {
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
            ("referenceISODay".into(), Value::Number(1972.0)),
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
        crate::ops::Builtin::TemporalPlainMonthDayFrom => from(arguments.first()),
        crate::ops::Builtin::TemporalPlainMonthDayCompare => compare(arguments),
        crate::ops::Builtin::TemporalPlainMonthDayCalendarIdGetter => field(receiver, "calendarId"),
        crate::ops::Builtin::TemporalPlainMonthDayDayGetter => field(receiver, "day"),
        crate::ops::Builtin::TemporalPlainMonthDayMonthCodeGetter => field(receiver, "monthCode"),
        crate::ops::Builtin::TemporalPlainMonthDayEquals => equals(receiver, arguments.first()),
        crate::ops::Builtin::TemporalPlainMonthDayToString
        | crate::ops::Builtin::TemporalPlainMonthDayToJSON
        | crate::ops::Builtin::TemporalPlainMonthDayToLocaleString => {
            to_string(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainMonthDayToPlainDate => {
            to_plain_date(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainMonthDayWith => with(receiver, arguments.first()),
        crate::ops::Builtin::TemporalPlainMonthDayValueOf => Err(
            crate::value::error::throw_type_error("Cannot convert PlainMonthDay to a number"),
        ),
        _ => return None,
    })
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    let value =
        value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainMonthDay"))?;
    if let Value::String(text) = value {
        let parts = text.split('-').collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainMonthDay",
            ));
        }
        return construct(
            parts[parts.len() - 2].parse().unwrap_or(0.0),
            parts[parts.len() - 1].parse().unwrap_or(0.0),
        );
    }
    let month = crate::execute::get_property_result(value, "month")
        .or_else(|_| crate::execute::get_property_result(value, "monthCode"))?;
    let month = match month {
        Value::String(code) => code.trim_start_matches('M').parse().unwrap_or(0.0),
        value => crate::conversion::to_number(&value)?,
    };
    let day = crate::conversion::to_number(&crate::execute::get_property_result(value, "day")?)?;
    construct(month, day)
}

fn fields(value: &Value) -> Result<(String, f64), VmError> {
    let month = crate::execute::get_property_result(value, "monthCode")?;
    let day = crate::conversion::to_number(&crate::execute::get_property_result(value, "day")?)?;
    Ok((crate::conversion::to_string(&month)?, day))
}

fn field(receiver: Option<&Value>, name: &str) -> Result<Value, VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainMonthDay receiver"))?;
    crate::execute::get_property_result(receiver, name)
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = fields(&from(arguments.first())?)?;
    let right = fields(&from(arguments.get(1))?)?;
    Ok(Value::Number(
        match (left.0.cmp(&right.0), left.1.partial_cmp(&right.1)) {
            (std::cmp::Ordering::Less, _) | (_, Some(std::cmp::Ordering::Less)) => -1.0,
            (std::cmp::Ordering::Greater, _) | (_, Some(std::cmp::Ordering::Greater)) => 1.0,
            _ => 0.0,
        },
    ))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        fields(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?
            == fields(&from(other)?)?,
    ))
}

fn to_string(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let (month, day) =
        fields(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    let name = calendar_name(options)?;
    let month = month.trim_start_matches('M');
    if matches!(name.as_str(), "always" | "critical") {
        let year = receiver
            .and_then(|value| crate::execute::get_property_result(value, "referenceISODay").ok())
            .and_then(|value| crate::conversion::to_number(&value).ok())
            .unwrap_or(1972.0) as i32;
        let marker = if name == "critical" {
            "[!u-ca=iso8601]"
        } else {
            "[u-ca=iso8601]"
        };
        return Ok(Value::String(format!("{year:04}-{month}-{day:02}{marker}")));
    }
    Ok(Value::String(format!("{month}-{day:02}")))
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

fn to_plain_date(receiver: Option<&Value>, year: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let year = crate::conversion::to_number(
        year.ok_or_else(|| crate::value::error::throw_type_error("Missing year"))?,
    )?;
    let (month, day) = fields(receiver)?;
    let month = month.trim_start_matches('M').parse().unwrap_or(0.0);
    crate::temporal::plain_date::construct(&[
        Value::Number(year),
        Value::Number(month),
        Value::Number(day),
    ])
}

fn with(receiver: Option<&Value>, changes: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?;
    let (month, day) = fields(receiver)?;
    let changes = changes
        .filter(|v| crate::value::is_object(v))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid fields"))?;
    let day = match crate::execute::get_property_result(changes, "day")? {
        Value::Undefined => day,
        value => crate::conversion::to_number(&value)?,
    };
    let month = match crate::execute::get_property_result(changes, "monthCode")? {
        Value::Undefined => month.trim_start_matches('M').parse().unwrap_or(0.0),
        Value::String(code) => code.trim_start_matches('M').parse().unwrap_or(0.0),
        value => crate::conversion::to_number(&value)?,
    };
    construct(month, day)
}
