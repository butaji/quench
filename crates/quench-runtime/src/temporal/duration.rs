use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let values = (0..10)
        .map(|index| number(arguments.get(index)))
        .collect::<Vec<_>>();
    if values
        .iter()
        .any(|value| value.is_finite() && value.fract() != 0.0)
    {
        return Err(crate::value::error::throw_range_error(
            "Duration fields must be integral",
        ));
    }
    let whole_seconds = values[3] as i128 * 86_400
        + values[4] as i128 * 3_600
        + values[5] as i128 * 60
        + values[6] as i128;
    let subsecond_nanos =
        values[7] as i128 * 1_000_000 + values[8] as i128 * 1_000 + values[9] as i128;
    let total_nanos = whole_seconds * 1_000_000_000 + subsecond_nanos;
    let max_seconds = 9_007_199_254_740_991_i128;
    let max_nanos = max_seconds * 1_000_000_000 + 999_999_999;
    if total_nanos.abs() > max_nanos {
        return Err(crate::value::error::throw_range_error(
            "Duration is outside the supported range",
        ));
    }
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
        crate::ops::Builtin::TemporalDurationNegated => Some(negated(receiver)),
        crate::ops::Builtin::TemporalDurationAbs => Some(absolute(receiver)),
        crate::ops::Builtin::TemporalDurationToJSON => Some(to_json(receiver)),
        crate::ops::Builtin::TemporalDurationAdd => Some(combine(receiver, arguments.first(), 1.0)),
        crate::ops::Builtin::TemporalDurationSubtract => {
            Some(combine(receiver, arguments.first(), -1.0))
        }
        crate::ops::Builtin::TemporalDurationValueOf => Some(Err(
            crate::value::error::throw_type_error("Cannot convert Duration to a number"),
        )),
        _ => None,
    }
}

