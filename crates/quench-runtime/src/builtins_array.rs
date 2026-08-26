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
    if descriptor_flag_in(properties.as_ref(), key, "configurable") == Some(false) {
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
    let configurable = descriptor_flag_in(&function.properties.borrow()[..], key, "configurable");
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
    let cell = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then_some(value))
        .and_then(|value| match value {
            Value::BindingCell(cell) => Some(Rc::clone(&cell)),
            _ => None,
        });
    let mut values: crate::value::ObjectProperties = properties
        .iter()
        .filter(|(name, _)| name != key && name != &descriptor_key(key))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    if let Some(cell) = cell {
        values.push((crate::builtins::deleted_key(key).into(), Value::BindingCell(cell)));
    }
    if crate::vm::is_global_object(&Value::Object(Rc::clone(&properties)))
        && crate::vm::global_builtin_exists(key)
    {
        values.push((crate::builtins::deleted_key(key).into(), Value::Boolean(true)));
    }
    Value::Object(Rc::new(crate::value::ObjectData::with_shared_properties(
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
        Value::BoundFunction(bound) => {
            {
                let mut properties = bound.properties.borrow_mut();
                let deleted = crate::builtins::deleted_key(key);
                properties.retain(|(name, _)| name != key && name != &deleted);
                properties.push((key.to_string(), value));
            }
            Value::BoundFunction(bound)
        }
        Value::Builtin(builtin) => {
            crate::builtins::write_intrinsic_override(builtin, key, Value::Object(Rc::new(crate::value::ObjectData::new(vec![("value".to_string(), value)]))));
            target
        }
        target => set_property(target, key, value),
    }
}

const MAX_DENSE_ARRAY_INDEX_GAP: usize = 1024;

fn define_array_descriptor(target: &mut Value, key: &str, descriptor: Vec<(String, Value)>) {
    let Value::Array(values) = target else { return };
    let mut values = Rc::clone(values);
    let mut descriptor = descriptor;
    if key == "length" {
        // Array length is always a complete data descriptor.  An empty or
        // partial defineProperty descriptor preserves the omitted attributes;
        // retaining only the supplied fields would make a writable length
        // appear non-writable to subsequent assignments.
        if !descriptor.iter().any(|(name, _)| name == "writable") {
            descriptor.push(("writable".to_string(), Value::Boolean(true)));
        }
        if !descriptor.iter().any(|(name, _)| name == "enumerable") {
            descriptor.push(("enumerable".to_string(), Value::Boolean(false)));
        }
        if !descriptor.iter().any(|(name, _)| name == "configurable") {
            descriptor.push(("configurable".to_string(), Value::Boolean(false)));
        }
    }
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    let writable = descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "writable").then_some(value));
    let ordinary_dense_index = crate::arrays::array_index(key).is_some()
        && !accessor
        && writable == Some(&Value::Boolean(true))
        && descriptor.iter().rev().find_map(|(name, value)| (name == "enumerable").then_some(value)) == Some(&Value::Boolean(true))
        && descriptor.iter().rev().find_map(|(name, value)| (name == "configurable").then_some(value)) == Some(&Value::Boolean(true));
    // A default indexed data descriptor is the dense element itself. Keeping
    // a second heap object for it duplicates a semantic fact and disables the
    // packed O(1) path on the very next write.
    if ordinary_dense_index {
        return;
    }
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
    // arguments.length is a plain value property, not an array length;
    // skip the array-length validation entirely.
    if matches!(target, Value::Array(values) if values.is_arguments()) {
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

fn validate_array_index_length(target: &Value, key: &str) -> Result<(), crate::execute::VmError> {
    let Value::Array(values) = target else {
        return Ok(());
    };
    let Some(index) = crate::arrays::array_index(key) else {
        return Ok(());
    };
    if index as usize >= values.logical_len()
        && array_descriptor_flag(values, "length", "writable") == Some(false)
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot extend array with non-writable length",
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
    if values.is_arguments() {
        return Ok(None);
    }
    let Some(value) = array_descriptor_value(descriptor, "value") else {
        return Ok(None);
    };
    // ArraySetLength performs ToUint32 and ToNumber separately. The first
    // coercion is done by validate_array_length_descriptor; repeat it here
    // for the NumberLen value used by the rest of the algorithm.
    let new_length = crate::conversion::to_number(&value)?;
    if !new_length.is_finite()
        || new_length < 0.0
        || new_length.fract() != 0.0
        || new_length > u32::MAX as f64
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    let new_length = new_length as usize;
    // Bound deletion by physically stored elements, not the logical length.
    let old_length = values.physical_len();
    if new_length >= old_length {
        // Restoration may widen a length that was temporarily made
        // non-writable by verifyWritable.  The descriptor being applied is
        // authoritative here; consulting the current (temporary) metadata
        // would leave the logical length unchanged.
        if array_descriptor_value(descriptor, "writable") != Some(Value::Boolean(false)) {
            let mut values = Rc::clone(values);
            Rc::make_mut(&mut values).set_length(new_length);
            let mut partial = Value::Array(values);
            store_descriptor_metadata(&mut partial, key, descriptor);
            define_array_descriptor(&mut partial, key, descriptor.to_vec());
            crate::locals::replace_value(target, &partial);
        }
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
    let result = descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == flag).then_some(matches!(value, Value::Boolean(true))));
    result
}

fn set_array_property(mut values: Rc<crate::value::ArrayData>, key: &str, value: Value) -> Value {
    if key == "\0prototype" {
        let data = Rc::make_mut(&mut values);
        data.set_prototype(value.clone());
        data.set_property(key, value);
        return Value::Array(values);
    }
    if key == "length" {
        if values.is_arguments() {
            // Per spec 10.6 / Annex 10.6, arguments.length's descriptor is a plain
            // value property. Keep the live override visible through every alias.
            values.set_arguments_length_override(value.clone());
            return Value::Array(values);
        }
        let length = array_length_number(&value) as usize;
        let data = Rc::make_mut(&mut values);
        data.set_length(length);
        return Value::Array(values);
    }
    let Some(index) = crate::arrays::array_index(key) else {
        Rc::make_mut(&mut values).set_property(key, value);
        return Value::Array(values);
    };
    let index = index as usize;
    let existing_number = values.set_existing_number(index, &value);
    let appended_number = !existing_number && values.append_preallocated_number(index, &value);
    if existing_number || appended_number {
        return Value::Array(values);
    }
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
