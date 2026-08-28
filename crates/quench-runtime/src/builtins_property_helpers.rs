fn set_object_alias_property(
    alias: crate::value::ObjectAliasValue,
    key: &str,
    value: Value,
) -> Value {
    let Some(properties) = alias.0.borrow().upgrade() else {
        return Value::ObjectAlias(alias);
    };
    let previous = Rc::clone(&properties);
    let result = builtins_cells::set_object_property(properties, key, value);
    retarget_object_alias(&alias, &result);
    if let Value::Object(object) = &result {
        crate::vm::replace_realm_global_if_current(&previous, object);
    }
    result
}

fn retarget_object_alias(alias: &crate::value::ObjectAliasValue, value: &Value) {
    let Value::Object(object) = value else { return };
    *alias.0.borrow_mut() = Rc::downgrade(object);
}

fn set_prototype_slot(target: &Value, key: &str, value: Value) -> Option<Value> {
    if key != "\0prototype" {
        return None;
    }
    if let Value::Builtin(builtin) = target {
        crate::builtins::set_intrinsic_prototype_override(*builtin, value);
        return Some(target.clone());
    }
    Some(match target {
        Value::ArrayBuffer(buffer) => {
            buffer.set_prototype(value);
            Value::ArrayBuffer(buffer.clone())
        }
        Value::DataView(view) => {
            view.set_prototype(value);
            Value::DataView(view.clone())
        }
        Value::Map(data) => {
            data.set_prototype(value);
            Value::Map(data.clone())
        }
        Value::Set(data) => {
            data.set_prototype(value);
            Value::Set(data.clone())
        }
        Value::Array(data) => {
            data.set_prototype(value);
            Value::Array(data.clone())
        }
        Value::Promise(data) => {
            data.set_prototype(value);
            Value::Promise(data.clone())
        }
        other => return set_typed_array_prototype(other.clone(), value),
    })
}

