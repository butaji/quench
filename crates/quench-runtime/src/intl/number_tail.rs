fn slot_tail(number: &NumberOptions) -> Vec<(String, Value)> {
    let mut properties = Vec::new();
    properties.push((
        "notation".to_string(),
        Value::String(number.notation.clone()),
    ));
    if number.notation == "compact" {
        properties.push((
            "compactDisplay".to_string(),
            Value::String(number.compact_display.clone()),
        ));
    }
    properties.extend([
        (
            "signDisplay".to_string(),
            Value::String(number.sign_display.clone()),
        ),
        (
            "roundingIncrement".to_string(),
            Value::Number(number.rounding_increment as f64),
        ),
        (
            "roundingMode".to_string(),
            Value::String(number.rounding_mode.clone()),
        ),
        (
            "roundingPriority".to_string(),
            Value::String(number.rounding_priority.clone()),
        ),
        (
            "trailingZeroDisplay".to_string(),
            Value::String(number.trailing_zero_display.clone()),
        ),
    ]);
    properties
}

fn slot_primary(number: &NumberOptions) -> Vec<(String, Value)> {
    vec![
        ("locale".to_string(), Value::String(number.locale.clone())),
        (
            "numberingSystem".to_string(),
            Value::String(number.numbering_system.clone()),
        ),
        ("style".to_string(), Value::String(number.style.clone())),
    ]
}

fn valid_unit(unit: Option<&str>) -> bool {
    let Some(unit) = unit else { return false };
    if super::supported_values::UNITS.contains(&unit) {
        return true;
    }
    let Some((left, right)) = unit.split_once("-per-") else {
        return false;
    };
    super::supported_values::UNITS.contains(&left)
        && super::supported_values::UNITS.contains(&right)
}

fn validate_unit_display(value: &str) -> Result<(), VmError> {
    matches!(value, "short" | "narrow" | "long")
        .then_some(())
        .ok_or_else(|| crate::value::error::throw_range_error("invalid unitDisplay"))
}

fn validate_significant_digits(raw: &RawOptions) -> Result<(), VmError> {
    for value in [
        raw.minimum_significant_digits,
        raw.maximum_significant_digits,
    ] {
        if value != -1.0
            && (!value.is_finite() || value.fract() != 0.0 || !(1.0..=21.0).contains(&value))
        {
            return Err(crate::value::error::throw_range_error(
                "invalid significant digits",
            ));
        }
    }
    if raw.minimum_significant_digits >= 0.0
        && raw.maximum_significant_digits >= 0.0
        && raw.maximum_significant_digits < raw.minimum_significant_digits
    {
        return Err(crate::value::error::throw_range_error(
            "maximum significant digits below minimum",
        ));
    }
    Ok(())
}

fn grouping_enabled(value: &str) -> bool {
    matches!(value, "true" | "always" | "auto" | "min2")
}

fn fraction_digits(style: &str, currency: Option<&str>, notation: &str, requested: f64) -> u32 {
    if requested >= 0.0 {
        return requested as u32;
    }
    match style {
        "percent" => 0,
        "currency" if notation != "standard" => 0,
        "currency" if currency == Some("JPY") => 0,
        "currency" => 2,
        _ => requested as u32,
    }
}

fn significant_digits(value: f64) -> Option<u32> {
    (value >= 1.0).then_some(value as u32)
}

fn maximum_fraction(
    style: &str,
    currency: &Option<String>,
    notation: &str,
    requested: f64,
    minimum: u32,
) -> u32 {
    let default = match style {
        "currency" if notation == "compact" => 0,
        "currency" if notation != "standard" => 3,
        "currency" if currency.as_deref() == Some("JPY") => 0,
        "currency" => 2,
        _ => 3,
    };
    if requested >= 0.0 {
        requested as u32
    } else {
        default.max(minimum)
    }
}

fn range_value(value: Option<&Value>) -> Result<f64, VmError> {
    match value {
        None | Some(Value::Undefined) => Err(crate::value::error::throw_type_error(
            "Number range argument is undefined",
        )),
        Some(Value::BigInt(value)) => value
            .parse::<f64>()
            .map_err(|_| crate::value::error::throw_range_error("Number range is out of range")),
        Some(value) => crate::conversion::to_number(value),
    }
}

fn strip_currency_prefix(text: &str, currency: Option<&str>) -> String {
    let symbols = ["$", "€", "¥", "£", "₹", "₽", "₩"];
    let (sign, mut result) = if let Some(rest) = text.strip_prefix('+') {
        ("", rest.to_string())
    } else if let Some(rest) = text.strip_prefix('-') {
        ("-", rest.to_string())
    } else {
        ("", text.to_string())
    };
    for symbol in symbols {
        if result.starts_with(symbol) {
            result = result[symbol.len()..].to_string();
            break;
        }
    }
    let _ = currency;
    format!("{sign}{result}")
}

fn strip_currency_suffix(text: &str) -> String {
    text.rsplit_once('\u{a0}')
        .map_or_else(|| text.to_string(), |(number, _)| number.to_string())
}

fn is_decimal_integer(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn strip_positive_sign(text: &str) -> String {
    text.strip_prefix('+').unwrap_or(text).to_string()
}

