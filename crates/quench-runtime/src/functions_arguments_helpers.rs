fn to_integer_or_infinity(value: f64) -> f64 {
    if value.is_nan() || value == 0.0 || value.is_infinite() && value.is_sign_negative() {
        return 0.0;
    }
    if value.is_infinite() {
        return f64::INFINITY;
    }
    if value == -0.0 {
        0.0
    } else {
        value.trunc()
    }
}

fn length_descriptor(length: f64) -> crate::value::Value {
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), crate::value::Value::Number(length)),
        ("writable".to_string(), crate::value::Value::Boolean(false)),
        ("enumerable".to_string(), crate::value::Value::Boolean(false)),
        ("configurable".to_string(), crate::value::Value::Boolean(true)),
    ])))
}

fn name_descriptor(value: &str) -> crate::value::Value {
    crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        (
            "value".to_string(),
            crate::value::Value::String(value.to_string()),
        ),
        ("writable".to_string(), crate::value::Value::Boolean(false)),
        ("enumerable".to_string(), crate::value::Value::Boolean(false)),
        ("configurable".to_string(), crate::value::Value::Boolean(true)),
    ])))
}
