pub(crate) fn array_shift(receiver: Option<&Value>) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    let mut updated = values.to_vec();
    let first = updated.first().cloned().unwrap_or(Value::Undefined);
    if !updated.is_empty() {
        updated.remove(0);
    }
    crate::locals::replace_value(receiver, &Value::array(updated));
    first
}
