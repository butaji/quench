pub(crate) fn array(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    if let [Value::Number(length)] = arguments {
        if *length >= 0.0 && length.fract() == 0.0 && *length <= u32::MAX as f64 {
            let mut values = Value::array(Vec::new());
            if let Value::Array(values) = &mut values {
                Rc::make_mut(values).set_length(*length as usize);
            }
            return Ok(values);
        }
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    Ok(Value::array(arguments.to_vec()))
}

pub(crate) fn array_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.map called on null or undefined",
        ));
    };
    if matches!(receiver, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.map called on null or undefined",
        ));
    }
    let Some(callback) = arguments.first() else {
        return Ok(Value::array(Vec::new()));
    };
    let length = map_length(receiver)?;
    if length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error("Invalid array length"));
    }
    let mut mapped = Value::array(Vec::new());
    if let Value::Array(values) = &mut mapped {
        Rc::make_mut(values).set_length(length);
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    map_array_values(receiver, callback, this_arg, length, &mut mapped)?;
    Ok(mapped)
}

fn map_array_values(
    receiver: &Value,
    callback: &Value,
    this_arg: &Value,
    length: usize,
    mapped: &mut Value,
) -> Result<(), crate::execute::VmError> {
    if let Value::Array(source) = receiver {
        let mut index = 0;
        while let Some(current) = source.next_index(index, length) {
            index = current.saturating_add(1);
            let Some(value) = map_value(receiver, current)? else { continue };
            let args = [value, Value::Number(current as f64), receiver.clone()];
            let result = crate::functions::execute_target(callback, this_arg, &args)?;
            if let Value::Array(values) = mapped {
                Rc::make_mut(values).set_index(current, result);
            }
        }
    } else {
        for index in 0..length {
            let Some(value) = map_value(receiver, index)? else { continue };
            let args = [value, Value::Number(index as f64), receiver.clone()];
            let result = crate::functions::execute_target(callback, this_arg, &args)?;
            if let Value::Array(values) = mapped {
                Rc::make_mut(values).set_index(index, result);
            }
        }
    }
    Ok(())
}
pub(crate) fn map_length(receiver: &Value) -> Result<usize, crate::execute::VmError> {
    if let Value::Array(values) = receiver {
        return Ok(values.logical_len());
    }
    let length = crate::execute::get_property_result(receiver, "length")?;
    let number = crate::conversion::to_number(&length)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.floor().min(9_007_199_254_740_991.0) as usize)
}
pub(crate) fn map_value(
    receiver: &Value,
    index: usize,
) -> Result<Option<Value>, crate::execute::VmError> {
    if let Value::Array(values) = receiver {
        return Ok(values
            .has_index(index)
            .then(|| values.get_index(index))
            .flatten());
    }
    let key = index.to_string();
    if !crate::with_scope::has_property(receiver, &key)? {
        return Ok(None);
    }
    crate::execute::get_property_result(receiver, &key).map(Some)
}
pub(crate) fn array_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.forEach called on null or undefined",
        ));
    };
    let Some(callback) = arguments.first() else {
        return Err(crate::vm::not_callable());
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let length = map_length(receiver)?;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    array_for_each_values(receiver, callback, this_arg, length)?;
    Ok(Value::Undefined)
}

fn array_for_each_values(
    receiver: &Value,
    callback: &Value,
    this_arg: &Value,
    length: usize,
) -> Result<(), crate::execute::VmError> {
    if let Value::Array(values) = receiver {
        let mut index = 0;
        while let Some(current) = values.next_index(index, length) {
            index = current.saturating_add(1);
            let Some(value) = map_value(receiver, current)? else {
                continue;
            };
            crate::functions::execute_target(
                callback,
                this_arg,
                &[value, Value::Number(current as f64), receiver.clone()],
            )?;
        }
    } else {
        for index in 0..length {
            let Some(value) = map_value(receiver, index)? else {
                continue;
            };
            crate::functions::execute_target(
                callback,
                this_arg,
                &[value, Value::Number(index as f64), receiver.clone()],
            )?;
        }
    }
    Ok(())
}
pub(crate) fn array_filter(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.filter called on null or undefined",
        ));
    };
    if matches!(receiver, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.filter called on null or undefined",
        ));
    }
    let Some(callback) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.filter callback is not callable",
        ));
    };
    let length = map_length(receiver)?;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    let mut filtered = Vec::new();
    for index in 0..length {
        let Some(value) = map_value(receiver, index)? else {
            continue;
        };
        let args = [value.clone(), Value::Number(index as f64), receiver.clone()];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if crate::execute::is_truthy(&result) {
            filtered.push(value);
        }
    }
    Ok(Value::array(filtered))
}

pub(crate) fn array_join(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::String(String::new());
    };
    let separator = arguments
        .first()
        .map_or_else(|| Value::String(",".to_string()), Clone::clone);
    let separator_units = string_units_for_join(&separator);
    let mut units = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            units.extend_from_slice(&separator_units);
        }
        units.extend_from_slice(&string_units_for_join(value));
    }
    crate::strings::from_units(units)
}

fn string_units_for_join(value: &Value) -> Vec<u16> {
    crate::strings::units_of(value)
        .unwrap_or_else(|| value_to_string(value).encode_utf16().collect())
}

pub(crate) fn array_push(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Number(f64::NAN);
    };
    if values.is_packed_ordinary() {
        let mut updated = std::rc::Rc::clone(values);
        let data = std::rc::Rc::make_mut(&mut updated);
        let start = data.logical_len();
        for (offset, value) in arguments.iter().cloned().enumerate() {
            data.set_index(start + offset, value);
        }
        let length = data.logical_len();
        crate::locals::replace_value(receiver, &Value::Array(updated));
        return Value::Number(length as f64);
    }
    let mut result = values.to_vec();
    result.extend_from_slice(arguments);
    let length = result.len();
    values.append_live(arguments);
    crate::locals::replace_value(receiver, &Value::array(result));
    Value::Number(length as f64)
}
