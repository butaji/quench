use super::duration::construct;
use crate::{execute::VmError, value::Value};

pub(super) fn from(value: Option<&Value>) -> Result<Value, VmError> {
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
    let (negative, body) = text.strip_prefix('-').map_or_else(
        || (false, text.strip_prefix('+').unwrap_or(text)),
        |body| (true, body),
    );
    let body = body
        .strip_prefix('P')
        .or_else(|| body.strip_prefix('p'))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid duration string"))?;
    let mut values = [0.0; 10];
    let (date, time) = body.split_once(['T', 't']).unwrap_or((body, ""));
    let date_seen = parse_section(date, false, &mut values)?;
    let time_seen = parse_section(time, true, &mut values)?;
    if !date_seen && !time_seen {
        return Err(crate::value::error::throw_range_error(
            "Invalid duration string",
        ));
    }
    if negative {
        values.iter_mut().for_each(|value| *value = -*value);
    }
    values.iter_mut().for_each(|value| {
        if *value == 0.0 {
            *value = 0.0;
        }
    });
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

pub(crate) fn parse_string(text: &str) -> Result<Value, VmError> {
    from_string(text)
}

fn parse_section(section: &str, time: bool, values: &mut [f64; 10]) -> Result<bool, VmError> {
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
        validate_number(number, tail, time, unit)?;
        let (whole, fraction) = parse_number(number)?;
        add_component(values, time, unit, whole, fraction)?;
        seen = true;
        rest = &tail[unit.len_utf8()..];
    }
    Ok(seen)
}

fn parse_number(number: &str) -> Result<(f64, f64), VmError> {
    let (whole, fraction) = number.split_once(['.', ',']).unwrap_or((number, ""));
    let whole = whole
        .parse::<f64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid duration string"))?;
    let fraction = if fraction.is_empty() {
        0.0
    } else {
        let scale = 10_f64.powi(fraction.len() as i32);
        fraction
            .parse::<f64>()
            .map(|value| value / scale)
            .map_err(|_| crate::value::error::throw_range_error("Invalid duration string"))?
    };
    Ok((whole, fraction))
}

fn validate_number(number: &str, tail: &str, time: bool, unit: char) -> Result<(), VmError> {
    let separators = number.matches(['.', ',']).count();
    let fractional = separators > 0;
    let digits = number.split(['.', ',']).collect::<Vec<_>>();
    let invalid = number.is_empty()
        || !number
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '.' | ','))
        || separators > 1
        || digits.first().is_some_and(|part| part.is_empty())
        || digits.get(1).is_some_and(|part| part.is_empty())
        || fractional && (!time || !tail[unit.len_utf8()..].is_empty())
        || unit.eq_ignore_ascii_case(&'S') && digits.get(1).is_some_and(|part| part.len() > 9);
    if invalid {
        return Err(crate::value::error::throw_range_error(
            "Invalid duration string",
        ));
    }
    Ok(())
}

fn add_component(
    values: &mut [f64; 10],
    time: bool,
    unit: char,
    whole: f64,
    fraction: f64,
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
    values[index] += whole;
    if time && fraction != 0.0 {
        add_fraction(values, index, fraction);
    }
    Ok(())
}

fn add_fraction(values: &mut [f64; 10], index: usize, fraction: f64) {
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
