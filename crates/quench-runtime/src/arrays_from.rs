fn from(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let source = arguments.first().cloned().unwrap_or(Value::Undefined);
    reject_source(&source)?;
    let mapper = mapper(arguments)?;
    let iterable = !matches!(source, Value::ArrayBuffer(_) | Value::DataView(_))
        && has_iterator(&source)?;
    let mut values = Vec::new();
    if let Value::Array(array) = &source {
        collect_array_iterator(array, mapper.as_ref(), arguments, &mut values)?;
    } else if iterable {
        collect_iterator(source, mapper.as_ref(), arguments, &mut values)?;
    } else {
        collect_array_like(&source, mapper.as_ref(), arguments, &mut values)?;
    }
    create_result(receiver, values, iterable)
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

fn collect_iterator(
    source: Value,
    mapper: Option<&Value>,
    arguments: &[Value],
    values: &mut Vec<Value>,
) -> Result<(), crate::execute::VmError> {
    let _receiver_guard = crate::collections::iterator::ReceiverUpdateGuard::install();
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    crate::collections::iterator::for_each_iterable(source, |item| {
        let index = values.len();
        let value = map_item(mapper, this_arg.clone(), item, index)?;
        values.push(value);
        Ok(())
    })
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
    let mut result = construct_result(receiver, values.len(), iterable)?;
    for (index, value) in values.into_iter().enumerate() {
        let updated = crate::builtins::set_property(result.clone(), &index.to_string(), value);
        crate::locals::replace_value(&result, &updated);
        result = updated;
    }
    Ok(result)
}

fn construct_result(
    receiver: Option<&Value>,
    length: usize,
    iterable: bool,
) -> Result<Value, crate::execute::VmError> {
    let Some(constructor) = receiver.filter(|value| crate::conversion::is_callable(value)) else {
        return Ok(Value::array(Vec::new()));
    };
    let arguments = if iterable {
        Vec::new()
    } else {
        vec![Value::Number(length as f64)]
    };
    crate::construct::construct_value(constructor, &arguments)
}
