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
    let receiver = crate::construct::to_object(receiver)?;
    let length = map_length(&receiver)?;
    let Some(callback) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.map callback is not callable",
        ));
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.map callback is not callable",
        ));
    }
    if length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    let fast_result = default_array_map_result(&receiver)?;
    let mut mapped_values = fast_result.then(|| Value::array(Vec::new()));
    if let Some(Value::Array(values)) = mapped_values.as_mut() {
        std::rc::Rc::make_mut(values).set_length(length);
    }
    let mut mapped = (!fast_result)
        .then(|| array_species_create(&receiver, length))
        .transpose()?
        .unwrap_or(Value::Undefined);
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = map_value(&receiver, index)? else {
            continue;
        };
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if let Some(Value::Array(values)) = mapped_values.as_mut() {
            std::rc::Rc::make_mut(values).set_index(index, result);
        } else {
            mapped = create_data_property_or_throw(mapped, &index.to_string(), result)?;
        }
    }
    if let Some(values) = mapped_values {
        return Ok(values);
    }
    let previous = mapped.clone();
    let result = crate::builtins::set_property(
        mapped,
        "length",
        Value::Number(length as f64),
    );
    crate::locals::replace_value(&previous, &result);
    Ok(result)
}

fn default_array_map_result(receiver: &Value) -> Result<bool, crate::execute::VmError> {
    let Value::Array(values) = receiver else {
        return Ok(false);
    };
    let packed = values.is_packed_ordinary();
    let overrides = crate::builtins::intrinsic_override_keys(crate::ops::Builtin::ArrayPrototype);
    let constructor = crate::execute::get_property_result(receiver, "constructor")?;
    Ok(packed
        && overrides.is_empty()
        && matches!(
            constructor,
            Value::Undefined | Value::Builtin(crate::ops::Builtin::Array)
        ))
}

pub(crate) fn array_species_create(
    receiver: &Value,
    length: usize,
) -> Result<Value, crate::execute::VmError> {
    let is_array = matches!(crate::builtins::is_array(Some(receiver))?, Value::Boolean(true));
    if !is_array {
        if length > u32::MAX as usize {
            return Err(crate::value::error::throw_range_error("Invalid array length"));
        }
        return Ok(Value::array(Vec::new()));
    }
    let constructor = crate::execute::get_property_result(receiver, "constructor")?;
    if matches!(constructor, Value::Undefined | Value::Builtin(crate::ops::Builtin::Array)) {
        if length > u32::MAX as usize {
            return Err(crate::value::error::throw_range_error("Invalid array length"));
        }
        return Ok(Value::array(Vec::new()));
    }
    if !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "Species constructor is not a constructor",
        ));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    if matches!(species, Value::Undefined | Value::Null) {
        if length > u32::MAX as usize {
            return Err(crate::value::error::throw_range_error("Invalid array length"));
        }
        return Ok(Value::array(Vec::new()));
    }
    crate::construct::construct_value(&species, &[Value::Number(length as f64)])
}
pub(crate) fn map_length(receiver: &Value) -> Result<usize, crate::execute::VmError> {
    if let Value::Array(values) = receiver {
        return Ok(values.logical_len());
    }
    let length = if let Value::Builtin(builtin) = receiver {
        crate::builtins::read_descriptor_value(*builtin, "length")
            .unwrap_or(crate::execute::get_property_result(receiver, "length")?)
    } else {
        crate::execute::get_property_result(receiver, "length")?
    };
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
    let receiver = crate::locals::resolved_replacement(receiver.clone());
    if let Value::Array(_) = &receiver {
        if let Value::Array(array) = &receiver {
            if array.is_packed_ordinary()
                && !crate::arrays::prototype_override_present(&index.to_string())
            {
                return Ok(array.get_index(index));
            }
        }
        let key = index.to_string();
        if !crate::with_scope::has_property(&receiver, &key)?
            && !crate::arrays::prototype_override_present(&key)
        {
            return Ok(None);
        }
        return crate::execute::get_property_result(&receiver, &key).map(Some);
    }
    let key = index.to_string();
    if !crate::with_scope::has_property(&receiver, &key)? {
        let descriptor = crate::builtins::object::descriptor(
            Some(&receiver),
            Some(&Value::String(key.clone())),
        )?;
        if matches!(descriptor, Value::Undefined) {
            return Ok(None);
        }
    }
    crate::execute::get_property_result(&receiver, &key).map(Some)
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
    let receiver = crate::construct::to_object(receiver)?;
    let length = map_length(&receiver)?;
    let Some(callback) = arguments.first() else {
        return Err(crate::vm::not_callable());
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = map_value(&receiver, index)? else {
            continue;
        };
        crate::functions::execute_target(
            callback,
            this_arg,
            &[value, Value::Number(index as f64), receiver.clone()],
        )?;
    }
    Ok(Value::Undefined)
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
    let receiver = crate::construct::to_object(receiver)?;
    let length = map_length(&receiver)?;
    let Some(callback) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.filter callback is not callable",
        ));
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.filter callback is not callable",
        ));
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    let mut filtered = array_species_create(&receiver, 0)?;
    let mut output = 0usize;
    for index in 0..length {
        let Some(value) = map_value(&receiver, index)? else {
            continue;
        };
        let args = [value.clone(), Value::Number(index as f64), receiver.clone()];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        if crate::execute::is_truthy(&result) {
            filtered = create_data_property_or_throw(filtered, &output.to_string(), value)?;
            output = output.saturating_add(1);
        }
    }
    if matches!(&filtered, Value::Array(values) if values.logical_len() == output) {
        return Ok(filtered);
    }
    Ok(crate::builtins::set_property(
        filtered,
        "length",
        Value::Number(output as f64),
    ))
}

