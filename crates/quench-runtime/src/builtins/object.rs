use crate::{
    execute::VmError,
    ops::Builtin,
    value::{ObjectData, Value},
};
use std::rc::Rc;
include!("object_proxy.rs");
pub(crate) fn boxed_constructor(value: &Value) -> Builtin {
    match value {
        Value::String(value) if value.contains('\0') => Builtin::Symbol,
        Value::String(_) => Builtin::String,
        Value::Number(_) => Builtin::Number,
        Value::Boolean(_) => Builtin::Boolean,
        Value::BigInt(_) => Builtin::BigInt,
        _ => Builtin::Object,
    }
}
pub(crate) fn has_own_property(receiver: Option<&Value>, key: Option<&Value>) -> Value {
    has_own_property_result(receiver, key).unwrap_or(Value::Boolean(false))
}
include!("object_prototype.rs");
include!("object_has_own.rs");
pub(crate) fn execute_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    match builtin {
        Builtin::ObjectHasOwn => {
            let (target, key) = static_target(arguments);
            crate::builtins::object::has_own_property_static_result(target, key)
        }
        Builtin::ObjectHasOwnProperty => {
            let (target, key) = has_own_target(receiver, arguments);
            has_own_property_result(target, key)
        }
        Builtin::ObjectPropertyIsEnumerable => object_property_is_enumerable(receiver, arguments),
        Builtin::ObjectPrototypeIsPrototypeOf => is_prototype_of(receiver, arguments),
        Builtin::ObjectGetOwnPropertyDescriptor => {
            let (target, key) = static_target(arguments);
            require_object_coercible(target)?;
            if let (Some(target @ Value::Proxy(_)), Some(Value::String(key))) = (target, key) {
                return crate::proxy::proxy_get_own_property_descriptor(target, key);
            }
            descriptor(target, key)
        }
        _ => execute_special_tail(builtin, receiver, arguments),
    }
}

fn execute_special_tail(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    match builtin {
        Builtin::ObjectGetOwnPropertyDescriptors => get_own_property_descriptors(arguments),
        Builtin::ObjectGetOwnPropertyNames => object_proxy_names(arguments.first(), false),
        Builtin::ObjectGetOwnPropertySymbols => object_proxy_names(arguments.first(), true),
        Builtin::ObjectKeys => object_keys_dispatch(arguments.first()),
        Builtin::ObjectValues => object_values_entries(arguments.first(), false),
        Builtin::ObjectEntries => object_values_entries(arguments.first(), true),
        Builtin::ObjectAssign => assign(arguments),
        Builtin::ObjectFromEntries => from_entries(arguments),
        Builtin::ObjectGroupBy => group_by(arguments),
        Builtin::ObjectCreate => create(arguments),
        Builtin::ObjectGetPrototypeOf
            if arguments.is_empty()
                && receiver
                    .is_some_and(|value| !matches!(value, Value::Builtin(Builtin::Object))) =>
        {
            get_prototype_of(receiver)
        }
        Builtin::ObjectGetPrototypeOf => get_prototype_of(arguments.first()),
        Builtin::ObjectSetPrototypeOf
            if arguments.is_empty()
                && receiver
                    .is_some_and(|value| !matches!(value, Value::Builtin(Builtin::Object))) =>
        {
            Ok(Value::Undefined)
        }
        Builtin::ObjectSetPrototypeOf
            if arguments.len() == 1
                && receiver
                    .is_some_and(|value| !matches!(value, Value::Builtin(Builtin::Object))) =>
        {
            let mut call_arguments = vec![receiver.cloned().unwrap_or(Value::Undefined)];
            call_arguments.extend_from_slice(arguments);
            if matches!(receiver, Some(Value::Proxy(_))) {
                let _ = crate::proxy::proxy_set_prototype_of(
                    receiver.expect("receiver checked above"),
                    arguments.first().expect("argument checked above"),
                )?;
                Ok(Value::Undefined)
            } else {
                set_prototype_of(&call_arguments).map(|_| Value::Undefined)
            }
        }
        Builtin::ObjectSetPrototypeOf => set_prototype_of(arguments),
        _ => legacy_accessor_special(builtin, receiver, arguments),
    }
}