fn set_typed_array_prototype(target: Value, value: Value) -> Option<Value> {
    macro_rules! set_view {
        ($variant:ident, $view:ident) => {{
            $view.set_prototype(value);
            Value::$variant($view)
        }};
    }
    Some(match target {
        Value::Float64Array(view) => set_view!(Float64Array, view),
        Value::Float32Array(view) => set_view!(Float32Array, view),
        Value::Int8Array(view) => set_view!(Int8Array, view),
        Value::Int16Array(view) => set_view!(Int16Array, view),
        Value::Int32Array(view) => set_view!(Int32Array, view),
        Value::Uint8Array(view) => set_view!(Uint8Array, view),
        Value::Uint16Array(view) => set_view!(Uint16Array, view),
        Value::Uint32Array(view) => set_view!(Uint32Array, view),
        Value::Uint8ClampedArray(view) => set_view!(Uint8ClampedArray, view),
        Value::BigInt64Array(view) => set_view!(BigInt64Array, view),
        Value::BigUint64Array(view) => set_view!(BigUint64Array, view),
        _ => return None,
    })
}
pub(crate) fn define_property(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(target) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let target = crate::vm::resolve_global_owner(target)
        .unwrap_or_else(|| crate::locals::resolved_replacement(target.clone()));
    if !crate::value::is_object(&target) {
        return Err(crate::value::error::throw_type_error(
            "Object.defineProperty target must be an object",
        ));
    }
    let key = crate::conversion::to_property_key(arguments.get(1).unwrap_or(&Value::Undefined))?;
    if matches!(target, Value::Proxy(_)) {
        let result = crate::proxy::proxy_define_property(
            &target,
            &key,
            arguments.get(2).unwrap_or(&Value::Undefined),
        )?;
        if !crate::execute::is_truthy(&result) {
            return Err(crate::value::error::throw_type_error(
                "Proxy defineProperty returned false",
            ));
        }
        return Ok(target);
    }
    let Some(descriptor) = arguments.get(2) else {
        return Ok(target.clone());
    };
    if !crate::value::is_object(descriptor) {
        return Err(crate::value::error::throw_type_error(
            "Property descriptor must be an object",
        ));
    }
    let descriptor = descriptor_fields(descriptor)?;
    let result = define_own_property(&target, &key, &descriptor)?;
    crate::locals::replace_value(&target, &result);
    // Host-side callers (module construction and capability setup) do not
    // have the opcode register file that normally publishes a global COW
    // transition.  Synchronize that one derived object representative here;
    // otherwise a hidden global property leaves subsequent global lookups on
    // an unregistered snapshot and drops context-provided bindings.
    let mut registers = crate::register_file::RegisterFile::new();
    crate::vm::synchronize_global_object(&mut registers, &target, &result);
    crate::super_scope::attach_home_objects(&result);
    Ok(result)
}
pub(crate) fn define_own_property(
    target: &Value,
    key: &str,
    descriptor: &[(String, Value)],
) -> Result<Value, crate::execute::VmError> {
    let target = crate::locals::resolved_replacement(target.clone());
    if is_process_env_object(&target) {
        let accessor = descriptor
            .iter()
            .any(|(name, _)| name == "get" || name == "set");
        let valid_data = descriptor
            .iter()
            .any(|(name, value)| name == "configurable" && matches!(value, Value::Boolean(true)))
            && descriptor
                .iter()
                .any(|(name, value)| name == "enumerable" && matches!(value, Value::Boolean(true)))
            && descriptor
                .iter()
                .any(|(name, value)| name == "writable" && matches!(value, Value::Boolean(true)));
        if accessor {
            return Err(process_env_descriptor_error(
                "'process.env' does not accept an accessor(getter/setter) descriptor",
            ));
        }
        if !valid_data {
            return Err(process_env_descriptor_error(
                "'process.env' only accepts a configurable, writable, and enumerable data descriptor",
            ));
        }
    }
    if matches!(target, Value::Proxy(_)) {
        let desc = Value::Object(Rc::new(ObjectData::new(descriptor.to_vec())));
        let result = crate::proxy::proxy_define_property(&target, key, &desc)?;
        if !crate::execute::is_truthy(&result) {
            return Err(crate::value::error::throw_type_error(
                "Proxy defineProperty returned false",
            ));
        }
        return Ok(target);
    }
    // ArraySetLength coerces its value before reading the current length
    // descriptor; the coercion may mutate the array.
    validate_array_length_descriptor(&target, key, descriptor)?;
    // ToNumber can call user code that publishes a structural array update
    // (for example, making length non-writable). Continue with that current
    // representative rather than the pre-coercion COW snapshot.
    let target = crate::locals::resolved_replacement(target);
    validate_array_index_length(&target, key)?;
    // Integer-indexed exotic objects reserve every CanonicalNumericIndexString
    // key, including invalid indices such as -0, -1, 0.1, and Infinity.
    // These keys never become ordinary properties; Reflect.defineProperty
    // converts this rejection to false while Object.defineProperty throws.
    let is_typed_array = target.typed_array_meta().is_some();
    if is_typed_array
        && crate::typed_array_ops::canonical_numeric_index(key)
        && crate::typed_array_ops::typed_array_index(key).is_none()
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot define a property on an integer-indexed exotic object",
        ));
    }
    if is_typed_array {
        if let Some(index) = crate::typed_array_ops::typed_array_index(key) {
            if crate::typed_array_prototype::is_out_of_bounds(&target)
                || crate::typed_array_ops::logical_len(&target)
                    .is_some_and(|length| index >= length)
            {
                return Err(crate::value::error::throw_type_error(
                    "Cannot define a property on an out-of-bounds typed array",
                ));
            }
            let accessor = descriptor
                .iter()
                .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
            let restricted = descriptor.iter().any(|(name, value)| {
                matches!(name.as_str(), "configurable" | "enumerable" | "writable")
                    && matches!(value, Value::Boolean(false))
            });
            if accessor || restricted {
                return Err(crate::value::error::throw_type_error(
                    "Cannot define a property on an integer-indexed exotic object",
                ));
            }
            if let Some((_, value)) = descriptor.iter().rev().find(|(name, _)| name == "value") {
                if let Some(updated) = crate::typed_array_ops::set_property(&target, key, value) {
                    return updated;
                }
            }
            return Ok(target);
        }
    }
    if let Some(result) = prepare_array_length_definition(&target, key, descriptor)? {
        return Ok(result);
    }
    let key_value = Value::String(key.to_string());
    let current = ordinary_own_descriptor(&target, key, &key_value)?;
    if matches!(current, Value::Undefined) && !crate::properties::object_is_extensible(&target) {
        return Err(crate::value::error::throw_type_error(
            "Cannot define a property on a non-extensible object",
        ));
    }
    validate_redefinition(&current, descriptor)?;
    let preserved_temporal_slot = temporal_slot_value(&target, key, &current);
    let descriptor = complete_descriptor(descriptor, &current);
    let value = descriptor
        .iter()
        .rev()
        .find(|(name, _)| name == "value")
        .map_or(Value::Undefined, |(_, value)| value.clone());
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    // Per spec 10.2.4 ([[DefineOwnProperty]] for arguments objects),
    // redefining an index with an accessor (or with writable: false)
    // disconnects the parameter mapping. Skip the placeholder write in
    // that case so the placeholder value (`Undefined`) does not flow
    // through the parameter binding.
    let is_arguments = matches!(&target, Value::Array(values) if values.is_arguments());
    let needs_disconnect = is_arguments
        && (accessor
            || descriptor
                .iter()
                .rev()
                .any(|(name, value)| name == "writable" && matches!(value, Value::Boolean(false))));
    let mut result = if accessor {
        if needs_disconnect {
            // Skip the placeholder write — the index has been, or is
            // about to be, disconnected. The descriptor is installed
            // below.
            target.clone()
        } else {
            define_accessor_placeholder(target.clone(), key)
        }
    } else if let Some(updated) = crate::typed_array_ops::set_property(&target, key, &value) {
        updated?
    } else {
        define_property_value(target.clone(), key, value)
    };
    store_descriptor_metadata(&mut result, key, &descriptor);
    if let Some(value) = preserved_temporal_slot {
        if let Value::Object(properties) = &mut result {
            Rc::make_mut(properties).push((format!("\0temporal-slot:\0{key}").into(), value));
        }
    }
    define_array_descriptor(&mut result, key, descriptor);
    Ok(result)
}

