pub(crate) fn delete_property(target: Value, key: &str) -> (Value, bool) {
    match target {
        Value::Object(properties) => delete_object_property_value(properties, key),
        Value::ObjectAlias(alias) => delete_object_alias_property(alias, key),
        Value::Array(values) => delete_array_property(values, key),
        Value::Function(function) => delete_function_property(function, key),
        Value::BoundFunction(bound) => delete_bound_function_property(bound, key),
        Value::Builtin(builtin) => {
            let deletable = crate::builtins::props::is_builtin_deletable(builtin, key);
            if deletable {
                crate::builtins::remove_intrinsic_override(builtin, key);
                crate::builtins::mark_builtin_prototype_property_removed(builtin, key);
            }
            (Value::Builtin(builtin), deletable)
        }
        Value::DataView(view) => {
            let removed = view.remove_own_property(key, &descriptor_key(key));
            (Value::DataView(view), removed)
        }
        value => (value, true),
    }
}

fn delete_bound_function_property(
    bound: std::rc::Rc<crate::value::BoundFunctionValue>,
    key: &str,
) -> (Value, bool) {
    if bound.target == Value::Builtin(crate::ops::Builtin::AbstractModuleSource)
        && key == "prototype"
    {
        return (Value::BoundFunction(bound), false);
    }
    {
        let mut properties = bound.properties.borrow_mut();
        let metadata = descriptor_key(key);
        properties.retain(|(name, _)| name != key && name != &metadata);
        if bound.target == Value::Builtin(crate::ops::Builtin::AbstractModuleSource)
            && matches!(key, "length" | "name")
        {
            properties.push((crate::builtins::deleted_key(key), Value::Boolean(true)));
        }
    }
    (Value::BoundFunction(bound), true)
}

fn delete_object_property_value(
    properties: Rc<crate::value::ObjectData>,
    key: &str,
) -> (Value, bool) {
    if global_constant(&properties, key) || boxed_string_non_configurable(&properties, key) {
        return (Value::Object(properties), false);
    }
    if properties.iter().any(|(name, _)| name == "\0realm")
        && !matches!(key, "undefined" | "Infinity" | "NaN")
    {
        return (delete_object_property(properties, key), true);
    }
    if descriptor_flag_in(&properties, key, "configurable") == Some(false) {
        return (Value::Object(properties), false);
    }
    (delete_object_property(properties, key), true)
}

fn delete_array_property(mut values: Rc<crate::value::ArrayData>, key: &str) -> (Value, bool) {
    if array_descriptor_flag(&values, key, "configurable") == Some(false) {
        return (Value::Array(values), false);
    }
    if values.is_arguments() || key != "length" {
        Rc::make_mut(&mut values).delete_property(key);
        return (Value::Array(values), true);
    }
    (Value::Array(values), false)
}

fn boxed_string_non_configurable(properties: &Rc<crate::value::ObjectData>, key: &str) -> bool {
    let is_boxed_string = properties
        .iter()
        .any(|(name, value)| name == "_value" && matches!(value, Value::String(_)));
    is_boxed_string && (key == "length" || key.parse::<usize>().is_ok())
}

fn delete_object_alias_property(alias: crate::value::ObjectAliasValue, key: &str) -> (Value, bool) {
    let Some(properties) = alias.0.borrow().upgrade() else {
        return (Value::ObjectAlias(alias), true);
    };
    let result = delete_property(Value::Object(properties), key);
    retarget_object_alias(&alias, &result.0);
    result
}

fn global_constant(properties: &Rc<crate::value::ObjectData>, key: &str) -> bool {
    if !matches!(key, "NaN" | "Infinity" | "undefined") {
        return false;
    }
    crate::vm::is_global_object(&Value::Object(Rc::clone(properties)))
}

fn delete_function_property(function: Rc<crate::value::FunctionValue>, key: &str) -> (Value, bool) {
    let configurable = descriptor_flag_in(&function.properties.borrow(), key, "configurable");
    if configurable == Some(false) {
        return (Value::Function(function), false);
    }
    let metadata = descriptor_key(key);
    function
        .properties
        .borrow_mut()
        .retain(|(name, _)| name != key && name != &metadata);
    (Value::Function(function), true)
}

fn delete_object_property(properties: Rc<crate::value::ObjectData>, key: &str) -> Value {
    let mut values: Vec<(String, Value)> = properties
        .iter()
        .filter(|(name, _)| name != key && name != &descriptor_key(key))
        .cloned()
        .collect();
    if crate::vm::is_global_object(&Value::Object(Rc::clone(&properties)))
        && crate::vm::global_builtin_exists(key)
    {
        values.push((crate::builtins::deleted_key(key), Value::Boolean(true)));
    }
    Value::Object(Rc::new(crate::value::ObjectData::with_private_slots(
        values,
        Rc::clone(&properties.private_slots),
    )))
}