fn object_keys_dispatch(target: Option<&Value>) -> Result<Value, VmError> {
    let Some(target) = target else {
        return crate::own_keys::keys_result(None);
    };
    let mut resolved = target.clone();
    while let Value::BindingCell(cell) = resolved {
        resolved = cell.load();
    }
    object_keys(Some(&resolved))
}

fn get_own_property_descriptors(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or_else(|| {
        crate::value::error::throw_type_error("Object.getOwnPropertyDescriptors requires an object")
    })?;
    require_object_coercible(Some(target))?;
    let keys = if matches!(target, Value::Proxy(_)) {
        crate::proxy::proxy_own_keys(target)?
    } else {
        let names = crate::own_keys::names(Some(target))?;
        let symbols = crate::own_keys::symbols(Some(target))?;
        let mut keys = match names {
            Value::Array(names) => names.snapshot(),
            _ => Vec::new(),
        };
        if let Value::Array(symbols) = symbols {
            keys.extend(symbols.snapshot());
        }
        Value::array(keys)
    };
    let mut properties = Vec::new();
    if let Value::Array(keys) = keys {
        for key in keys.snapshot() {
            let descriptor = descriptor(Some(target), Some(&key))?;
            if !matches!(descriptor, Value::Undefined) {
                if let Value::String(key) = key {
                    properties.push((key, descriptor));
                }
            }
        }
    }
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    )))
}
fn legacy_accessor_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    Ok(match builtin {
        Builtin::ObjectPrototypeDefineGetter => define_legacy_accessor(receiver, arguments, "get")?,
        Builtin::ObjectPrototypeDefineSetter => define_legacy_accessor(receiver, arguments, "set")?,
        Builtin::ObjectPrototypeLookupGetter => lookup_legacy_accessor(receiver, arguments, "get")?,
        Builtin::ObjectPrototypeLookupSetter => lookup_legacy_accessor(receiver, arguments, "set")?,
        _ => Value::Undefined,
    })
}
fn assign(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().cloned().unwrap_or(Value::Undefined);
    crate::properties::assign_properties(target, &arguments[1..])
}
fn create(arguments: &[Value]) -> Result<Value, VmError> {
    let prototype = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(prototype, Value::Null) && !crate::value::is_object(&prototype) {
        return Err(crate::value::error::throw_type_error(
            "Object prototype must be an object or null",
        ));
    }
    let object = Value::Object(Rc::new(ObjectData::new(vec![(
        "\0prototype".to_string(),
        prototype,
    )])));
    if let Some(descriptors) = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
    {
        let coerced = crate::construct::to_object(descriptors)?;
        return crate::builtins::define_properties(&[object, coerced]);
    }
    Ok(object)
}
pub(crate) fn set_prototype_of(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(target) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Object.setPrototypeOf target must be an object",
        ));
    };
    if !crate::value::is_object(target) && !matches!(target, Value::Null | Value::Undefined) {
        let prototype = arguments.get(1).cloned().unwrap_or(Value::Undefined);
        validate_set_prototype_value(&prototype)?;
        return Ok(target.clone());
    }
    validate_set_prototype_target(target)?;
    let prototype = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    validate_set_prototype_value(&prototype)?;
    let current = get_prototype_of(Some(target))?;
    if matches!(target, Value::Builtin(Builtin::ObjectPrototype))
        && !crate::builtins::same_value(Some(&current), Some(&prototype))
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot set the prototype of an immutable prototype object",
        ));
    }
    if !crate::builtins::same_value(Some(&current), Some(&prototype))
        && !crate::properties::object_is_extensible(target)
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot set the prototype of a non-extensible object",
        ));
    }
    if !crate::builtins::same_value(Some(&current), Some(&prototype))
        && ordinary_prototype_contains(&prototype, target)?
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot create a prototype cycle",
        ));
    }
    if matches!(prototype, Value::Null) {
        let constructor = crate::vm::get_property(&current, "constructor");
        if let Value::String(name) = crate::vm::get_property(&constructor, "name") {
            let _ = crate::execute::set_property_in_place(
                target,
                "\0original_constructor_name",
                Value::String(name),
            );
        }
    }
    let result = match target {
        Value::Proxy(_) => {
            let success = crate::proxy::proxy_set_prototype_of(target, &prototype)?;
            if !crate::execute::is_truthy(&success) {
                return Err(crate::value::error::throw_type_error(
                    "Object.setPrototypeOf proxy trap returned false",
                ));
            }
            target.clone()
        }
        Value::Function(_) | Value::BoundFunction(_) => set_function_prototype(target, prototype),
        Value::Object(data) => set_object_prototype(data, prototype),
        _ => crate::builtins::set_property(target.clone(), "\0prototype", prototype),
    };
    crate::locals::replace_value(target, &result);
    crate::super_scope::attach_home_objects(&result);
    Ok(result)
}

