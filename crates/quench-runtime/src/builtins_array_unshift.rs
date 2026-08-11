pub(crate) fn array_unshift(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    let mut updated = arguments.to_vec();
    updated.extend(values.to_vec());
    let length = updated.len();
    crate::locals::replace_value(receiver, &Value::array(updated));
    Value::Number(length as f64)
}
