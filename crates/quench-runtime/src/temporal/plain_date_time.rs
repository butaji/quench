use crate::{execute::VmError, value::Value};

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
        _ => None,
    }
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
    let fields = NAMES
        .iter()
        .map(|name| crate::execute::get_property_result(value, name))
        .collect::<Result<Vec<_>, _>>()?;
    if fields.iter().any(|field| matches!(field, Value::Undefined)) {
        return Err(crate::value::error::throw_type_error(
            "Missing date-time field",
        ));
    }
    construct(&fields)
}

fn parse_string(text: &str) -> Result<Value, VmError> {
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
    let nanos = format!("{fraction:0<9}").parse::<f64>().unwrap_or(0.0);
    fields.extend([
        (nanos / 1_000_000.0).trunc(),
        (nanos / 1_000.0).trunc() % 1_000.0,
        nanos % 1_000.0,
    ]);
    construct(&fields.into_iter().map(Value::Number).collect::<Vec<_>>())
}