fn is_process_env_object(target: &Value) -> bool {
    let Value::Object(properties) = target else {
        return false;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0quench:process_env").then_some(value))
        == Some(Value::Boolean(true))
}

fn process_env_descriptor_error(message: &str) -> crate::execute::VmError {
    crate::execute::VmError::Thrown(crate::host_api::object(vec![
        (
            "code".into(),
            Value::String("ERR_INVALID_OBJECT_DEFINE_PROPERTY".into()),
        ),
        ("name".into(), Value::String("TypeError".into())),
        ("message".into(), Value::String(message.into())),
    ]))
}

fn temporal_slot_value(target: &Value, key: &str, current: &Value) -> Option<Value> {
    let Value::Object(object) = target else {
        return None;
    };
    let is_temporal_date = object.iter().any(|(name, value)| {
        name == "\0prototype"
            && matches!(
                value,
                Value::Builtin(
                    crate::ops::Builtin::TemporalPlainDatePrototype
                        | crate::ops::Builtin::TemporalPlainDateTimePrototype
                        | crate::ops::Builtin::TemporalZonedDateTimePrototype
                )
            )
    });
    if !is_temporal_date
        || !matches!(
            key,
            "year"
                | "month"
                | "day"
                | "hour"
                | "minute"
                | "second"
                | "millisecond"
                | "microsecond"
                | "nanosecond"
        )
    {
        return None;
    }
    match current {
        Value::Object(descriptor) => descriptor.iter().find_map(|(name, value)| {
            (name == "value" && matches!(value, Value::Number(_))).then_some(value.clone())
        }),
        _ => None,
    }
}

fn ordinary_own_descriptor(
    target: &Value,
    key: &str,
    key_value: &Value,
) -> Result<Value, crate::execute::VmError> {
    let current = crate::builtins::object::descriptor(Some(target), Some(key_value))?;
    if !matches!(current, Value::Undefined) {
        crate::execution_trace::descriptor_object("current");
        return Ok(current);
    }
    let owned = crate::builtins::object::has_own_property(Some(target), Some(key_value));
    if owned != Value::Boolean(true) {
        return Ok(Value::Undefined);
    }
    let value = crate::execute::get_property_result(target, key)?;
    Ok(Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]))))
}

