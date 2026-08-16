use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let values = (0..10)
        .map(|index| number(arguments.get(index)))
        .collect::<Result<Vec<_>, _>>()?;
    validate_range(&values)?;
    let sign = values
        .iter()
        .find(|value| **value != 0.0)
        .map_or(0.0, |value| value.signum());
    let blank = values.iter().all(|value| *value == 0.0);
    let mut properties = values
        .into_iter()
        .zip([
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ])
        .map(|(value, name)| (name.to_string(), Value::Number(value)))
        .collect::<Vec<_>>();
    properties.extend([
        ("sign".to_string(), Value::Number(sign)),
        ("blank".to_string(), Value::Boolean(blank)),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::TemporalDurationPrototype),
        ),
    ]);
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    )))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalDurationFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalDurationCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalDurationAbs => Some(abs(receiver)),
        crate::ops::Builtin::TemporalDurationToLocaleString => Some(
            crate::intl::duration::format_temporal_duration(receiver, arguments),
        ),
        crate::ops::Builtin::TemporalDurationYearsGetter => Some(field_getter(receiver, "years")),
        crate::ops::Builtin::TemporalDurationMonthsGetter => Some(field_getter(receiver, "months")),
        crate::ops::Builtin::TemporalDurationWeeksGetter => Some(field_getter(receiver, "weeks")),
        crate::ops::Builtin::TemporalDurationDaysGetter => Some(field_getter(receiver, "days")),
        crate::ops::Builtin::TemporalDurationHoursGetter => Some(field_getter(receiver, "hours")),
        crate::ops::Builtin::TemporalDurationMinutesGetter => {
            Some(field_getter(receiver, "minutes"))
        }
        crate::ops::Builtin::TemporalDurationSecondsGetter => {
            Some(field_getter(receiver, "seconds"))
        }
        crate::ops::Builtin::TemporalDurationMillisecondsGetter => {
            Some(field_getter(receiver, "milliseconds"))
        }
        crate::ops::Builtin::TemporalDurationMicrosecondsGetter => {
            Some(field_getter(receiver, "microseconds"))
        }
        crate::ops::Builtin::TemporalDurationNanosecondsGetter => {
            Some(field_getter(receiver, "nanoseconds"))
        }
        _ => None,
    }
}

fn field_getter(receiver: Option<&Value>, field: &str) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    Ok(object
        .iter()
        .find(|(key, _)| key == field)
        .map_or(Value::Number(0.0), |(_, value)| value.clone()))
}

fn abs(receiver: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let arguments = absolute_fields(object);
    construct(&arguments)
}

fn duration_receiver(receiver: Option<&Value>) -> Result<&crate::value::ObjectData, VmError> {
    let Some(Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Temporal.Duration.prototype.abs called on incompatible receiver",
        ));
    };
    let branded = matches!(
        crate::builtins::object::is_prototype_of(
            Some(&Value::Builtin(
                crate::ops::Builtin::TemporalDurationPrototype
            )),
            &[Value::Object(object.clone())],
        )?,
        Value::Boolean(true)
    ) && has_duration_slots(object);
    branded.then_some(object.as_ref()).ok_or_else(|| {
        crate::value::error::throw_type_error(
            "Temporal.Duration.prototype.abs called on incompatible receiver",
        )
    })
}

fn has_duration_slots(object: &crate::value::ObjectData) -> bool {
    let fields = [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ];
    fields.iter().all(|name| {
        object
            .iter()
            .any(|(key, value)| key == *name && matches!(value, Value::Number(_)))
    }) && object
        .iter()
        .any(|(key, value)| key == "sign" && matches!(value, Value::Number(_)))
        && object
            .iter()
            .any(|(key, value)| key == "blank" && matches!(value, Value::Boolean(_)))
}