pub(crate) fn create_data_property_or_throw(
    target: Value,
    key: &str,
    value: Value,
) -> Result<Value, crate::execute::VmError> {
    if let Value::Array(values) = &target {
        let index = key.parse::<usize>().ok();
        let has_own = crate::builtins::object::has_own_property(
            Some(&target),
            Some(&Value::String(key.to_string())),
        ) == Value::Boolean(true);
        if !has_own && !crate::properties::object_is_extensible(&target) {
            return Err(crate::value::error::throw_type_error(
                "Cannot define a property on a non-extensible object",
            ));
        }
        if let Some(descriptor) = values.descriptor(key) {
            if crate::builtins::descriptor_flag(&target, key, "configurable") == Some(false) {
                return Err(crate::value::error::throw_type_error(
                    "Cannot redefine non-configurable property",
                ));
            }
            if crate::builtins::descriptor_flag(&target, key, "writable") == Some(false)
                && crate::builtins::descriptor_flag(&target, key, "configurable") != Some(true)
            {
                return Err(crate::value::error::throw_type_error(
                    "Cannot redefine non-writable property",
                ));
            }
            let _ = descriptor;
        }
        let Some(index) = index else {
            return crate::builtins::define_own_property_public(&target, key, &[
                ("value".to_string(), value),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(true)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]);
        };
        let mut updated = std::rc::Rc::clone(values);
        let data = std::rc::Rc::make_mut(&mut updated);
        data.set_index(index, value.clone());
        data.define_descriptor(
            key,
            Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
                ("value".to_string(), value),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(true)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]))),
        );
        let replacement = Value::Array(updated.clone());
        crate::locals::replace_value(&target, &replacement);
        return Ok(replacement);
    }
    let target = if crate::builtins::descriptor_flag(&target, key, "configurable") == Some(true) {
        let (target, deleted) = crate::execute::delete_property(target, key);
        if deleted {
            target
        } else {
            return Err(crate::value::error::throw_type_error(
                "Cannot redefine non-configurable property",
            ));
        }
    } else {
        target
    };
    let descriptor = vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    crate::builtins::define_own_property_public(&target, key, &descriptor)
}

pub(crate) fn array_join(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined)) else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.join called on null or undefined",
        ));
    };
    let receiver = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&receiver)?;
    let separator: Vec<u16> = match arguments.first() {
        Some(Value::Undefined) | None => ",".encode_utf16().collect(),
        Some(value) => crate::conversion::to_string(value)?.encode_utf16().collect(),
    };
    let mut result = Vec::new();
    for index in 0..length {
        if index != 0 {
            result.extend_from_slice(&separator);
        }
        let Some(value) = crate::builtins::map_value(&receiver, index)? else {
            continue;
        };
        if matches!(value, Value::Null | Value::Undefined) { continue; }
        if let Some(units) = crate::strings::units_of(&value) {
            // Append UTF-16 units before materializing: adjacent array
            // elements can form a surrogate pair across the element boundary.
            result.extend(units);
        } else {
            result.extend(crate::conversion::to_string(&value)?.encode_utf16());
        }
    }
    Ok(crate::strings::from_units(result))
}