fn validate_descriptor_kind(descriptor: &[(String, Value)]) -> Result<(), crate::execute::VmError> {
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    let data = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "value" | "writable"));
    if accessor && data {
        return Err(crate::value::error::throw_type_error(
            "Invalid property descriptor",
        ));
    }
    for field in ["get", "set"] {
        let Some(value) = descriptor
            .iter()
            .rev()
            .find_map(|(name, value)| (name == field).then_some(value))
        else {
            continue;
        };
        if !matches!(value, Value::Undefined) && !crate::conversion::is_callable(value) {
            return Err(crate::value::error::throw_type_error(
                "Accessor descriptor must be callable",
            ));
        }
    }
    Ok(())
}
fn store_descriptor_metadata(result: &mut Value, key: &str, descriptor: &[(String, Value)]) {
    let descriptor_key = descriptor_key(key);
    if let Value::Object(properties) = result {
        if default_ordinary_descriptor(descriptor) {
            // Avoid cloning a freshly-created cyclic object merely to remove a
            // metadata entry that is absent. `Rc::make_mut` would allocate a
            // new representative and leave weak self aliases targeting the
            // discarded one.
            if properties
                .hot_properties()
                .position_rev(&descriptor_key)
                .is_some()
            {
                Rc::make_mut(properties).retain(|(name, _)| name != &descriptor_key);
            }
            return;
        }
    }
    let metadata = Value::Object(Rc::new(ObjectData::new(descriptor.to_vec())));
    crate::execution_trace::descriptor_object("metadata");
    match result {
        Value::Object(properties) => {
            let properties = Rc::make_mut(properties);
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key.into(), metadata));
        }
        Value::Function(function) => {
            let mut properties = function.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        Value::Promise(promise) => {
            let mut properties = promise.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        Value::Builtin(builtin) => write_intrinsic_override(*builtin, key, metadata),
        Value::ArrayBuffer(buffer) => buffer.set_own_property(&descriptor_key, metadata),
        Value::DataView(view) => view.set_own_property(&descriptor_key, metadata),
        Value::BoundFunction(bound) => {
            let mut properties = bound.properties.borrow_mut();
            properties.retain(|(name, _)| name != &descriptor_key);
            properties.push((descriptor_key, metadata));
        }
        value if key != "length" && value.typed_array_meta().is_some() => {
            if let Some(meta) = value.typed_array_meta() {
                meta.set_descriptor(key, metadata);
            }
        }
        _ => {}
    }
}

fn default_ordinary_descriptor(descriptor: &[(String, Value)]) -> bool {
    if descriptor.len() != 4 {
        return false;
    }
    ["writable", "enumerable", "configurable"]
        .into_iter()
        .all(|field| {
            descriptor
                .iter()
                .rev()
                .find_map(|(name, value)| (name == field).then_some(value))
                .is_some_and(|value| matches!(value, Value::Boolean(true)))
        })
        && descriptor.iter().any(|(name, _)| name == "value")
}
fn define_accessor_placeholder(target: Value, key: &str) -> Value {
    if matches!(
        target,
        Value::Object(_)
            | Value::Function(_)
            | Value::Builtin(_)
            | Value::Promise(_)
            | Value::BoundFunction(_)
            | Value::ArrayBuffer(_)
            | Value::Array(_)
    ) {
        return define_property_value(target, key, Value::Undefined);
    }
    target
}

include!("builtins_array.rs");
include!("builtins_descriptor.rs");
include!("builtins_define_properties.rs");
