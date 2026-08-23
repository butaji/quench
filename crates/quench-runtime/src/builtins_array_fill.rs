pub(crate) fn array_fill(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    // Preserve the array's logical length, including holes created by
    // `Array(length)`.  Copying only the physical value vector collapses
    // those holes and makes fill a no-op on sparse arrays.
    let length = values.logical_len();
    let mut result = Value::Array(values.clone());
    let start = fill_index(arguments.get(1), length, 0);
    let end = fill_index(arguments.get(2), length, length);
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    if let Value::Array(updated) = &mut result {
        let data = std::rc::Rc::make_mut(updated);
        for index in start..end {
            data.set_index(index, value.clone());
        }
    }
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
