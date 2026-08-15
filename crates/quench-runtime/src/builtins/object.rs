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
            has_own_property_result(target, key)
        }
        Builtin::ObjectHasOwnProperty => {
            let (target, key) = has_own_target(receiver, arguments);
            has_own_property_result(target, key)
        }
        Builtin::ObjectPropertyIsEnumerable => {
            Ok(object_property_is_enumerable(receiver, arguments))
        }
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
        Builtin::ObjectKeys => object_keys(arguments.first()),
        Builtin::ObjectValues => object_values_entries(arguments.first(), false),
        Builtin::ObjectEntries => object_values_entries(arguments.first(), true),
        Builtin::ObjectAssign => assign(arguments),
        Builtin::ObjectFromEntries => from_entries(arguments),
        Builtin::ObjectGroupBy => group_by(arguments),
        Builtin::ObjectCreate => create(arguments),
        Builtin::ObjectSetPrototypeOf => set_prototype_of(arguments),
        _ => legacy_accessor_special(builtin, receiver, arguments),
    }
}

fn get_own_property_descriptors(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or_else(|| {
        crate::value::error::throw_type_error("Object.getOwnPropertyDescriptors requires an object")
    })?;
    require_object_coercible(Some(target))?;
    let names = crate::own_keys::names(Some(target))?;
    let symbols = crate::own_keys::symbols(Some(target))?;
    let mut properties = Vec::new();
    for keys in [names, symbols] {
        let Value::Array(keys) = keys else { continue };
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
    Ok(Value::Object(Rc::new(ObjectData::new(vec![(
        "\0prototype".to_string(),
        prototype,
    )]))))
}
pub(crate) fn set_prototype_of(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(target) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Object.setPrototypeOf target must be an object",
        ));
    };
    validate_set_prototype_target(target)?;
    let prototype = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    validate_set_prototype_value(&prototype)?;
    let current = get_prototype_of(Some(target))?;
    if !crate::builtins::same_value(Some(&current), Some(&prototype))
        && !crate::properties::object_is_extensible(target)
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot set the prototype of a non-extensible object",
        ));
    }
    let result = crate::builtins::set_property(target.clone(), "\0prototype", prototype);
    crate::super_scope::attach_home_objects(&result);
    Ok(result)
}

fn validate_set_prototype_target(target: &Value) -> Result<(), VmError> {
    if matches!(
        target,
        Value::Object(_)
            | Value::Array(_)
            | Value::ObjectAlias(_)
            | Value::Function(_)
            | Value::BoundFunction(_)
            | Value::Builtin(_)
    ) {
        return Ok(());
    }
    Err(crate::value::error::throw_type_error(
        "Object.setPrototypeOf target must be an object",
    ))
}

fn validate_set_prototype_value(prototype: &Value) -> Result<(), VmError> {
    if matches!(
        prototype,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Builtin(_) | Value::Null
    ) {
        return Ok(());
    }
    Err(crate::value::error::throw_type_error(
        "Object prototype must be an object or null",
    ))
}
pub(crate) fn object_property_is_enumerable(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Value {
    if matches!(receiver, Some(Value::Builtin(_))) {
        return Value::Boolean(false);
    }
    let (Some(receiver), Some(key)) = (receiver, arguments.first()) else {
        return Value::Boolean(false);
    };
    let Ok(key) = crate::properties::dynamic_property_key(key) else {
        return Value::Boolean(false);
    };
    let owned = owns_property(receiver, &key).unwrap_or(false);
    let enumerable = crate::builtins::descriptor_flag(receiver, &key, "enumerable").unwrap_or(true);
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
    let value = crate::locals::resolved_replacement(value.clone());
    let key = crate::conversion::to_property_key(key)?;
    let descriptor = match &value {
        Value::Object(properties) => {
            let global = Value::Object(properties.clone());
            let deleted = properties
                .iter()
                .any(|(name, _)| name == &crate::builtins::deleted_key(&key));
            if !deleted
                && crate::vm::is_global_object(&global)
                && crate::vm::global_builtin_exists(&key)
            {
                let value = crate::execute::get_property(&global, &key);
                Some(descriptor_object_with_flags(value, true, false, true))
            } else if is_child_realm_global(&global)
                && !matches!(key.as_str(), "undefined" | "Infinity" | "NaN")
            {
                configurable_global_descriptor(&global, &key)
            } else {
                object_descriptor(properties, &key)
            }
        }
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .and_then(|properties| object_descriptor(&properties, &key)),
        Value::Array(values) => array_descriptor(values, &key),
        Value::String(value) => string_descriptor(value, &key),
        Value::Builtin(builtin) => builtin_descriptor(*builtin, &key),
        Value::Function(function) => function_descriptor(function, &key),
        Value::BoundFunction(bound) => bound_descriptor(bound, &key),
        Value::ArrayBuffer(buffer) => buffer_descriptor(buffer, &key),
        Value::DataView(view) => data_view_descriptor(view, &key),
        _ => None,
    };
    Ok(descriptor.unwrap_or(Value::Undefined))
}

fn is_child_realm_global(value: &Value) -> bool {
    crate::vm::is_child_global_object(value)
        || matches!(value, Value::Object(properties) if properties.iter().any(|(key, _)| key == "\0realm"))
}

include!("object_descriptor_core.rs");
