fn non_enumerable_descriptor(value: &Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}

pub(crate) fn same_value_zero(left: &Value, right: &Value) -> bool {
    if let (Value::Number(left), Value::Number(right)) = (left, right) {
        return left.is_nan() && right.is_nan() || left == right;
    }
    same_value(Some(left), Some(right))
}