fn ordinary_prototype_contains(prototype: &Value, target: &Value) -> Result<bool, VmError> {
    let mut current = prototype.clone();
    while !matches!(current, Value::Null) {
        if crate::builtins::same_value(Some(&current), Some(target)) {
            return Ok(true);
        }
        if matches!(current, Value::Proxy(_)) {
            return Ok(false);
        }
        current = get_prototype_of(Some(&current))?;
    }
    Ok(false)
}

fn set_object_prototype(data: &Rc<crate::value::ObjectData>, prototype: Value) -> Value {
    if let Some((_, Value::BindingCell(cell))) =
        data.iter().rev().find(|(key, _)| key == "\0prototype")
    {
        cell.store(prototype);
        return Value::Object(Rc::clone(data));
    }
    crate::builtins::set_property(Value::Object(Rc::clone(data)), "\0prototype", prototype)
}

fn set_function_prototype(target: &Value, prototype: Value) -> Value {
    let properties = match target {
        Value::Function(function) => &function.properties,
        Value::BoundFunction(function) => &function.properties,
        Value::HostCapability(capability) => &capability.properties,
        _ => return target.clone(),
    };
    let mut properties = properties.borrow_mut();
    if let Some((_, value)) = properties
        .iter_mut()
        .find(|(key, _)| key == "\0function_prototype")
    {
        if let Value::BindingCell(cell) = value {
            cell.store(prototype);
        } else {
            *value = prototype;
        }
    } else {
        properties.push(("\0function_prototype".to_string(), prototype));
    }
    target.clone()
}

pub fn original_prototype(value: &Value) -> Option<Value> {
    match value {
        Value::Object(data) => data.original_prototype(),
        _ => None,
    }
}

fn validate_set_prototype_target(target: &Value) -> Result<(), VmError> {
    if matches!(
        target,
        Value::Object(_)
            | Value::Array(_)
            | Value::ObjectAlias(_)
            | Value::ArrayBuffer(_)
            | Value::DataView(_)
            | Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
            | Value::Uint32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Map(_)
            | Value::Set(_)
            | Value::Promise(_)
            | Value::Proxy(_)
            | Value::Function(_)
            | Value::BoundFunction(_)
            | Value::HostCapability(_)
            | Value::Builtin(_)
    ) {
        return Ok(());
    }
    Err(crate::value::error::throw_type_error(
        "Object.setPrototypeOf target must be an object",
    ))
}

