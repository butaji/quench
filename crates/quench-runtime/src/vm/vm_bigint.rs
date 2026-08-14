fn explicit_bigint(value: Option<&Value>) -> Result<Value, VmError> {
    let primitive = match value {
        Some(value) => crate::conversion::to_primitive(value, "number")?,
        None => return Err(crate::value::error::throw_type_error("Cannot convert value to BigInt")),
    };
    if crate::conversion::is_symbol(&primitive) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert Symbol to BigInt",
        ));
    }
    match primitive {
        Value::BigInt(value) => Ok(Value::BigInt(value)),
        Value::Number(value) if value.is_finite() && value.fract() == 0.0 => {
            Ok(Value::BigInt(format!("{:.0}", value + 0.0)))
        }
        Value::Number(_) => Err(crate::value::error::throw_range_error(
            "Cannot convert non-integral Number to BigInt",
        )),
        Value::String(value) => crate::bigint::parse_string(&value)
            .map(|value| Value::BigInt(value.to_string()))
            .ok_or_else(|| crate::value::error::throw_syntax_error("Invalid BigInt value")),
        _ => Err(crate::value::error::throw_type_error("Cannot convert value to BigInt")),
    }
}

fn bigint_to_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let value = bigint_value_of(receiver)?;
    let Value::BigInt(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid BigInt receiver"));
    };
    let radix: f64 = arguments
        .first()
        .map(|value| crate::intl::tolocale::value::to_number_result(Some(value)))
        .transpose()?
        .unwrap_or(10.0);
    let radix = if radix.is_nan() { 10.0 } else { radix.trunc() };
    if !(2.0..=36.0).contains(&radix) {
        return Err(crate::value::error::throw_range_error("Invalid BigInt radix"));
    }
    let value = value
        .parse::<num_bigint::BigInt>()
        .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt value"))?;
    Ok(Value::String(value.to_str_radix(radix as u32)))
}

fn bigint_to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Value::BigInt(value) = bigint_value_of(receiver)? else {
        return Err(crate::value::error::throw_type_error(
            "Invalid BigInt receiver",
        ));
    };
    if let Some(formatted) = bigint_significant_format(&value, arguments) {
        return Ok(Value::String(formatted));
    }
    if let Some(formatted) = bigint_fraction_format(&value, arguments) {
        return Ok(Value::String(formatted));
    }
    if arguments
        .get(1)
        .is_some_and(|option| !matches!(option, Value::Undefined))
        || value.trim_start_matches('-').len() <= 15
    {
        let number = value.parse::<f64>().map_err(|_| {
            crate::value::error::throw_type_error("Cannot convert BigInt to number")
        })?;
        return crate::intl::tolocale::number_to_locale_string(
            Some(&Value::Number(number)),
            arguments,
        );
    }
    let locale = arguments.first().and_then(|value| match value {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    });
    let separator = locale
        .is_some_and(|value| value.starts_with("de"))
        .then_some('.')
        .unwrap_or(',');
    Ok(Value::String(group_bigint(&value, separator)))
}

fn bigint_fraction_format(value: &str, arguments: &[Value]) -> Option<String> {
    let Value::Object(options) = arguments.get(1)? else {
        return None;
    };
    let digits = options.properties.iter().find_map(|(key, value)| {
        (key == "minimumFractionDigits").then(|| to_option_number(value) as usize)
    })?;
    let locale = arguments.first().and_then(|value| match value {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    });
    let grouping = locale
        .is_some_and(|value| value.starts_with("de"))
        .then_some('.')
        .unwrap_or(',');
    let decimal = if grouping == '.' { ',' } else { '.' };
    Some(format!(
        "{}{decimal}{}",
        group_bigint(value, grouping),
        "0".repeat(digits)
    ))
}

fn bigint_significant_format(value: &str, arguments: &[Value]) -> Option<String> {
    let Value::Object(options) = arguments.get(1)? else {
        return None;
    };
    let maximum = options.properties.iter().find_map(|(key, value)| {
        (key == "maximumSignificantDigits").then(|| to_option_number(value))
    })? as usize;
    if maximum == 0 || value.trim_start_matches('-').len() <= maximum {
        return None;
    }
    let is_percent = options.properties.iter().any(|(key, value)| {
        key == "style" && crate::intl::tolocale::value::to_string(Some(value)) == "percent"
    });
    let source = if is_percent {
        format!("{}00", value)
    } else {
        value.to_string()
    };
    let rounded = round_integer_significant(&source, maximum);
    let locale = arguments.first().and_then(|value| match value {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    });
    let separator = locale
        .is_some_and(|value| value.starts_with("de"))
        .then_some('.')
        .unwrap_or(',');
    let mut result = group_bigint(&rounded, separator);
    if is_percent {
        if locale.is_some_and(|value| value.starts_with("de")) {
            result.push('\u{a0}');
        }
        result.push('%');
    }
    Some(result)
}

