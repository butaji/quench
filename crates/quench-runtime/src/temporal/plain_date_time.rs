use crate::{execute::VmError, value::Value};
use chrono::{Datelike, Duration as CalendarDuration, NaiveDate};

const NAMES: [&str; 9] = [
    "year",
    "month",
    "day",
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let mut fields = arguments
        .iter()
        .take(9)
        .map(crate::conversion::to_number)
        .collect::<Result<Vec<_>, _>>()?;
    while fields.len() < 9 {
        fields.push(0.0);
    }
    validate(&fields)?;
    let month_code = format!("M{:02}", fields[1] as u32);
    let properties = NAMES
        .into_iter()
        .zip(fields)
        .map(|(name, value)| (name.into(), Value::Number(value)))
        .chain([
            ("monthCode".into(), Value::String(month_code)),
            ("calendarId".into(), Value::String("iso8601".into())),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype),
            ),
        ])
        .collect();
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    )))
}

fn validate(fields: &[f64]) -> Result<(), VmError> {
    if !(1.0..=12.0).contains(&fields[1])
        || !(1.0..=31.0).contains(&fields[2])
        || !(0.0..=23.0).contains(&fields[3])
        || fields[4..]
            .iter()
            .any(|value| !(0.0..=999.0).contains(value))
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    Ok(())
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalPlainDateTimeFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalPlainDateTimeCalendarIdGetter
        | crate::ops::Builtin::TemporalPlainDateTimeYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMonthGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMonthCodeGetter
        | crate::ops::Builtin::TemporalPlainDateTimeDayGetter
        | crate::ops::Builtin::TemporalPlainDateTimeHourGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMinuteGetter
        | crate::ops::Builtin::TemporalPlainDateTimeSecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMillisecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMicrosecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeNanosecondGetter => {
            Some(getter(builtin, _receiver))
        }
        crate::ops::Builtin::TemporalPlainDateTimeToString
        | crate::ops::Builtin::TemporalPlainDateTimeToJSON => Some(to_string(_receiver)),
        crate::ops::Builtin::TemporalPlainDateTimeCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalPlainDateTimeEquals => {
            Some(equals(_receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeValueOf => Some(Err(
            crate::value::error::throw_type_error("Cannot convert PlainDateTime to a number"),
        )),
        crate::ops::Builtin::TemporalPlainDateTimeAdd => {
            Some(add(_receiver, arguments.first(), 1.0))
        }
        crate::ops::Builtin::TemporalPlainDateTimeSubtract => {
            Some(add(_receiver, arguments.first(), -1.0))
        }
        crate::ops::Builtin::TemporalPlainDateTimeWith => {
            Some(with(_receiver, arguments.first(), arguments.get(1)))
        }
        _ => None,
    }
}

fn getter(builtin: crate::ops::Builtin, receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let name = match builtin {
        crate::ops::Builtin::TemporalPlainDateTimeCalendarIdGetter => "calendarId",
        crate::ops::Builtin::TemporalPlainDateTimeYearGetter => "year",
        crate::ops::Builtin::TemporalPlainDateTimeMonthGetter => "month",
        crate::ops::Builtin::TemporalPlainDateTimeMonthCodeGetter => "monthCode",
        crate::ops::Builtin::TemporalPlainDateTimeDayGetter => "day",
        crate::ops::Builtin::TemporalPlainDateTimeHourGetter => "hour",
        crate::ops::Builtin::TemporalPlainDateTimeMinuteGetter => "minute",
        crate::ops::Builtin::TemporalPlainDateTimeSecondGetter => "second",
        crate::ops::Builtin::TemporalPlainDateTimeMillisecondGetter => "millisecond",
        crate::ops::Builtin::TemporalPlainDateTimeMicrosecondGetter => "microsecond",
        _ => "nanosecond",
    };
    crate::execute::get_property_result(receiver, name)
}

fn fields(value: &Value) -> Result<Vec<f64>, VmError> {
    NAMES
        .iter()
        .map(|name| crate::execute::get_property_result(value, name))
        .map(|value| value.and_then(|value| crate::conversion::to_number(&value)))
        .collect()
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = fields(&from(arguments.first())?)?;
    let right = fields(&from(arguments.get(1))?)?;
    Ok(Value::Number(match left.partial_cmp(&right) {
        Some(std::cmp::Ordering::Less) => -1.0,
        Some(std::cmp::Ordering::Greater) => 1.0,
        _ => 0.0,
    }))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    Ok(Value::Boolean(fields(receiver)? == fields(&from(other)?)?))
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let duration = crate::temporal::duration::from(duration)?;
    let mut values = fields(receiver)?;
    let months = (number_property(&duration, "years") * 12.0
        + number_property(&duration, "months"))
        * direction;
    let total = values[0] * 12.0 + values[1] - 1.0 + months;
    values[0] = (total / 12.0).floor();
    values[1] = total.rem_euclid(12.0) + 1.0;
    let days = (number_property(&duration, "weeks") * 7.0 + number_property(&duration, "days"))
        * direction;
    if days != 0.0 {
        let date = NaiveDate::from_ymd_opt(values[0] as i32, values[1] as u32, values[2] as u32)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?
            + CalendarDuration::days(days as i64);
        values[0] = date.year() as f64;
        values[1] = date.month() as f64;
        values[2] = date.day() as f64;
    }
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(0.0)
}

fn with(
    receiver: Option<&Value>,
    changes: Option<&Value>,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let changes = changes
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid date-time"))?;
    let mut values = fields(receiver)?;
    let calendar = crate::execute::get_property_result(changes, "calendar")?;
    if !matches!(calendar, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    let month_code = crate::execute::get_property_result(changes, "monthCode")?;
    let month = crate::execute::get_property_result(changes, "month")?;
    if !matches!(month_code, Value::Undefined) {
        values[1] = crate::conversion::to_number(&month_code_number(&month_code)?)?;
    }
    for (index, name) in NAMES.iter().enumerate() {
        let value = crate::execute::get_property_result(changes, name)?;
        if !matches!(value, Value::Undefined) {
            values[index] = if *name == "monthCode" {
                crate::conversion::to_number(&month_code_number(&value)?)?
            } else {
                crate::conversion::to_number(&value)?
            };
        }
    }
    if !matches!(month_code, Value::Undefined)
        && month != Value::Undefined
        && crate::conversion::to_number(&month)?
            != crate::conversion::to_number(&month_code_number(&month_code)?)?
    {
        return Err(crate::value::error::throw_range_error("Month mismatch"));
    }
    let recognized = NAMES.iter().any(|name| {
        crate::execute::get_property_result(changes, name)
            .is_ok_and(|value| !matches!(value, Value::Undefined))
    }) || !matches!(month_code, Value::Undefined);
    if !recognized {
        return Err(crate::value::error::throw_type_error(
            "Insufficient date-time data",
        ));
    }
    let overflow = options
        .and_then(|value| crate::execute::get_property_result(value, "overflow").ok())
        .unwrap_or(Value::String("constrain".into()));
    if values[2] > days_in_month(values[0] as i32, values[1] as u32) as f64 {
        if matches!(overflow, Value::String(value) if value == "constrain") {
            values[2] = days_in_month(values[0] as i32, values[1] as u32) as f64;
        } else {
            return Err(crate::value::error::throw_range_error("Invalid date-time"));
        }
    }
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .map(|date| (date - chrono::Days::new(1)).day())
        .unwrap_or(28)
}

fn to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let values = NAMES
        .iter()
        .map(|name| crate::execute::get_property_result(receiver, name))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|value| crate::conversion::to_number(&value))
        .collect::<Result<Vec<_>, _>>()?;
    let fraction = values[6] as u32 * 1_000_000 + values[7] as u32 * 1_000 + values[8] as u32;
    let suffix = if fraction == 0 {
        String::new()
    } else {
        format!(".{fraction:09}").trim_end_matches('0').to_string()
    };
    Ok(Value::String(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{suffix}",
        values[0], values[1], values[2], values[3], values[4], values[5]
    )))
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    };
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    }
    if let Value::String(text) = value {
        return parse_string(text);
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    }
    let year = crate::execute::get_property_result(value, "year")?;
    let day = crate::execute::get_property_result(value, "day")?;
    let month = crate::execute::get_property_result(value, "month")?;
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    if matches!(year, Value::Undefined)
        || matches!(day, Value::Undefined)
        || (matches!(month, Value::Undefined) && matches!(month_code, Value::Undefined))
    {
        return Err(crate::value::error::throw_type_error(
            "Missing date-time field",
        ));
    }
    let month = if matches!(month, Value::Undefined) {
        month_code_number(&month_code)?
    } else {
        month
    };
    if !matches!(month_code, Value::Undefined)
        && crate::conversion::to_number(&month)?
            != crate::conversion::to_number(&month_code_number(&month_code)?)?
    {
        return Err(crate::value::error::throw_range_error("Month mismatch"));
    }
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    if !matches!(calendar, Value::Undefined) {
        validate_calendar(&calendar)?;
    }
    let mut fields = vec![year, month, day];
    for name in &NAMES[3..] {
        let field = crate::execute::get_property_result(value, name)?;
        fields.push(if matches!(field, Value::Undefined) {
            Value::Number(0.0)
        } else {
            field
        });
    }
    construct(&fields)
}

