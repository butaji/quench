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
    let mut mapped = array_species_create(&receiver, length)?;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = map_value(&receiver, index)? else {
            continue;
        };
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        mapped = create_data_property_or_throw(mapped, &index.to_string(), result)?;
    }
    Ok(crate::builtins::set_property(
        mapped,
        "length",
        Value::Number(length as f64),
    ))
}

fn array_species_create(
    receiver: &Value,
    length: usize,
) -> Result<Value, crate::execute::VmError> {
    let is_array = matches!(crate::builtins::is_array(Some(receiver))?, Value::Boolean(true));
    if !is_array {
        return Ok(Value::array(Vec::new()));
    }
    let constructor = crate::execute::get_property_result(receiver, "constructor")?;
    if matches!(constructor, Value::Undefined | Value::Builtin(crate::ops::Builtin::Array)) {
        return Ok(Value::array(Vec::new()));
    }
    if !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "Species constructor is not a constructor",
        ));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    if matches!(species, Value::Undefined | Value::Null) {
        return Ok(Value::array(Vec::new()));
    }
    crate::construct::construct_value(&species, &[Value::Number(length as f64)])
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
    let receiver = crate::locals::resolved_replacement(receiver.clone());
    if let Value::Array(_) = &receiver {
        let key = index.to_string();
        if !crate::with_scope::has_property(&receiver, &key)?
            && !crate::arrays::prototype_override_present(&key)
        {
            return Ok(None);
        }
        return crate::execute::get_property_result(&receiver, &key).map(Some);
    }
    let key = index.to_string();
    let has_intrinsic_property = matches!(
        receiver,
        Value::Builtin(builtin) if crate::builtins::object::builtin_owns_property(builtin, &key)
    );
    if !has_intrinsic_property && !crate::with_scope::has_property(&receiver, &key)? {
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

fn create_data_property_or_throw(
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

pub(crate) fn array_join(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::String(String::new());
    };
    let separator = arguments
        .first()
        .map_or_else(|| ",".encode_utf16().collect(), join_units);
    let mut result = Vec::new();
    for (index, value) in values.snapshot().iter().enumerate() {
        if index != 0 {
            result.extend_from_slice(&separator);
        }
        append_join_units(value, &mut result);
    }
    crate::strings::from_units(result)
}

fn join_units(value: &Value) -> Vec<u16> {
    let mut units = Vec::new();
    append_join_units(value, &mut units);
    units
}

fn append_join_units(value: &Value, output: &mut Vec<u16>) {
    match value {
        Value::String(value) => output.extend(value.encode_utf16()),
        Value::StringUnits(units) => output.extend(units.iter().copied()),
        _ => output.extend(value_to_string(value).encode_utf16()),
    }
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
    let mut updated = std::rc::Rc::clone(values);
    let data = std::rc::Rc::make_mut(&mut updated);
    data.append_physical(arguments);
    let length = data.logical_len();
    data.append_live(arguments);
    crate::locals::replace_value(receiver, &Value::Array(updated));
    Value::Number(length as f64)
}