fn absolute_fields(object: &crate::value::ObjectData) -> Vec<Value> {
    let names = [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ];
    names
        .iter()
        .map(|name| {
            object
                .iter()
                .find(|(key, _)| key == name)
                .map_or(Value::Number(0.0), |(_, value)| match value {
                    Value::Number(value) => Value::Number(value.abs()),
                    _ => Value::Number(0.0),
                })
        })
        .collect()
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    if let Some(Value::String(text)) = value {
        return from_string(text);
    }
    let Some(value) = value.filter(|value| crate::value::is_object(value)) else {
        return Err(crate::value::error::throw_type_error(
            "Duration.from requires a duration-like object",
        ));
    };
    let names = [
        "days",
        "hours",
        "microseconds",
        "milliseconds",
        "minutes",
        "months",
        "nanoseconds",
        "seconds",
        "weeks",
        "years",
    ];
    let fields = names
        .iter()
        .map(|name| {
            crate::execute::get_property_result(value, name).and_then(|field| match field {
                Value::Undefined => Ok(Value::Undefined),
                field => crate::conversion::to_number(&field).map(Value::Number),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if fields.iter().all(|field| matches!(field, Value::Undefined)) {
        return Err(crate::value::error::throw_type_error(
            "Duration requires at least one field",
        ));
    }
    let order = [9, 5, 8, 0, 1, 4, 7, 3, 2, 6];
    construct(&order.map(|index| fields[index].clone()))
}

fn from_string(text: &str) -> Result<Value, VmError> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let body = body
        .strip_prefix('P')
        .or_else(|| body.strip_prefix('p'))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration string"))?;
    let mut values = [0.0; 10];
    let (date, time) = body.split_once(['T', 't']).unwrap_or((body, ""));
    let date_seen = parse_duration_section(date, false, &mut values)?;
    let time_seen = parse_duration_section(time, true, &mut values)?;
    if !date_seen && !time_seen {
        return Err(crate::value::error::throw_range_error(
            "Invalid duration string",
        ));
    }
    if negative {
        values.iter_mut().for_each(|value| *value = -*value);
    }
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn parse_duration_section(
    section: &str,
    time: bool,
    values: &mut [f64; 10],
) -> Result<bool, VmError> {
    let mut rest = section;
    let mut seen = false;
    while !rest.is_empty() {
        let end = rest
            .char_indices()
            .find_map(|(index, character)| character.is_ascii_alphabetic().then_some(index))
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration string"))?;
        let (number, tail) = rest.split_at(end);
        let unit = tail
            .chars()
            .next()
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration string"))?;
        validate_duration_number(number, tail, time, unit)?;
        let magnitude = number
            .replace(',', ".")
            .parse::<f64>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid duration string"))?;
        if !magnitude.is_finite() {
            return Err(crate::value::error::throw_range_error(
                "Invalid duration string",
            ));
        }
        add_duration_component(values, time, unit, magnitude)?;
        seen = true;
        rest = &tail[unit.len_utf8()..];
    }
    Ok(seen)
}

fn validate_duration_number(
    number: &str,
    tail: &str,
    time: bool,
    unit: char,
) -> Result<(), VmError> {
    let separators = number.matches(['.', ',']).count();
    let fractional = separators > 0;
    let digits = number.split(['.', ',']).collect::<Vec<_>>();
    if number.is_empty()
        || !number
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '.' | ','))
        || separators > 1
        || digits.first().is_some_and(|part| part.is_empty())
        || digits.get(1).is_some_and(|part| part.is_empty())
        || fractional && (!time || !tail[unit.len_utf8()..].is_empty())
        || unit.eq_ignore_ascii_case(&'S') && digits.get(1).is_some_and(|part| part.len() > 9)
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid duration string",
        ));
    }
    Ok(())
}

fn add_duration_component(
    values: &mut [f64; 10],
    time: bool,
    unit: char,
    magnitude: f64,
) -> Result<(), VmError> {
    let index = match (time, unit.to_ascii_uppercase()) {
        (false, 'Y') => 0,
        (false, 'M') => 1,
        (false, 'W') => 2,
        (false, 'D') => 3,
        (true, 'H') => 4,
        (true, 'M') => 5,
        (true, 'S') => 6,
        _ => {
            return Err(crate::value::error::throw_range_error(
                "Invalid duration string",
            ))
        }
    };
    let whole = magnitude.trunc();
    values[index] += whole;
    if time && magnitude.fract() != 0.0 {
        add_fractional_time(values, index, magnitude.fract());
    }
    Ok(())
}

fn add_fractional_time(values: &mut [f64; 10], index: usize, fraction: f64) {
    let multiplier = [3_600.0, 60.0, 1.0][index - 4] * 1_000_000_000.0;
    let mut nanos = (fraction * multiplier).round() as i64;
    if index == 4 {
        values[5] += (nanos / 60_000_000_000) as f64;
        nanos %= 60_000_000_000;
    }
    values[6] += (nanos / 1_000_000_000) as f64;
    nanos %= 1_000_000_000;
    values[7] += (nanos / 1_000_000) as f64;
    values[8] += (nanos / 1_000 % 1_000) as f64;
    values[9] += (nanos % 1_000) as f64;
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    validate_compare_options(arguments.get(2))?;
    if same_fields(arguments.first(), arguments.get(1)) {
        return Ok(Value::Number(0.0));
    }
    let left = from(arguments.first())?;
    let right = from(arguments.get(1))?;
    let relative_to_missing = arguments
        .get(2)
        .is_none_or(|value| matches!(value, Value::Undefined));
    if (date_units(&left) || date_units(&right)) && relative_to_missing {
        return Err(crate::value::error::throw_range_error(
            "relativeTo is required for date units",
        ));
    }
    let difference = if date_units(&left) || date_units(&right) {
        duration_value(&left) - duration_value(&right)
    } else {
        exact_time_difference(&left, &right)
    };
    if difference == 0.0 {
        return Ok(Value::Number(0.0));
    }
    Ok(Value::Number(difference.signum()))
}

