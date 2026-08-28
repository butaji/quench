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

pub(crate) fn of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| is_constructor(value)) else {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.of called on a non-constructor",
        ));
    };
    create_result(Some(receiver), arguments.to_vec(), false)
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
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let mut values = Vec::new();
    let mut index = 0;
    loop {
        let current = crate::locals::resolved_replacement(source.clone());
        let Value::Array(array) = current else { break };
        if index >= array.logical_len() {
            break;
        }
        let item = array.get_index(index).unwrap_or(Value::Undefined);
        let value = map_item(mapper, &this_arg, item, index)?;
        values.push(value);
        index += 1;
    }
    create_result(receiver, values, true)
}

fn from_iterable(
    receiver: Option<&Value>,
    source: Value,
    mapper: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if !uses_custom_result(receiver) {
        let mut values = Vec::new();
        let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
        let _receiver_guard = crate::collections::iterator::ReceiverUpdateGuard::install();
        crate::collections::iterator::for_each_iterable(source, |item| {
            let index = values.len();
            values.push(map_item(mapper, &this_arg, item, index)?);
            Ok(())
        })?;
        return Ok(Value::array(values));
    }

    let mut result = construct_result(receiver, 0, true)?;
    let mut index = 0;
    let mut values = Vec::new();
    let this_arg = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    let _receiver_guard = crate::collections::iterator::ReceiverUpdateGuard::install();
    crate::collections::iterator::for_each_iterable(source, |item| {
        let value = map_item(mapper, &this_arg, item, index)?;
        result = write_result_element(result.clone(), index, value)?;
        values.push(Value::Undefined);
        index += 1;
        Ok(())
    })?;
    result = set_result_length(result, values.len())?;
    Ok(result)
}

fn uses_custom_result(receiver: Option<&Value>) -> bool {
    matches!(
        receiver,
        Some(value) if !matches!(value, Value::Null | Value::Undefined)
            && !matches!(value, Value::Builtin(crate::ops::Builtin::Array))
    )
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
        values.push(map_item(mapper, &this_arg, item, index)?);
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
        values.push(map_item(mapper, &this_arg, item, index)?);
    }
    Ok(())
}

pub(crate) fn array_like_length(source: &Value) -> Result<usize, crate::execute::VmError> {
    if matches!(source, Value::ArrayBuffer(_) | Value::DataView(_)) {
        return Ok(0);
    }
    let value = crate::execute::get_property_result(source, "length")?;
    let number = crate::conversion::to_number(&value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.floor().min(9_007_199_254_740_991.0) as usize)
}

fn map_item(
    mapper: Option<&Value>,
    this_arg: &Value,
    item: Value,
    index: usize,
) -> Result<Value, crate::execute::VmError> {
    let Some(mapper) = mapper else {
        return Ok(item);
    };
    crate::functions::execute_target(mapper, this_arg, &[item, Value::Number(index as f64)])
}

fn create_result(
    receiver: Option<&Value>,
    values: Vec<Value>,
    iterable: bool,
) -> Result<Value, crate::execute::VmError> {
    let length = values.len();
    let plain_array = match receiver {
        None | Some(Value::Null | Value::Undefined) => true,
        Some(value) => matches!(value, Value::Builtin(crate::ops::Builtin::Array)),
    };
    if plain_array {
        return Ok(Value::array(values));
    }
    // Allocate the final typed-array extent before writing elements.  An
    // empty iterable construction cannot grow a typed-array view; assigning
    // its `length` afterward only changes metadata and leaves a zero-byte
    // backing buffer.
    let mut result = construct_result(receiver, length, iterable)?;
    for (index, value) in values.into_iter().enumerate() {
        result = write_result_element(result, index, value)?;
    }
    if crate::typed_array_ops::is_view(&result) {
        return Ok(result);
    }
    result = set_result_length(result, length)?;
    Ok(result)
}

fn set_result_length(result: Value, length: usize) -> Result<Value, crate::execute::VmError> {
    let updated =
        crate::properties::assign_set_property(&result, "length", Value::Number(length as f64))?;
    crate::locals::replace_value(&result, &updated);
    Ok(updated)
}

fn write_result_element(
    result: Value,
    index: usize,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    let key = index.to_string();
    // Typed-array elements are integer-indexed exotic properties: they are
    // writable but non-configurable, so defining a fresh property would
    // incorrectly trip the ordinary-object read-only check.
    if crate::typed_array_ops::is_view(&result) {
        let updated = crate::builtins::set_property(result.clone(), &key, value);
        crate::locals::replace_value(&result, &updated);
        return Ok(updated);
    }
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
    if matches!(result, Value::Proxy(_)) {
        let descriptor = Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(descriptor)));
        crate::builtins::define_property(&[result.clone(), Value::String(key), descriptor])?;
        return Ok(result);
    }
    let updated = crate::builtins::define_own_property(&result, &key, &descriptor)?;
    crate::locals::replace_value(&result, &updated);
    Ok(updated)
}

#[cfg(test)]
mod arrays_from_tests {
    use super::map_item;
    use crate::value::Value;

    #[test]
    fn map_item_without_mapper_returns_item_without_touching_this_arg() {
        let item = Value::String("canonical".into());
        let this_arg = Value::String("receiver".into());

        let result = map_item(None, &this_arg, item.clone(), 0).expect("mapping succeeds");
        assert!(matches!(&result, Value::String(value) if value == "canonical"));
    }
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
    // Built-in typed arrays must reserve the final extent before iterable
    // elements are written: unlike a custom constructor, their views cannot
    // grow after construction. Custom constructors retain the observable
    // iterable-vs-array-like calling convention.
    let builtin_typed = matches!(
        constructor,
        Value::Builtin(
            crate::ops::Builtin::Float64Array
                | crate::ops::Builtin::Float32Array
                | crate::ops::Builtin::Int8Array
                | crate::ops::Builtin::Int16Array
                | crate::ops::Builtin::Int32Array
                | crate::ops::Builtin::Uint8Array
                | crate::ops::Builtin::Uint16Array
                | crate::ops::Builtin::Uint32Array
                | crate::ops::Builtin::Uint8ClampedArray
                | crate::ops::Builtin::BigInt64Array
                | crate::ops::Builtin::BigUint64Array
        )
    );
    let arguments = if builtin_typed || !iterable {
        vec![Value::Number(length as f64)]
    } else {
        Vec::new()
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
