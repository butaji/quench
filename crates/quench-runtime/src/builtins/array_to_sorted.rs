pub(crate) fn array_to_sorted(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined)) else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.toSorted called on null or undefined",
        ));
    };
    let compare = arguments.first();
    if let Some(compare) = compare.filter(|value| !matches!(value, Value::Undefined)) {
        if !crate::conversion::is_callable(compare) {
            return Err(crate::value::error::throw_type_error(
                "Array.prototype.toSorted comparator is not callable",
            ));
        }
    }
    let length = crate::builtins::map_length(receiver)?;
    if length >= 1usize << 32 {
        return Err(crate::value::error::throw_range_error("Invalid array length"));
    }
    let mut sorted = Vec::with_capacity(length);
    for index in 0..length {
        sorted.push(crate::execute::get_property_result(receiver, &index.to_string())?);
    }
    let compare = compare.filter(|value| !matches!(value, Value::Undefined));
    let mut index = 1;
    while index < sorted.len() {
        let value = sorted.remove(index);
        let mut position = 0;
        while position < index {
            let order = if matches!(value, Value::Undefined) {
                1.0
            } else if matches!(sorted[position], Value::Undefined) {
                -1.0
            } else if let Some(compare) = compare {
                crate::conversion::to_number(&crate::functions::execute_target(
                    compare,
                    &Value::Undefined,
                    &[value.clone(), sorted[position].clone()],
                )?)?
            } else {
                let left = crate::intl::tolocale::value::to_string(Some(&value));
                let right = crate::intl::tolocale::value::to_string(Some(&sorted[position]));
                match left.cmp(&right) {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Greater => 1.0,
                    std::cmp::Ordering::Equal => 0.0,
                }
            };
            if order.is_nan() || order >= 0.0 {
                position += 1;
            } else {
                break;
            }
        }
        sorted.insert(position, value);
        index += 1;
    }
    Ok(Value::array(sorted))
}
