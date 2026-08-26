pub(crate) fn array_pop(receiver: Option<&Value>) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    if let Some(value) = values.pop_packed_value_shared() {
        return value.unwrap_or(Value::Undefined);
    }
    if values.is_packed_ordinary() {
        let mut updated = values.to_vec();
        let result = updated.pop().unwrap_or(Value::Undefined);
        crate::locals::replace_value(receiver, &Value::array(updated));
        return result;
    }
    let mut updated = values.to_vec();
    let result = updated.pop().unwrap_or(Value::Undefined);
    crate::locals::replace_value(receiver, &Value::array(updated));
    result
}
