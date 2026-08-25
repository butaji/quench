use crate::{execute::VmError, value::Value};

pub(crate) fn construct(year: f64, month: f64) -> Result<Value, VmError> {
    if !year.is_finite() || !(1.0..=12.0).contains(&month) {
        return Err(crate::value::error::throw_range_error(
            "Invalid PlainYearMonth",
        ));
    }
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("year".into(), Value::Number(year)),
            ("month".into(), Value::Number(month)),
            (
                "monthCode".into(),
                Value::String(format!("M{:02}", month as u32)),
            ),
            ("calendarId".into(), Value::String("iso8601".into())),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainYearMonthPrototype),
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
        crate::ops::Builtin::TemporalPlainYearMonthFrom => from(arguments.first()),
        crate::ops::Builtin::TemporalPlainYearMonthCompare => compare(arguments),
        crate::ops::Builtin::TemporalPlainYearMonthCalendarIdGetter => {
            field(receiver, "calendarId")
        }
        crate::ops::Builtin::TemporalPlainYearMonthYearGetter => field(receiver, "year"),
        crate::ops::Builtin::TemporalPlainYearMonthMonthGetter => field(receiver, "month"),
        crate::ops::Builtin::TemporalPlainYearMonthMonthCodeGetter => field(receiver, "monthCode"),
        crate::ops::Builtin::TemporalPlainYearMonthEquals => equals(receiver, arguments.first()),
        crate::ops::Builtin::TemporalPlainYearMonthToString
        | crate::ops::Builtin::TemporalPlainYearMonthToJSON
        | crate::ops::Builtin::TemporalPlainYearMonthToLocaleString => {
            to_string(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainYearMonthToPlainDate => {
            to_plain_date(receiver, arguments.first())
        }
        crate::ops::Builtin::TemporalPlainYearMonthWith => with(receiver, arguments.first()),
        crate::ops::Builtin::TemporalPlainYearMonthAdd => add(receiver, arguments.first(), 1.0),
        crate::ops::Builtin::TemporalPlainYearMonthSubtract => {
            add(receiver, arguments.first(), -1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthUntil => {
            difference(receiver, arguments.first(), 1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthSince => {
            difference(receiver, arguments.first(), -1.0)
        }
        crate::ops::Builtin::TemporalPlainYearMonthDaysInMonthGetter => days_in_month(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthDaysInYearGetter => days_in_year(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthInLeapYearGetter => in_leap_year(receiver),
        crate::ops::Builtin::TemporalPlainYearMonthMonthsInYearGetter => Ok(Value::Number(12.0)),
        crate::ops::Builtin::TemporalPlainYearMonthEraGetter
        | crate::ops::Builtin::TemporalPlainYearMonthEraYearGetter => Ok(Value::Undefined),
        crate::ops::Builtin::TemporalPlainYearMonthValueOf => Err(
            crate::value::error::throw_type_error("Cannot convert PlainYearMonth to a number"),
        ),
        _ => return None,
    })
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    let value =
        value.ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth"))?;
    if let Value::String(text) = value {
        let parts = text.split('-').collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err(crate::value::error::throw_range_error(
                "Invalid PlainYearMonth",
            ));
        }
        return construct(
            parts[parts.len() - 2].parse().unwrap_or(0.0),
            parts[parts.len() - 1].parse().unwrap_or(0.0),
        );
    }
    let year = crate::conversion::to_number(&crate::execute::get_property_result(value, "year")?)?;
    let month = match crate::execute::get_property_result(value, "month")? {
        Value::Undefined => {
            crate::conversion::to_string(&crate::execute::get_property_result(value, "monthCode")?)?
                .trim_start_matches('M')
                .parse()
                .unwrap_or(0.0)
        }
        value => crate::conversion::to_number(&value)?,
    };
    construct(year, month)
}

fn field(receiver: Option<&Value>, name: &str) -> Result<Value, VmError> {
    let receiver = receiver
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid PlainYearMonth receiver"))?;
    crate::execute::get_property_result(receiver, name)
}

fn values(value: &Value) -> Result<(f64, f64), VmError> {
    Ok((
        crate::conversion::to_number(&field(Some(value), "year")?)?,
        crate::conversion::to_number(&field(Some(value), "month")?)?,
    ))
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = values(&from(arguments.first())?)?;
    let right = values(&from(arguments.get(1))?)?;
    Ok(Value::Number(match left.partial_cmp(&right) {
        Some(std::cmp::Ordering::Less) => -1.0,
        Some(std::cmp::Ordering::Greater) => 1.0,
        _ => 0.0,
    }))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?
            == values(&from(other)?)?,
    ))
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
        return Ok(Value::String(format!("{year_text}-{month:02}-01{marker}")));
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
    direction: f64,
) -> Result<Value, VmError> {
    let left =
        values(receiver.ok_or_else(|| crate::value::error::throw_type_error("Invalid receiver"))?)?;
    let right = values(&from(other)?)?;
    crate::temporal::duration::construct(&[
        Value::Number(0.0),
        Value::Number(((right.0 - left.0) * 12.0 + right.1 - left.1) * direction),
    ])
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
