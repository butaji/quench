fn validate_duration_fields(properties: &[(String, Value)]) -> Result<(), VmError> {
    const UNITS: [&str; 10] = [
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
    let fields = properties
        .iter()
        .filter(|(name, _)| UNITS.contains(&name.as_str()))
        .collect::<Vec<_>>();
    if fields.is_empty()
        || fields
            .iter()
            .any(|(_, value)| matches!(value, Value::Undefined))
    {
        return Err(crate::value::error::throw_type_error(
            "invalid duration record",
        ));
    }
    if fields
        .iter()
        .any(|(_, value)| invalid_duration_field(value))
    {
        return Err(runtime_error("RangeError: invalid duration field"));
    }
    Ok(())
}

fn invalid_duration_field(value: &Value) -> bool {
    match value {
        Value::Number(value) => !value.is_finite() || value.fract() != 0.0,
        Value::Undefined => true,
        _ => false,
    }
}

fn format_days(days: i64) -> String {
    let text = days.to_string();
    let grouped = text
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(",");
    format!("{grouped} {}", if days == 1 { "day" } else { "days" })
}

fn digital_fraction(slots: &[(String, Value)], nanoseconds: i64) -> Option<String> {
    let digits = slot_number(slots, "fractionalDigits");
    let raw = format!("{nanoseconds:09}");
    let count = digits.map_or_else(|| raw.trim_end_matches('0').len(), |value| value as usize);
    (count > 0).then(|| format!(".{}", &raw[..count.min(9)]))
}

fn slot_number(slots: &[(String, Value)], key: &str) -> Option<f64> {
    slots.iter().find_map(|(name, value)| {
        (name == key)
            .then_some(value)
            .and_then(|value| match value {
                Value::Number(value) => Some(*value),
                _ => None,
            })
    })
}

fn validate_duration(properties: &[(String, Value)]) -> Result<(), VmError> {
    if raw_duration_out_of_range(properties) {
        return Err(runtime_error("RangeError: invalid duration"));
    }
    let values = [
        number(properties, "years"),
        number(properties, "months"),
        number(properties, "weeks"),
        number(properties, "days"),
        number(properties, "hours"),
        number(properties, "minutes"),
        number(properties, "seconds"),
        number(properties, "milliseconds"),
        number(properties, "microseconds"),
        number(properties, "nanoseconds"),
    ];
    if values[..3]
        .iter()
        .any(|value| value.unsigned_abs() >= 1_u64 << 32)
        || values[3].unsigned_abs() >= 104_249_991_375
        || normalized_nanoseconds(&values).abs() >= 9_007_199_254_740_992_i128 * 1_000_000_000
    {
        return Err(runtime_error("RangeError: invalid duration"));
    }
    let positive = values.iter().any(|value| *value > 0);
    let negative = values.iter().any(|value| *value < 0);
    if positive && negative {
        return Err(runtime_error("RangeError: mixed-sign duration"));
    }
    Ok(())
}

fn raw_duration_out_of_range(properties: &[(String, Value)]) -> bool {
    let factors = [
        ("days", 86_400_000_000_000_i128),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ];
    let total = factors.iter().fold(0_i128, |total, (unit, factor)| {
        total
            + properties.iter().fold(0_i128, |subtotal, (name, value)| {
                subtotal
                    + if name == unit {
                        match value {
                            Value::Number(value) => *value as i128 * factor,
                            _ => 0,
                        }
                    } else {
                        0
                    }
            })
    });
    total.abs() >= 9_007_199_254_740_992_i128 * 1_000_000_000
}

fn normalized_nanoseconds(values: &[i64; 10]) -> i128 {
    i128::from(values[3]) * 86_400_000_000_000
        + i128::from(values[4]) * 3_600_000_000_000
        + i128::from(values[5]) * 60_000_000_000
        + i128::from(values[6]) * 1_000_000_000
        + i128::from(values[7]) * 1_000_000
        + i128::from(values[8]) * 1_000
        + i128::from(values[9])
}

fn slot_value<'a>(slots: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    slots.iter().find_map(|(name, value)| {
        (name == key)
            .then_some(value)
            .and_then(|value| match value {
                Value::String(value) => Some(value.as_str()),
                _ => None,
            })
    })
}

fn number(properties: &[(String, Value)], key: &str) -> i64 {
    properties
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as i64),
            _ => None,
        })
        .unwrap_or(0)
}

fn option(options: Option<&Value>, key: &str) -> Result<Option<String>, VmError> {
    let Some(options) = options else {
        return Ok(None);
    };
    let value = crate::execute::get_property_result(options, key)?;
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    Ok(Some(crate::conversion::to_string(&value)?))
}