fn validate_compare_options(options: Option<&Value>) -> Result<(), VmError> {
    if let Some(options) = options {
        if !matches!(options, Value::Undefined) && !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error(
                "Duration.compare options must be an object",
            ));
        }
    }
    Ok(())
}

fn date_units(value: &Value) -> bool {
    ["years", "months", "weeks"]
        .iter()
        .any(|name| number_property(value, name) != 0.0)
}

fn same_fields(left: Option<&Value>, right: Option<&Value>) -> bool {
    let Some((Value::Object(left), Value::Object(right))) = left.zip(right) else {
        return false;
    };
    [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ]
    .iter()
    .all(|name| {
        let left = left
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value);
        let right = right
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value);
        crate::builtins::same_value(left, right)
    })
}

fn duration_value(value: &Value) -> f64 {
    [
        ("years", 31_536_000.0),
        ("months", 2_592_000.0),
        ("weeks", 604_800.0),
        ("days", 86_400.0),
        ("hours", 3_600.0),
        ("minutes", 60.0),
        ("seconds", 1.0),
        ("milliseconds", 1e-3),
        ("microseconds", 1e-6),
        ("nanoseconds", 1e-9),
    ]
    .iter()
    .map(|(name, scale)| number_property(value, name) * scale)
    .sum()
}

fn exact_time_difference(left: &Value, right: &Value) -> f64 {
    let left = time_nanoseconds(left);
    let right = time_nanoseconds(right);
    match left.cmp(&right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

fn time_nanoseconds(value: &Value) -> i128 {
    [
        ("days", 86_400_000_000_000_i128),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| i128::from(number_property(value, name) as i64) * scale)
    .sum()
}

fn validate_range(values: &[f64]) -> Result<(), VmError> {
    if mixed_signs(values)
        || date_fields_out_of_range(&values[..3])
        || time_fields_out_of_range(&values[3..])
    {
        return Err(crate::value::error::throw_range_error(
            "Duration fields are out of range",
        ));
    }
    Ok(())
}

fn mixed_signs(values: &[f64]) -> bool {
    let Some(sign) = values
        .iter()
        .find(|value| **value != 0.0)
        .map(|value| value.signum())
    else {
        return false;
    };
    values
        .iter()
        .any(|value| *value != 0.0 && value.signum() != sign)
}

fn date_fields_out_of_range(values: &[f64]) -> bool {
    values.iter().any(|value| value.abs() > 4_294_967_295.0)
}

fn time_fields_out_of_range(values: &[f64]) -> bool {
    let limits = [
        104_249_991_375.0,
        2_501_999_792_984.0,
        150_119_987_579_017.0,
        9_007_199_254_740_991.0,
        9_007_199_254_740_991.0,
        9_007_199_254_740_991.0,
        9_007_199_254_740_991.0,
    ];
    values
        .iter()
        .zip(limits)
        .any(|(value, limit)| value.abs() > limit)
        || total_time_out_of_range(values)
}

fn total_time_out_of_range(values: &[f64]) -> bool {
    let scales = [
        86_400_000_000_000_i128,
        3_600_000_000_000,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let total = values
        .iter()
        .zip(scales)
        .try_fold(0_i128, |total, (value, scale)| {
            let value = i128::from(*value as i64);
            total.checked_add(value.checked_mul(scale)?)
        });
    let limit = 9_007_199_254_740_991_i128 * 1_000_000_000 + 999_999_999;
    total.is_none_or(|total| total.abs() > limit)
}

fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| match value {
            Value::Number(value) => Some(value),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    let value = match value {
        Some(Value::Number(value)) => Ok(*value),
        Some(Value::Undefined) | None => Ok(0.0),
        Some(value) => crate::conversion::to_number(value),
    }?;
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Duration fields must be integral",
        ));
    }
    Ok(if value == 0.0 { 0.0 } else { value })
}