fn define_property_value(target: Value, key: &str, value: Value) -> Value {
    match target {
        Value::Object(properties) => {
            crate::builtins::builtins_cells::set_object_property(properties, key, value)
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::ArrayBuffer(buffer) => {
            buffer.set_own_property(key, value);
            Value::ArrayBuffer(buffer)
        }
        Value::Function(function) => {
            let function = std::rc::Rc::clone(&function);
            {
                let mut properties = function.properties.borrow_mut();
                if let Some((_, current)) =
                    properties.iter_mut().rev().find(|(name, _)| name == key)
                {
                    *current = value;
                } else {
                    properties.push((key.to_string(), value));
                }
            }
            Value::Function(function)
        }
        Value::BoundFunction(bound)
            if bound.target == Value::Builtin(crate::ops::Builtin::AbstractModuleSource) =>
        {
            {
                let mut properties = bound.properties.borrow_mut();
                let deleted = crate::builtins::deleted_key(key);
                properties.retain(|(name, _)| name != key && name != &deleted);
                properties.push((key.to_string(), value));
            }
            Value::BoundFunction(bound)
        }
        target => set_property(target, key, value),
    }
}

const MAX_DENSE_ARRAY_INDEX_GAP: usize = 1024;

fn define_array_descriptor(target: &mut Value, key: &str, descriptor: Vec<(String, Value)>) {
    let Value::Array(values) = target else { return };
    let mut values = Rc::clone(values);
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    let writable = descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "writable").then_some(value));
    if accessor || writable == Some(&Value::Boolean(false)) {
        if let Some(index) = crate::arrays::array_index(key) {
            Rc::make_mut(&mut values).disconnect_index(index as usize);
        }
    }
    if let Some(index) = crate::arrays::array_index(key) {
        let data = Rc::make_mut(&mut values);
        if index as usize >= data.logical_len() {
            data.set_length(index as usize + 1);
        }
    }
    Rc::make_mut(&mut values).define_descriptor(
        key,
        Value::Object(Rc::new(crate::value::ObjectData::new(descriptor))),
    );
    *target = Value::Array(values);
}

fn validate_array_length_descriptor(
    target: &Value,
    key: &str,
    descriptor: &[(String, Value)],
) -> Result<(), crate::execute::VmError> {
    if !matches!(target, Value::Array(_)) || key != "length" {
        return Ok(());
    }
    let Some(value) = descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "value").then_some(value))
    else {
        return Ok(());
    };
    let number = crate::conversion::to_number(value)?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > u32::MAX as f64 {
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    Ok(())
}

fn prepare_array_length_definition(
    target: &Value,
    key: &str,
    descriptor: &[(String, Value)],
) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Array(values) = target else {
        return Ok(None);
    };
    if key != "length" {
        return Ok(None);
    }
    let Some(Value::Number(new_length)) = array_descriptor_value(descriptor, "value") else {
        return Ok(None);
    };
    let old_length = values.logical_len();
    let new_length = new_length as usize;
    if new_length >= old_length {
        return Ok(None);
    }
    if array_descriptor_flag(values, key, "writable") == Some(false) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read only array length",
        ));
    }
    let mut values = Rc::clone(values);
    let data = Rc::make_mut(&mut values);
    for index in (new_length..old_length).rev() {
        let index_key = index.to_string();
        if !array_own_index(data, &index_key, index) {
            continue;
        }
        if array_descriptor_flag(data, &index_key, "configurable") == Some(false) {
            data.set_length(index + 1);
            return Err(commit_failed_array_length(
                target,
                values,
                descriptor,
                index + 1,
            ));
        }
        data.delete_property(&index_key);
    }
    data.set_length(new_length);
    let mut result = Value::Array(values);
    store_descriptor_metadata(&mut result, key, descriptor);
    define_array_descriptor(&mut result, key, descriptor.to_vec());
    Ok(Some(result))
}

fn array_descriptor_value(descriptor: &[(String, Value)], field: &str) -> Option<Value> {
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(value.clone()))
}

fn array_own_index(data: &crate::value::ArrayData, key: &str, index: usize) -> bool {
    data.get_index(index).is_some()
        || data.descriptor(key).is_some()
        || data.property(key).is_some()
}

fn commit_failed_array_length(
    target: &Value,
    values: Rc<crate::value::ArrayData>,
    descriptor: &[(String, Value)],
    length: usize,
) -> crate::execute::VmError {
    let mut descriptor = descriptor.to_vec();
    if let Some((_, value)) = descriptor.iter_mut().find(|(name, _)| name == "value") {
        *value = Value::Number(length as f64);
    }
    let mut partial = Value::Array(values);
    store_descriptor_metadata(&mut partial, "length", &descriptor);
    define_array_descriptor(&mut partial, "length", descriptor);
    crate::locals::replace_value(target, &partial);
    crate::value::error::throw_type_error("Cannot delete non-configurable array element")
}

fn array_descriptor_flag(values: &crate::value::ArrayData, key: &str, flag: &str) -> Option<bool> {
    let Value::Object(descriptor) = values.descriptor(key)? else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == flag).then_some(matches!(value, Value::Boolean(true))))
}

fn set_array_property(mut values: Rc<crate::value::ArrayData>, key: &str, value: Value) -> Value {
    if key == "length" {
        if values.is_arguments() {
            let length = array_length_number(&value) as usize;
            Rc::make_mut(&mut values).set_length(length);
            return Value::Array(values);
        }
        let length = array_length_number(&value) as usize;
        Rc::make_mut(&mut values).set_length(length);
        return Value::Array(values);
    }
    let Some(index) = crate::arrays::array_index(key) else {
        Rc::make_mut(&mut values).set_property(key, value);
        return Value::Array(values);
    };
    let index = index as usize;
    if index
        > values
            .logical_len()
            .saturating_add(MAX_DENSE_ARRAY_INDEX_GAP)
    {
        let values = Rc::make_mut(&mut values);
        values.set_property(key, value);
        values.set_length(index.saturating_add(1));
    } else {
        Rc::make_mut(&mut values).set_index(index, value);
    }
    Value::Array(values)
}

fn array_length_number(value: &Value) -> f64 {
    crate::conversion::to_number(value).unwrap_or(f64::NAN)
}
