pub(crate) fn array_reverse(receiver: Option<&Value>) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    let mut updated = values.to_vec();
    updated.reverse();
    let result = Value::array(updated);
    crate::locals::replace_value(receiver, &result);
    result
}
