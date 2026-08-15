pub(crate) fn from(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    reject_source(&source)?;
    let mapper = mapper(arguments)?;
    let iterable =
        !matches!(source, Value::ArrayBuffer(_) | Value::DataView(_)) && has_iterator(&source)?;
    if iterable {
        if is_default_array_iterator(&source)? {
            return from_live_array(receiver, source, mapper.as_ref(), arguments);
        }
        return from_iterable(receiver, source, mapper.as_ref(), arguments);
    }
    let mut values = Vec::new();
    if let Value::Array(array) = &source {
        collect_array_iterator(array, mapper.as_ref(), arguments, &mut values)?;
    } else {
        collect_array_like(&source, mapper.as_ref(), arguments, &mut values)?;
    }
    create_result(receiver, values, iterable)
}

fn is_default_array_iterator(source: &Value) -> Result<bool, crate::execute::VmError> {
    Ok(matches!(source, Value::Array(_))
        && matches!(
            crate::execute::get_property_result(source, "Symbol.iterator")?,
            Value::Builtin(crate::ops::Builtin::ArrayIterator)
        ))
}

fn from_live_array(
    receiver: Option<&Value>,
    source: Value,
    mapper: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let mut result = construct_result(receiver, 0, true)?;
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let mut index = 0;
    loop {
        let mut current = source.clone();
        while let Some(updated) = crate::locals::replacement(&current) {
            current = updated;
        }
        let Value::Array(array) = current else { break };
        if index >= array.logical_len() {
            break;
        }
        let item = array.get_index(index).unwrap_or(Value::Undefined);
        let value = map_item(mapper, this_arg.clone(), item, index)?;
        result = write_result_element(result, index, value)?;
        index += 1;
    }
    let updated =
        crate::properties::assign_set_property(&result, "length", Value::Number(index as f64))?;
    Ok(updated)
}

fn from_iterable(
    receiver: Option<&Value>,
    source: Value,
    mapper: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let mut result = construct_result(receiver, 0, true)?;
    let mut index = 0;
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let _receiver_guard = crate::collections::iterator::ReceiverUpdateGuard::install();
    crate::collections::iterator::for_each_iterable(source, |item| {
        let value = map_item(mapper, this_arg.clone(), item, index)?;
        result = write_result_element(result.clone(), index, value)?;
        index += 1;
        Ok(())
    })?;
    let updated =
        crate::properties::assign_set_property(&result, "length", Value::Number(index as f64))?;
    crate::locals::replace_value(&result, &updated);
    Ok(updated)
}

fn collect_array_iterator(
    array: &crate::value::ArrayData,
    mapper: Option<&Value>,
    arguments: &[Value],
    values: &mut Vec<Value>,
) -> Result<(), crate::execute::VmError> {
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let mut index = 0;
    while index < array.logical_len() {
        let item = array.get_index(index).unwrap_or(Value::Undefined);
        values.push(map_item(mapper, this_arg.clone(), item, index)?);
        index += 1;
    }
    Ok(())
}

fn reject_source(source: &Value) -> Result<(), crate::execute::VmError> {
    if matches!(source, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.from requires an array-like object",
        ));
    }
    Ok(())
}

fn mapper(arguments: &[Value]) -> Result<Option<Value>, crate::execute::VmError> {
    let Some(mapper) = arguments.get(1).cloned() else {
        return Ok(None);
    };
    if !crate::conversion::is_callable(&mapper) {
        return Err(crate::value::error::throw_type_error(
            "Array.from mapper must be callable",
        ));
    }
    Ok(Some(mapper))
}

fn has_iterator(source: &Value) -> Result<bool, crate::execute::VmError> {
    let method = crate::execute::get_property_result(source, "Symbol.iterator")?;
    Ok(!matches!(method, Value::Undefined | Value::Null))
}

fn collect_array_like(
    source: &Value,
    mapper: Option<&Value>,
    arguments: &[Value],
    values: &mut Vec<Value>,
) -> Result<(), crate::execute::VmError> {
    let length = array_like_length(source)?;
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    for index in 0..length {
        let item = crate::execute::get_property_result(source, &index.to_string())?;
        values.push(map_item(mapper, this_arg.clone(), item, index)?);
    }
    Ok(())
}

fn array_like_length(source: &Value) -> Result<usize, crate::execute::VmError> {
    if matches!(source, Value::ArrayBuffer(_) | Value::DataView(_)) {
        return Ok(0);
    }
    let value = crate::execute::get_property_result(source, "length")?;
    let number = crate::conversion::to_number(&value)?;
    if !number.is_finite() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.floor().min(9_007_199_254_740_991.0) as usize)
}

fn map_item(
    mapper: Option<&Value>,
    this_arg: Value,
    item: Value,
    index: usize,
) -> Result<Value, crate::execute::VmError> {
    let Some(mapper) = mapper else {
        return Ok(item);
    };
    crate::functions::execute_target(mapper, &this_arg, &[item, Value::Number(index as f64)])
}

fn create_result(
    receiver: Option<&Value>,
    values: Vec<Value>,
    iterable: bool,
) -> Result<Value, crate::execute::VmError> {
    let length = values.len();
    let mut result = construct_result(receiver, length, iterable)?;
    for (index, value) in values.into_iter().enumerate() {
        result = write_result_element(result, index, value)?;
    }
    result =
        crate::properties::assign_set_property(&result, "length", Value::Number(length as f64))?;
    Ok(result)
}

fn write_result_element(
    result: Value,
    index: usize,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    let key = index.to_string();
    let current =
        crate::builtins::object::descriptor(Some(&result), Some(&Value::String(key.clone())))?;
    if !crate::properties::object_is_extensible(&result) && matches!(current, Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Cannot create property on a non-extensible object",
        ));
    }
    let result = if matches!(
        crate::builtins::descriptor_flag(&result, &key, "configurable"),
        Some(true)
    ) {
        let (updated, _) = crate::builtins::delete_property(result, &key);
        updated
    } else {
        result
    };
    let descriptor = vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    let updated = crate::builtins::define_own_property(&result, &key, &descriptor)?;
    crate::locals::replace_value(&result, &updated);
    Ok(updated)
}

fn construct_result(
    receiver: Option<&Value>,
    length: usize,
    iterable: bool,
) -> Result<Value, crate::execute::VmError> {
    let Some(constructor) = receiver else {
        return Ok(Value::array(Vec::new()));
    };
    if !is_constructor(constructor) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.from receiver is not a constructor",
        ));
    }
    let arguments = if iterable {
        Vec::new()
    } else {
        vec![Value::Number(length as f64)]
    };
    crate::construct::construct_value(constructor, &arguments)
}

fn is_constructor(value: &Value) -> bool {
    match value {
        Value::Function(function) => crate::functions::is_constructible(function),
        Value::BoundFunction(bound) => is_constructor(&bound.target),
        Value::Builtin(builtin) => crate::builtin_meta::constructor_name(*builtin).is_some(),
        Value::Proxy(proxy) => is_constructor(&proxy.target),
        _ => false,
    }
}