pub(crate) fn array_to_string(
    receiver: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined)) else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.toString called on null or undefined",
        ));
    };
    let receiver = crate::construct::to_object(receiver)?;
    let join = crate::execute::get_property_result(&receiver, "join")?;
    if crate::conversion::is_callable(&join) {
        return crate::functions::execute_target(&join, &receiver, &[]);
    }
    crate::builtins::prototype_to_string_result(Some(&receiver))
}

pub(crate) fn array_push(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    // Fast path for the canonical packed array. All guards mirror the
    // observable checks below, while avoiding a generic `length` property
    // lookup and receiver reification on every sequential append.
    if let Some(Value::Array(array)) = receiver {
        if array.is_packed_ordinary() {
            let length = array.logical_len();
            let final_length = length.checked_add(arguments.len()).ok_or_else(|| {
                crate::value::error::throw_type_error("Array length exceeds maximum safe integer")
            })?;
            if (final_length as u64) > 9_007_199_254_740_991u64 {
                return Err(crate::value::error::throw_type_error(
                    "Array length exceeds maximum safe integer",
                ));
            }
            if array.append_shared_numbers_proven(arguments)
                || array.append_shared_values_proven(arguments)
            {
                return Ok(Value::Number(final_length as f64));
            }
        }
    }
    let receiver = crate::construct::to_object(receiver.unwrap_or(&Value::Undefined))?;
    let length = crate::builtins::map_length(&receiver)?;
    let final_length = length.checked_add(arguments.len()).ok_or_else(|| {
        crate::value::error::throw_type_error("Array length exceeds maximum safe integer")
    })?;
    if (final_length as u64) > 9_007_199_254_740_991u64 {
        return Err(crate::value::error::throw_type_error(
            "Array length exceeds maximum safe integer",
        ));
    }
    if let Value::Array(array) = &receiver {
        let packed_ordinary = array.is_packed_ordinary();
        // `is_packed_ordinary` already proves a clean Array.prototype and no
        // indexed/accessor interception, so avoid rebuilding one million
        // string keys for the common append path. The explicit checks remain
        // for arrays whose structure needs complete observable semantics.
        let prototype_indices_clear = packed_ordinary || (0..arguments.len()).all(|offset| {
            let key = (length + offset).to_string();
            !crate::arrays::prototype_override_present(&key)
                && crate::property_define::accessor(&receiver, &key, "set").is_none()
        });
        if packed_ordinary
            && crate::properties::object_is_extensible(&receiver)
            && crate::builtins::descriptor_flag(&receiver, "length", "writable") != Some(false)
            && crate::builtins::intrinsic_override_keys(crate::ops::Builtin::ArrayPrototype)
                .is_empty()
            && prototype_indices_clear
        {
            if array.append_shared_numbers_proven(arguments) {
                return Ok(Value::Number(final_length as f64));
            }
            if array.append_shared_values_proven(arguments) {
                return Ok(Value::Number(final_length as f64));
            }
            let (mut values, _, _) = array.hot_storage();
            values.extend(arguments.iter().cloned());
            let updated = Value::array(values);
            crate::locals::replace_value(&receiver, &updated);
            return Ok(Value::Number(final_length as f64));
        }
    }
    let mut updated = receiver.clone();
    for (offset, value) in arguments.iter().cloned().enumerate() {
        let key = (length + offset).to_string();
        updated = crate::properties::assign_set_property(
            &updated,
            &key,
            value.clone(),
        )?;
        if key == u32::MAX.to_string() {
            if let Value::Array(values) = &updated {
                let mut values = std::rc::Rc::clone(values);
                std::rc::Rc::make_mut(&mut values).set_property(&key, value);
                updated = Value::Array(values);
            }
        }
        crate::locals::replace_value(&receiver, &updated);
    }
    if matches!(receiver, Value::Array(_)) && final_length > u32::MAX as usize {
        crate::locals::replace_value(&receiver, &updated);
        return Err(crate::value::error::throw_range_error("Invalid array length"));
    }
    updated = crate::locals::resolved_replacement(updated);
    updated = crate::properties::assign_set_property(
        &updated,
        "length",
        Value::Number(final_length as f64),
    )?;
    crate::locals::replace_value(&receiver, &updated);
    Ok(Value::Number(final_length as f64))
}
