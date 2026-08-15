pub(crate) fn array_fill(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    let mut updated = values.to_vec();
    let start = fill_index(arguments.get(1), updated.len(), 0);
    let end = fill_index(arguments.get(2), updated.len(), updated.len());
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    for slot in updated.iter_mut().take(end).skip(start) {
        *slot = value.clone();
    }
    let result = Value::array(updated);
    crate::locals::replace_value(receiver, &result);
    result
}

fn fill_index(value: Option<&Value>, length: usize, default: usize) -> usize {
    let Some(value) = value else { return default; };
    let number = crate::intl::tolocale::value::to_number(Some(value));
    if number.is_nan() { return 0; }
    if number.is_sign_negative() {
        return length.saturating_sub(number.abs().floor() as usize);
    }
    (number.floor() as usize).min(length)
}