fn validate_set_prototype_value(prototype: &Value) -> Result<(), VmError> {
    if matches!(prototype, Value::Null)
        || crate::value::is_object(prototype)
        || matches!(prototype, Value::HostCapability(_))
    {
        return Ok(());
    }
    Err(crate::value::error::throw_type_error(
        "Object prototype must be an object or null",
    ))
}
pub(crate) fn object_property_is_enumerable(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let receiver = require_object_coercible(receiver)?;
    let Some(key) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    let key = crate::properties::dynamic_property_key(key)?;
    Ok(enumerable_value(receiver, &key))
}

fn enumerable_value(receiver: &Value, key: &str) -> Value {
    let owned = owns_property(receiver, key).unwrap_or(false);
    let enumerable = crate::builtins::descriptor_flag(receiver, key, "enumerable").unwrap_or(true);
    Value::Boolean(owned && enumerable)
}
pub(crate) fn object_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Value {
    execute_special(builtin, receiver, arguments).unwrap_or(Value::Undefined)
}
pub(crate) fn descriptor(
    value: Option<&Value>,
    key: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    let (Some(value), Some(key)) = (value, key) else {
        return Ok(Value::Undefined);
    };
    let value = crate::vm::resolve_global_owner(value)
        .unwrap_or_else(|| crate::locals::resolved_replacement(value.clone()));
    let value = unwrap_binding_cells(value);
    let key = crate::conversion::to_property_key(key)?;
    if matches!(value, Value::Proxy(_)) {
        return crate::proxy::proxy_get_own_property_descriptor(&value, &key);
    }
    crate::module_bindings::exports(&value, &key)?;
    let descriptor = descriptor_for_value(&value, &key);
    Ok(descriptor.unwrap_or(Value::Undefined))
}

fn unwrap_binding_cells(value: Value) -> Value {
    match value {
        Value::BindingCell(cell) => unwrap_binding_cells(cell.load()),
        value => value,
    }
}

fn descriptor_for_value(value: &Value, key: &str) -> Option<Value> {
    if let Value::BindingCell(cell) = value {
        return descriptor_for_value(&cell.borrow(), key);
    }
    let descriptor = match value {
        Value::Object(properties) => {
            let global = Value::Object(properties.clone());
            let deleted = properties
                .iter()
                .any(|(name, _)| name.as_str() == crate::builtins::deleted_key(key).as_str());
            if !deleted
                && crate::vm::is_global_object(&global)
                && crate::vm::global_builtin_exists(key)
            {
                let immutable = crate::globals::immutable_value(key);
                let value = immutable
                    .clone()
                    .unwrap_or_else(|| crate::execute::get_property(&global, key));
                Some(descriptor_object_with_flags(
                    value,
                    immutable.is_none(),
                    false,
                    immutable.is_none(),
                ))
            } else if is_child_realm_global(&global)
                && !matches!(key, "undefined" | "Infinity" | "NaN")
            {
                configurable_global_descriptor(&global, key)
            } else {
                object_descriptor(properties.as_ref(), key)
            }
        }
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .and_then(|properties| object_descriptor(properties.as_ref(), key)),
        Value::Array(values) => array_descriptor(values, key),
        Value::String(value) => string_descriptor(value, key),
        Value::Builtin(builtin) => builtin_descriptor(*builtin, key),
        Value::Function(function) => function_descriptor(function, key),
        Value::BoundFunction(bound) => bound_descriptor(bound, key),
        Value::ArrayBuffer(buffer) => buffer_descriptor(buffer, key),
        Value::DataView(view) => data_view_descriptor(view, key),
        Value::Float64Array(_)
        | Value::Float32Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Uint16Array(_)
        | Value::Int32Array(_)
        | Value::Uint32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Uint8Array(_)
        | Value::Uint8ClampedArray(_) => typed_array_descriptor(value, key),
        _ => None,
    };
    descriptor
}

fn is_child_realm_global(value: &Value) -> bool {
    crate::vm::is_child_global_object(value)
}

include!("object_descriptor_core.rs");
