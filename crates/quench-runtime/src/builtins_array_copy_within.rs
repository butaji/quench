pub(crate) fn array_copy_within(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    let mut updated = values.to_vec();
    let length = updated.len();
    let target = copy_index(arguments.first(), length);
    let start = copy_index(arguments.get(1), length);
    let end = arguments
        .get(2)
        .map_or(length, |value| {
            if matches!(value, Value::Undefined) {
                length
            } else {
                copy_index(Some(value), length)
            }
        });
    let count = end.saturating_sub(start).min(length.saturating_sub(target));
    let source = updated[start..start + count].to_vec();
    updated[target..target + count].clone_from_slice(&source);
    let result = Value::array(updated);
    crate::locals::replace_value(receiver, &result);
    result
}

fn copy_index(value: Option<&Value>, length: usize) -> usize {
    let Some(value) = value else { return 0; };
    let number = crate::intl::tolocale::value::to_number(Some(value));
    if number.is_nan() { return 0; }
    if number.is_sign_negative() {
        return length.saturating_sub(number.abs().floor() as usize);
    }
    (number.floor() as usize).min(length)
}
