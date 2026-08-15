pub(crate) fn delete_property(target: Value, key: &str) -> (Value, bool) {
    match target {
        Value::Object(properties) if global_constant(&properties, key) => {
            (Value::Object(properties), false)
        }
        Value::Object(properties)
            if descriptor_flag_in(&properties, key, "configurable") == Some(false) =>
        {
            (Value::Object(properties), false)
        }
        Value::Object(properties) => (delete_object_property(properties, key), true),
        Value::ObjectAlias(alias) => delete_object_alias_property(alias, key),
        Value::Array(values)
            if array_descriptor_flag(&values, key, "configurable") == Some(false) =>
        {
            (Value::Array(values), false)
        }
        Value::Array(mut values) if values.is_arguments() => {
            Rc::make_mut(&mut values).delete_property(key);
            (Value::Array(values), true)
        }
        Value::Array(mut values) if key != "length" => {
            Rc::make_mut(&mut values).delete_property(key);
            (Value::Array(values), true)
        }
        Value::Array(values) => (Value::Array(values), false),
        Value::Function(function) => delete_function_property(function, key),
        Value::BoundFunction(bound) => {
            let mut properties = bound.properties.borrow_mut();
            let metadata = descriptor_key(key);
            properties.retain(|(name, _)| name != key && name != &metadata);
            (Value::BoundFunction(std::rc::Rc::clone(&bound)), true)
        }
        Value::Builtin(builtin) => {
            let deletable = crate::builtins::props::is_builtin_deletable(builtin, key);
            if deletable {
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

fn delete_object_alias_property(
    alias: crate::value::ObjectAliasValue,
    key: &str,
) -> (Value, bool) {
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
                if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
                    *current = value;
                } else {
                    properties.push((key.to_string(), value));
                }
            }
            Value::Function(function)
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
            Rc::make_mut(&mut values).set_property(key, value);
            return Value::Array(values);
        }
        let length = value_to_number(&value).max(0.0) as usize;
        Rc::make_mut(&mut values).set_length(length);
        return Value::Array(values);
    }
    let Some(index) = crate::arrays::array_index(key) else {
        Rc::make_mut(&mut values).set_property(key, value);
        return Value::Array(values);
    };
    let index = index as usize;
    if index > values.logical_len().saturating_add(MAX_DENSE_ARRAY_INDEX_GAP) {
        let values = Rc::make_mut(&mut values);
        values.set_property(key, value);
        values.set_length(index.saturating_add(1));
    } else {
        Rc::make_mut(&mut values).set_index(index, value);
    }
    Value::Array(values)
}
