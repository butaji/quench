fn pow(base: f64, exponent: f64) -> f64 {
    if exponent == 0.0 {
        return 1.0;
    }
    if exponent.is_nan() || base.is_nan() || base.abs() == 1.0 && exponent.is_infinite() {
        return f64::NAN;
    }
    if base.is_infinite() {
        return infinite_pow(base, exponent);
    }
    if base == 0.0 {
        return zero_pow(base, exponent);
    }
    base.powf(exponent)
}

fn infinite_pow(base: f64, exponent: f64) -> f64 {
    if exponent.is_sign_positive() {
        if base.is_sign_negative() && is_odd_integer(exponent) {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }
    } else if base.is_sign_negative() && is_odd_integer(exponent) {
        -0.0
    } else {
        0.0
    }
}

fn zero_pow(base: f64, exponent: f64) -> f64 {
    if exponent.is_sign_positive() {
        if base.is_sign_negative() && is_odd_integer(exponent) {
            -0.0
        } else {
            0.0
        }
    } else if base.is_sign_negative() && is_odd_integer(exponent) {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    }
}

fn is_odd_integer(value: f64) -> bool {
    value.is_finite()
        && value == value.trunc()
        && value.abs() < 9_007_199_254_740_992.0
        && value as i64 % 2 != 0
}

pub(crate) fn is_array(value: Option<&Value>) -> Value {
    Value::Boolean(matches!(value, Some(Value::Array(_))))
}

fn value_to_number(value: &Value) -> f64 {
    match value {
        Value::Number(value) => *value,
        Value::String(value) => value.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

pub(crate) fn object(arguments: &[Value]) -> Value {
    match arguments.first() {
        Some(Value::Array(_))
        | Some(Value::ArrayBuffer(_))
        | Some(Value::DataView(_))
        | Some(Value::Float32Array(_))
        | Some(Value::Float64Array(_))
        | Some(Value::Int16Array(_))
        | Some(Value::Int8Array(_))
        | Some(Value::Int32Array(_))
        | Some(Value::Uint16Array(_))
        | Some(Value::Uint8Array(_))
        | Some(Value::Uint8ClampedArray(_))
        | Some(Value::Uint32Array(_))
        | Some(Value::Object(_))
        | Some(Value::Function(_))
        | Some(Value::Builtin(_)) => arguments[0].clone(),
        Some(
            value @ (Value::String(_) | Value::Number(_) | Value::Boolean(_) | Value::BigInt(_)),
        ) => boxed_object(value),
        _ => Value::Object(Rc::new(ObjectData::new(Vec::new()))),
    }
}

fn boxed_object(value: &Value) -> Value {
    let constructor = object::boxed_constructor(value);
    let mut properties = vec![("_value".to_string(), value.clone())];
    if let Some(prototype) = crate::builtin_meta::instance_prototype(constructor) {
        properties.push(("\0prototype".to_string(), Value::Builtin(prototype)));
    }
    Value::Object(Rc::new(ObjectData::new(properties)))
}