fn to_option_number(value: &Value) -> f64 {
    match value {
        Value::Number(value) => *value,
        Value::String(value) => value.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn round_integer_significant(value: &str, maximum: usize) -> String {
    let negative = value.starts_with('-');
    let digits = value.trim_start_matches('-');
    let keep = &digits[..maximum];
    let round_up = digits.as_bytes()[maximum] >= b'5';
    let mut kept = keep.parse::<num_bigint::BigInt>().unwrap_or_default();
    if round_up {
        kept += 1;
    }
    let mut result = kept.to_string();
    result.push_str(&"0".repeat(digits.len() - maximum));
    if negative {
        format!("-{result}")
    } else {
        result
    }
}

fn group_bigint(value: &str, separator: char) -> String {
    let (sign, digits) = value.split_at(usize::from(value.starts_with('-')));
    let first = digits.len() % 3;
    let mut groups = Vec::new();
    if first != 0 {
        groups.push(digits[..first].to_string());
    }
    for chunk in digits[first..].as_bytes().chunks(3) {
        let Ok(chunk) = std::str::from_utf8(chunk) else {
            return value.to_string();
        };
        groups.push(chunk.to_string());
    }
    format!("{sign}{}", groups.join(&separator.to_string()))
}

fn bigint_as_n(arguments: &[Value], signed: bool) -> Result<Value, VmError> {
    let bits = arguments
        .first()
        .ok_or_else(|| crate::value::error::throw_type_error("BigInt.asN requires bits"))?;
    let number = crate::intl::tolocale::value::to_number_result(Some(bits))?;
    let bits = bigint_width(number)?;
    let value = bigint_argument(arguments.get(1))?;
    let magnitude_bits = usize::try_from(value.magnitude().bits()).unwrap_or(usize::MAX);
    let unchanged = if signed {
        bits > magnitude_bits.saturating_add(1)
    } else {
        value.sign() != num_bigint::Sign::Minus && bits >= magnitude_bits
    };
    if unchanged {
        return Ok(Value::BigInt(value.to_string()));
    }
    let modulus = num_bigint::BigInt::from(1u8) << bits;
    let mut reduced = ((value % &modulus) + &modulus) % &modulus;
    if signed && bits > 0 && reduced >= (num_bigint::BigInt::from(1u8) << (bits - 1)) {
        reduced -= &modulus;
    }
    Ok(Value::BigInt(reduced.to_string()))
}

fn bigint_width(number: f64) -> Result<usize, VmError> {
    if number.is_infinite() {
        return Err(crate::value::error::throw_range_error("Invalid BigInt width"));
    }
    let number = if number.is_nan() { 0.0 } else { number.trunc() };
    if !(0.0..=9_007_199_254_740_991.0).contains(&number) {
        return Err(crate::value::error::throw_range_error("Invalid BigInt width"));
    }
    Ok(number as usize)
}

fn bigint_argument(value: Option<&Value>) -> Result<num_bigint::BigInt, VmError> {
    let value = value.ok_or_else(|| {
        crate::value::error::throw_type_error("Cannot convert undefined to BigInt")
    })?;
    let primitive = crate::conversion::to_primitive(value, "number")?;
    if crate::conversion::is_symbol(&primitive) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert Symbol to BigInt",
        ));
    }
    match primitive {
        Value::BigInt(value) => parse_bigint_argument(&value),
        Value::String(value) => crate::bigint::parse_string(&value)
            .ok_or_else(|| crate::value::error::throw_syntax_error("Invalid BigInt value")),
        Value::Boolean(value) => Ok(num_bigint::BigInt::from(value as u8)),
        _ => Err(crate::value::error::throw_type_error(
            "Cannot convert value to BigInt",
        )),
    }
}

fn parse_bigint_argument(value: &str) -> Result<num_bigint::BigInt, VmError> {
    crate::bigint::parse_string(value)
        .ok_or_else(|| crate::value::error::throw_syntax_error("Invalid BigInt value"))
}
