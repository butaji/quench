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
        if matches!(bound.target, Value::Builtin(_)) && matches!(key, "length" | "name")
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
    if descriptor_flag_in(&properties, key, "configurable") == Some(false) {
        return (Value::Object(properties), false);
    }
    if crate::vm::is_global_object(&Value::Object(properties.clone()))
        && !matches!(key, "undefined" | "Infinity" | "NaN")
    {
        return (delete_object_property(properties, key), true);
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
    let cell = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then_some(value))
        .and_then(|value| match value {
            Value::BindingCell(cell) => Some(Rc::clone(cell)),
            _ => None,
        });
    let mut values: crate::value::ObjectProperties = properties
        .iter()
        .filter(|(name, _)| name != key && name != &descriptor_key(key))
        .cloned()
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
            let mut fields = vec![("value".to_string(), value)];
            if builtin == crate::ops::Builtin::ArrayPrototype && key == "length" {
                fields.extend([
                    ("writable".to_string(), Value::Boolean(true)),
                    ("enumerable".to_string(), Value::Boolean(false)),
                    ("configurable".to_string(), Value::Boolean(false)),
                ]);
            }
            crate::builtins::write_intrinsic_override(
                builtin,
                key,
                Value::Object(Rc::new(crate::value::ObjectData::new(fields))),
            );
            if builtin == crate::ops::Builtin::ArrayPrototype {
                if let Some(index) = crate::arrays::array_index(key) {
                    let length = crate::builtins::read_descriptor_value(builtin, "length")
                        .or_else(|| crate::builtins::special_property(builtin, "length"))
                        .and_then(|value| match value {
                            Value::Number(length) => Some(length as u32),
                            _ => None,
                        })
                        .unwrap_or(0);
                    if index >= length {
                        crate::builtins::write_intrinsic_override(
                            builtin,
                            "length",
                            Value::Object(Rc::new(crate::value::ObjectData::new(vec![
                                ("value".to_string(), Value::Number(f64::from(index) + 1.0)),
                                ("writable".to_string(), Value::Boolean(true)),
                                ("enumerable".to_string(), Value::Boolean(false)),
                                ("configurable".to_string(), Value::Boolean(false)),
                            ]))),
                        );
                    }
                }
            }
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
        && values.descriptor(key).is_none()
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
    // The arguments exotic object's `length` is an ordinary writable data
    // property, not ArraySetLength. Preserve its value without applying
    // array-length coercion (which would incorrectly throw for strings).
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
    // The current descriptor is read only after both coercions.  A coercion
    // may have changed it, so validate against that post-coercion descriptor
    // before mutating the array or committing the new length.
    let current =
        ordinary_own_descriptor(target, key, &Value::String(key.to_string()))?;
    validate_redefinition(&current, descriptor)?;
    let mut completed = complete_descriptor(descriptor, &current);
    if let Some((_, value)) = completed.iter_mut().find(|(name, _)| name == "value") {
        *value = Value::Number(new_length as f64);
    }
    // Bound deletion by physically stored elements, not the logical length.
    let old_length = values.physical_len();
    if new_length >= old_length {
        // Restoration may widen a length that was temporarily made
        // non-writable by verifyWritable.  The descriptor being applied is
        // authoritative here; consulting the current (temporary) metadata
        // would leave the logical length unchanged.
        let mut values = Rc::clone(values);
        Rc::make_mut(&mut values).set_length(new_length);
        let mut result = Value::Array(values);
        store_descriptor_metadata(&mut result, key, &completed);
        define_array_descriptor(&mut result, key, completed);
        return Ok(Some(result));
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
    store_descriptor_metadata(&mut result, key, &completed);
    define_array_descriptor(&mut result, key, completed);
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
            Rc::make_mut(&mut values).set_arguments_length_override(value.clone());
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
    if values.is_arguments() && index >= values.logical_len() {
        Rc::make_mut(&mut values).set_property(key, value);
        return Value::Array(values);
    }
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

pub(crate) fn array_with(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| {
        !matches!(value, Value::Null | Value::Undefined)
    }) else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.with called on null or undefined",
        ));
    };
    let length = crate::builtins::map_length(receiver)?;
    if length >= 1usize << 32 {
        return Err(crate::value::error::throw_range_error("Invalid array length"));
    }
    let number = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let integer = if number.is_nan() { 0.0 } else { number.trunc() };
    let index = if integer < 0.0 {
        (length as f64 + integer) as isize
    } else {
        integer as isize
    };
    if index < 0 || index as usize >= length {
        return Err(crate::value::error::throw_range_error("Invalid index"));
    }
    let mut result = Vec::with_capacity(length);
    for current in 0..length {
        if current == index as usize {
            result.push(arguments.get(1).cloned().unwrap_or(Value::Undefined));
        } else {
            result.push(crate::execute::get_property_result(
                receiver,
                &current.to_string(),
            )?);
        }
    }
    Ok(Value::array(result))
}

pub(crate) fn array_to_spliced(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| {
        !matches!(value, Value::Null | Value::Undefined)
    }) else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.toSpliced called on null or undefined",
        ));
    };
    let length = crate::builtins::map_length(receiver)?;
    let start_number = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let start_number = if start_number.is_nan() {
        0.0
    } else {
        start_number.trunc()
    };
    let start = if start_number < 0.0 {
        (length as f64 + start_number).max(0.0) as usize
    } else {
        (start_number as usize).min(length)
    };
    // With no arguments, the method returns a shallow copy.  A present start
    // with an omitted deleteCount instead deletes through the end.
    let delete_count = if arguments.is_empty() {
        0
    } else if arguments.len() < 2 {
        length - start
    } else {
        arguments
            .get(1)
            .map(crate::conversion::to_number)
            .transpose()?
            .unwrap_or(0.0)
            .max(0.0)
            .trunc() as usize
    }
    .min(length - start);
    let insert_count = arguments.len().saturating_sub(2);
    let new_length = length
        .saturating_sub(delete_count)
        .checked_add(insert_count)
        .ok_or_else(|| crate::value::error::throw_type_error("Array length exceeds limit"))?;
    if new_length > 9_007_199_254_740_991 {
        return Err(crate::value::error::throw_type_error(
            "Array length exceeds limit",
        ));
    }
    if new_length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    let mut result = Vec::with_capacity(new_length);
    for index in 0..start {
        result.push(crate::execute::get_property_result(
            receiver,
            &index.to_string(),
        )?);
    }
    result.extend(arguments.iter().skip(2).cloned());
    for index in start + delete_count..length {
        result.push(crate::execute::get_property_result(
            receiver,
            &index.to_string(),
        )?);
    }
    Ok(Value::array(result))
}