fn month_code_number(value: &Value) -> Result<Value, VmError> {
    let Value::String(code) = value else {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    };
    code.strip_prefix('M')
        .and_then(|value| value.parse::<f64>().ok())
        .map(Value::Number)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))
}

fn validate_calendar(value: &Value) -> Result<(), VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    let Value::String(calendar) = value else {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    };
    if calendar.eq_ignore_ascii_case("iso8601")
        || (calendar.chars().any(|character| character.is_ascii_digit()) && calendar.contains('-'))
    {
        Ok(())
    } else {
        Err(crate::value::error::throw_range_error("Invalid calendar"))
    }
}

fn parse_string(text: &str) -> Result<Value, VmError> {
    if text.split('[').skip(1).any(|part| {
        part.split_once('=')
            .is_some_and(|(key, _)| key.chars().any(|character| character.is_ascii_uppercase()))
    }) {
        return Err(crate::value::error::throw_range_error("Invalid annotation"));
    }
    let main = text.split('[').next().unwrap_or(text);
    let (date, time) = main
        .split_once('T')
        .or_else(|| main.split_once('t'))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?;
    let date_fields = date.split('-').collect::<Vec<_>>();
    if date_fields.len() != 3 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let (clock, fraction) = time
        .split_once('.')
        .or_else(|| time.split_once(','))
        .map_or((time, ""), |parts| parts);
    let clock = clock.split(':').collect::<Vec<_>>();
    if clock.len() < 2 || clock.len() > 3 || fraction.len() > 9 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let mut fields = date_fields
        .into_iter()
        .chain(clock)
        .map(|part| part.parse::<f64>().unwrap_or(f64::NAN))
        .collect::<Vec<_>>();
    if fields.get(5) == Some(&60.0) {
        fields[5] = 59.0;
    }
    let nanos = format!("{fraction:0<9}").parse::<f64>().unwrap_or(0.0);
    fields.extend([
        (nanos / 1_000_000.0).trunc(),
        (nanos / 1_000.0).trunc() % 1_000.0,
        nanos % 1_000.0,
    ]);
    construct(&fields.into_iter().map(Value::Number).collect::<Vec<_>>())
}
