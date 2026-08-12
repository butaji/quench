fn explicit_bigint(value: Option<&Value>) -> Result<Value, VmError> {
    let primitive = match value {
        Some(value) => crate::conversion::to_primitive(value, "number")?,
        None => return Err(crate::value::error::throw_type_error("Cannot convert value to BigInt")),
    };
    match primitive {
        Value::BigInt(value) => Ok(Value::BigInt(value)),
        Value::Number(value) if value.is_finite() && value.fract() == 0.0 => {
            Ok(Value::BigInt(format!("{value:.0}")))
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
