use super::duration::{construct, from};
use crate::{execute::VmError, value::Value};

pub(super) fn combine(
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
    let values = combined_fields(left, &right, direction);
    if can_construct_directly(&values) {
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    construct(
        &balanced_fields(values)
            .into_iter()
            .map(Value::Number)
            .collect::<Vec<_>>(),
    )
}

fn combined_fields(
    left: &crate::value::ObjectData,
    right: &crate::value::ObjectData,
    direction: f64,
) -> [f64; 10] {
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
    names.map(|name| number(left, name) + direction * number(right, name))
}

fn number(object: &crate::value::ObjectData, name: &str) -> f64 {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map_or(0.0, |(_, value)| match value {
            Value::Number(value) => *value,
            _ => 0.0,
        })
}

fn can_construct_directly(values: &[f64; 10]) -> bool {
    let positive = values[4..10].iter().any(|value| *value > 0.0);
    let negative = values[4..10].iter().any(|value| *value < 0.0);
    !(positive && negative) && !exceeds_unit(values)
}

fn exceeds_unit(values: &[f64; 10]) -> bool {
    values[4].abs() >= 24.0
        || values[5].abs() >= 60.0
        || values[6].abs() >= 60.0
        || values[7].abs() >= 1_000.0
        || values[8].abs() >= 1_000.0
}

fn balanced_fields(mut values: [f64; 10]) -> [f64; 10] {
    let mut largest = (4..10).find(|index| values[*index] != 0.0).unwrap_or(10);
    while values[3] != 0.0 && (5..10).contains(&largest) && values[largest].abs() >= limit(largest)
    {
        largest -= 1;
    }
    values[3] += (values[4] / 24.0).trunc();
    values[4] %= 24.0;
    let time = total_time(&values);
    let days = if largest <= 4 {
        (time / 86_400_000_000_000.0).trunc()
    } else {
        0.0
    };
    values[3] += days;
    let mut remainder = time - days * 86_400_000_000_000.0;
    values[4] = unit(&mut remainder, 3_600_000_000_000.0, largest <= 4);
    values[5] = unit(&mut remainder, 60_000_000_000.0, largest <= 5);
    values[6] = unit(&mut remainder, 1_000_000_000.0, true);
    values[7] = unit(&mut remainder, 1_000_000.0, true);
    values[8] = unit(&mut remainder, 1_000.0, true);
    values[9] = remainder;
    values
}

fn limit(index: usize) -> f64 {
    [60.0, 60.0, 1_000.0, 1_000.0, 1_000.0][index - 5]
}

fn total_time(values: &[f64; 10]) -> f64 {
    values[4] * 3_600_000_000_000.0
        + values[5] * 60_000_000_000.0
        + values[6] * 1_000_000_000.0
        + values[7] * 1_000_000.0
        + values[8] * 1_000.0
        + values[9]
}

fn unit(remainder: &mut f64, divisor: f64, enabled: bool) -> f64 {
    if enabled {
        let value = (*remainder / divisor).trunc();
        *remainder -= value * divisor;
        if value == 0.0 {
            0.0
        } else {
            value
        }
    } else {
        0.0
    }
}
