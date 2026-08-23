use std::cmp::Ordering;

use num_bigint::{BigInt, Sign};

fn compare_values(left: &Value, right: &Value, compare: fn(f64, f64) -> bool) -> Result<Value, VmError> {
    // Fast path: two ordinary numbers need no ToPrimitive / string/bigint logic.
    if let (Value::Number(l), Value::Number(r)) = (left, right) {
        return Ok(Value::Boolean(!l.is_nan() && !r.is_nan() && compare(*l, *r)));
    }
    let left = crate::conversion::to_primitive(left, "number")?;
    let right = crate::conversion::to_primitive(right, "number")?;
    let symbol = matches!(&left, Value::String(value) if crate::conversion::is_symbol_string(value))
        || matches!(&right, Value::String(value) if crate::conversion::is_symbol_string(value));
    if !symbol {
        if let Some(ordering) = compare_string_ordering(&left, &right) {
            return Ok(Value::Boolean(compare_ordering(ordering, compare, false)));
        }
    }
    if let Some(result) = compare_bigint_operands(&left, &right, compare)? {
        return Ok(Value::Boolean(result));
    }
    let left = crate::conversion::primitive_to_number(&left)?;
    let right = crate::conversion::primitive_to_number(&right)?;
    Ok(Value::Boolean(!left.is_nan() && !right.is_nan() && compare(left, right)))
}

fn compare_bigint_operands(left: &Value, right: &Value, compare: fn(f64, f64) -> bool) -> Result<Option<bool>, VmError> {
    let (bigint, number, reverse) = match (left, right) {
        (Value::BigInt(left), Value::BigInt(right)) => return Ok(Some(compare_ordering(parse_bigint(left)?.cmp(&parse_bigint(right)?), compare, false))),
        (Value::BigInt(bigint), Value::String(string)) if !crate::conversion::is_symbol_string(string) => {

            return Ok(compare_bigint_string(bigint, string, compare, false))
        }
        (Value::String(string), Value::BigInt(bigint)) if !crate::conversion::is_symbol_string(string) => {
            return Ok(compare_bigint_string(bigint, string, compare, true))
        }
        (Value::BigInt(bigint), value) => (bigint, crate::conversion::primitive_to_number(value)?, false),
        (value, Value::BigInt(bigint)) => (bigint, crate::conversion::primitive_to_number(value)?, true),
        _ => return Ok(None),
    };
    Ok(Some(bigint_number_ordering(bigint, number).is_some_and(|ordering| {
        compare_ordering(ordering, compare, reverse)
    })))
}
fn compare_string_ordering(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::String(left), Value::String(right)) => {
            Some(left.encode_utf16().cmp(right.encode_utf16()))
        }
        (Value::String(left), Value::StringUnits(right)) => {
            Some(left.encode_utf16().cmp(right.iter().copied()))
        }
        (Value::StringUnits(left), Value::String(right)) => {
            Some(left.iter().copied().cmp(right.encode_utf16()))
        }
        (Value::StringUnits(left), Value::StringUnits(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn compare_bigint_string(bigint: &str, string: &str, compare: fn(f64, f64) -> bool, reverse: bool) -> Option<bool> {
    let Some(right) = crate::bigint::parse_string(string) else { return Some(false) };
    Some(compare_ordering(bigint.parse::<BigInt>().ok()?.cmp(&right), compare, reverse))
}

fn parse_bigint(value: &str) -> Result<BigInt, VmError> { value.parse().map_err(|_| type_error("Invalid BigInt representation")) }

fn compare_ordering(ordering: Ordering, compare: fn(f64, f64) -> bool, reverse: bool) -> bool {
    let (left, right) = if reverse { (1.0, 0.0) } else { (0.0, 1.0) };
    match ordering { Ordering::Less => compare(left, right), Ordering::Equal => compare(0.0, 0.0), Ordering::Greater => compare(right, left) }
}

fn bigint_number_ordering(bigint: &str, number: f64) -> Option<Ordering> {
    if number.is_nan() { return None }
    let integer = bigint.parse::<BigInt>().ok()?;
    if number == f64::INFINITY { return Some(Ordering::Less) }
    if number == f64::NEG_INFINITY { return Some(Ordering::Greater) }
    let sign = integer.sign();
    if number == 0.0 { return Some(integer.cmp(&BigInt::from(0))) }
    if sign == Sign::Minus && number.is_sign_positive() { return Some(Ordering::Less) }
    if sign != Sign::Minus && number.is_sign_negative() { return Some(Ordering::Greater) }
    let magnitude = if sign == Sign::Minus { -integer } else { integer };
    Some(if number.is_sign_negative() { compare_positive_bigint_number(magnitude, number.abs()).reverse() } else { compare_positive_bigint_number(magnitude, number) })
}

fn compare_positive_bigint_number(integer: BigInt, number: f64) -> Ordering {
    let bits = number.to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let significand = if exponent_bits == 0 { bits & ((1 << 52) - 1) } else { (bits & ((1 << 52) - 1)) | (1 << 52) };
    let exponent = if exponent_bits == 0 { -1074 } else { exponent_bits - 1075 };
    let significand = BigInt::from(significand);
    if exponent >= 0 { integer.cmp(&(significand << exponent as usize)) } else { (integer << (-exponent) as usize).cmp(&significand) }
}