fn combine(
    receiver: Option<&Value>,
    other: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let (Some(Value::Object(left)), Some(right)) = (receiver, other) else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let right = from(Some(right))?;
    let Value::Object(right) = right else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let number = |object: &crate::value::ObjectData, name: &str| object_number(object, name);
    let mut values = [0.0; 10];
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
    for (index, name) in names.iter().enumerate() {
        values[index] = number(left, name) + direction * number(&right, name);
    }
    let positive = values[4..10].iter().any(|value| *value > 0.0);
    let negative = values[4..10].iter().any(|value| *value < 0.0);
    let exceeds_unit = values[4].abs() >= 24.0
        || values[5].abs() >= 60.0
        || values[6].abs() >= 60.0
        || values[7].abs() >= 1_000.0
        || values[8].abs() >= 1_000.0;
    if !(positive && negative) && !exceeds_unit {
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    let mut largest_time_unit = (4..10).find(|index| values[*index] != 0.0).unwrap_or(10);
    while values[3] != 0.0
        && (5..10).contains(&largest_time_unit)
        && values[largest_time_unit].abs()
            >= [60.0, 60.0, 1_000.0, 1_000.0, 1_000.0][largest_time_unit - 5]
    {
        largest_time_unit -= 1;
    }
    values[3] += (values[4] / 24.0).trunc();
    values[4] %= 24.0;
    let time = values[4] * 3_600_000_000_000.0
        + values[5] * 60_000_000_000.0
        + values[6] * 1_000_000_000.0
        + values[7] * 1_000_000.0
        + values[8] * 1_000.0
        + values[9];
    values[4] = if largest_time_unit <= 4 {
        (time / 3_600_000_000_000.0).trunc()
    } else {
        0.0
    };
    let remainder = time - values[4] * 3_600_000_000_000.0;
    values[5] = if largest_time_unit <= 5 {
        (remainder / 60_000_000_000.0).trunc()
    } else {
        0.0
    };
    let remainder = remainder - values[5] * 60_000_000_000.0;
    values[6] = (remainder / 1_000_000_000.0).trunc();
    let remainder = remainder - values[6] * 1_000_000_000.0;
    values[7] = (remainder / 1_000_000.0).trunc();
    let remainder = remainder - values[7] * 1_000_000.0;
    values[8] = (remainder / 1_000.0).trunc();
    values[9] = remainder - values[8] * 1_000.0;
    let arguments = values.into_iter().map(Value::Number).collect::<Vec<_>>();
    construct(&arguments)
}

fn to_json(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let mut date = String::new();
    for (name, suffix) in [
        ("years", 'Y'),
        ("months", 'M'),
        ("weeks", 'W'),
        ("days", 'D'),
    ] {
        let number = object_number(object, name);
        if number != 0.0 {
            date.push_str(&format!("{}{}", number_text(number), suffix));
        }
    }
    let mut time = String::new();
    for (name, suffix) in [("hours", 'H'), ("minutes", 'M')] {
        let number = object_number(object, name);
        if number != 0.0 {
            time.push_str(&format!("{}{}", number_text(number), suffix));
        }
    }
    let seconds = object_number(object, "seconds")
        + object_number(object, "milliseconds") / 1_000.0
        + object_number(object, "microseconds") / 1_000_000.0
        + object_number(object, "nanoseconds") / 1_000_000_000.0;
    if seconds != 0.0 {
        time.push_str(&format!("{}S", seconds_text(seconds)));
    }
    if time.is_empty() && date.is_empty() {
        return Ok(Value::String("PT0S".into()));
    }
    if !time.is_empty() {
        date.push('T');
        date.push_str(&time);
    }
    let sign = object_number(object, "sign") < 0.0;
    Ok(Value::String(format!(
        "{}P{date}",
        if sign { "-" } else { "" }
    )))
}

fn number_text(value: f64) -> String {
    let value = value.abs();
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn seconds_text(value: f64) -> String {
    let nanos = (value.abs() * 1_000_000_000.0).round() as i64;
    let whole = nanos / 1_000_000_000;
    let fraction = nanos % 1_000_000_000;
    if fraction == 0 {
        return whole.to_string();
    }
    format!("{whole}.{:09}", fraction)
        .trim_end_matches('0')
        .to_string()
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

fn absolute(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
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
    let values = names
        .iter()
        .map(|name| {
            match object
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
            {
                Some(Value::Number(value)) => Value::Number(value.abs()),
                _ => Value::Number(0.0),
            }
        })
        .collect::<Vec<_>>();
    construct(&values)
}

fn negated(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
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
    let values = names
        .iter()
        .map(|name| {
            match object
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
            {
                Some(Value::Number(value)) => Value::Number(-value),
                _ => Value::Number(0.0),
            }
        })
        .collect::<Vec<_>>();
    construct(&values)
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    if let Some(Value::String(text)) = value {
        return parse_string(text);
    }
    let Some(Value::Object(object)) = value else {
        return Err(crate::value::error::throw_type_error(
            "Duration.from requires a duration-like object",
        ));
    };
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
    if !names
        .iter()
        .any(|name| object.iter().any(|(key, _)| key == name))
    {
        return Err(crate::value::error::throw_type_error(
            "Duration-like object has no duration fields",
        ));
    }
    let arguments = names
        .iter()
        .map(|name| {
            object
                .iter()
                .find(|(key, _)| key == name)
                .map_or(Value::Undefined, |(_, value)| value.clone())
        })
        .collect::<Vec<_>>();
    construct(&arguments)
}

fn parse_string(text: &str) -> Result<Value, VmError> {
    let (negative, text) = text
        .strip_prefix('-')
        .map_or((false, text), |value| (true, value));
    let Some(rest) = text.strip_prefix('P') else {
        return Err(crate::value::error::throw_range_error("Invalid duration"));
    };
    let mut values = vec![Value::Number(0.0); 10];
    let mut number = String::new();
    let mut in_time = false;
    for character in rest.chars() {
        if character == 'T' {
            in_time = true;
            continue;
        }
        if character.is_ascii_digit() || matches!(character, '-' | '+' | '.') {
            number.push(character);
            continue;
        }
        let value: f64 = number
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid duration"))?;
        let raw = number.clone();
        number.clear();
        let index = match character {
            'Y' => 0,
            'M' if in_time => 5,
            'M' => 1,
            'W' => 2,
            'D' => 3,
            'H' => 4,
            'S' => 6,
            _ => return Err(crate::value::error::throw_range_error("Invalid duration")),
        };
        if index == 6 {
            if let Some((whole, fraction)) = raw.split_once('.') {
                let whole: f64 = whole
                    .parse()
                    .map_err(|_| crate::value::error::throw_range_error("Invalid duration"))?;
                let digits = fraction.chars().take(9).collect::<String>();
                let nanos = format!("{digits:0<9}")
                    .parse::<f64>()
                    .map_err(|_| crate::value::error::throw_range_error("Invalid duration"))?;
                let sign = if negative { -1.0 } else { 1.0 };
                values[6] = Value::Number(sign * whole.abs());
                values[9] = Value::Number(sign * nanos);
                continue;
            }
        }
        values[index] = Value::Number(if negative { -value } else { value });
    }
    if let Value::Number(seconds) = values[6] {
        if seconds.fract() != 0.0 {
            values[6] = Value::Number(seconds.trunc());
            values[9] = Value::Number(
                (if negative { -1.0 } else { 1.0 })
                    * (seconds.fract().abs() * 1_000_000_000.0).round(),
            );
        }
    }
    construct(&values)
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = from(arguments.first())?;
    let right = from(arguments.get(1))?;
    let options = arguments.get(2);
    if let Some(options) = options {
        if !matches!(options, Value::Undefined) && !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error("Invalid options"));
        }
    }
    if same_fields(arguments.first(), arguments.get(1)) {
        return Ok(Value::Number(0.0));
    }
    if (date_units(&left) || date_units(&right))
        && matches!(arguments.get(2), None | Some(Value::Undefined))
    {
        return Err(crate::value::error::throw_range_error(
            "relativeTo is required for date units",
        ));
    }
    let difference = duration_value(&left) - duration_value(&right);
    if difference == 0.0 {
        return Ok(Value::Number(0.0));
    }
    Ok(Value::Number(difference.signum()))
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

fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| match value {
            Value::Number(value) => Some(value),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(value)) if *value != 0.0 => *value,
        Some(Value::Number(_)) => 0.0,
        Some(Value::Undefined) | None => 0.0,
        _ => f64::NAN,
    }
}
