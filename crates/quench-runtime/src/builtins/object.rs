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
pub(crate) fn execute_special(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    match builtin {
        Builtin::ObjectHasOwnProperty => {
            let (target, key) = has_own_target(receiver, arguments);
            has_own_property_result(target, key)
        }
        Builtin::ObjectPropertyIsEnumerable => {
            Ok(object_property_is_enumerable(receiver, arguments))
        }
        Builtin::ObjectPrototypeIsPrototypeOf => is_prototype_of(receiver, arguments),
        Builtin::ObjectPrototypeDefineGetter => define_legacy_accessor(receiver, arguments, "get"),
        Builtin::ObjectPrototypeDefineSetter => define_legacy_accessor(receiver, arguments, "set"),
        Builtin::ObjectPrototypeLookupGetter => lookup_legacy_accessor(receiver, arguments, "get"),
        Builtin::ObjectPrototypeLookupSetter => lookup_legacy_accessor(receiver, arguments, "set"),
        Builtin::ObjectGetOwnPropertyDescriptor => {
            let (target, key) = static_target(arguments);
            require_object_coercible(target)?;
            if let (Some(target @ Value::Proxy(_)), Some(Value::String(key))) = (target, key) {
                return crate::proxy::proxy_get_own_property_descriptor(target, key);
            }
            descriptor(target, key)
        }
        Builtin::ObjectGetOwnPropertyNames => object_proxy_names(arguments.first(), false),
        Builtin::ObjectGetOwnPropertySymbols => object_proxy_names(arguments.first(), true),
        Builtin::ObjectKeys => object_keys(arguments.first()),
        Builtin::ObjectAssign => assign(arguments),
        Builtin::ObjectFromEntries => from_entries(arguments),
        Builtin::ObjectGroupBy => group_by(arguments),
        Builtin::ObjectCreate => create(arguments),
        Builtin::ObjectSetPrototypeOf => set_prototype_of(arguments),
        _ => Ok(Value::Undefined),
    }
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
    if !matches!(
        target,
        Value::Object(_)
            | Value::Array(_)
            | Value::ObjectAlias(_)
            | Value::Function(_)
            | Value::BoundFunction(_)
            | Value::Builtin(_)
    ) {
        return Err(crate::value::error::throw_type_error(
            "Object.setPrototypeOf target must be an object",
        ));
    }
    let prototype = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if !matches!(
        prototype,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Builtin(_) | Value::Null
    ) {
        return Err(crate::value::error::throw_type_error(
            "Object prototype must be an object or null",
        ));
    }
    let result = crate::builtins::set_property(target.clone(), "\0prototype", prototype);
    crate::super_scope::attach_home_objects(&result);
    Ok(result)
}
fn has_own_target<'a>(
    receiver: Option<&'a Value>,
    arguments: &'a [Value],
) -> (Option<&'a Value>, Option<&'a Value>) {
    if receiver.is_none() {
        return static_target(arguments);
    }
    (receiver, arguments.first())
}
fn static_target(arguments: &[Value]) -> (Option<&Value>, Option<&Value>) {
    (arguments.first(), arguments.get(1))
}
fn has_own_property_result(
    receiver: Option<&Value>,
    key: Option<&Value>,
) -> Result<Value, VmError> {
    let Some(key) = key else {
        return Ok(Value::Boolean(false));
    };
    let key = crate::properties::dynamic_property_key(key)?;
    let receiver = require_object_coercible(receiver)?;
    Ok(Value::Boolean(owns_property(receiver, &key)?))
}
fn require_object_coercible(receiver: Option<&Value>) -> Result<&Value, VmError> {
    match receiver {
        Some(Value::Null) | Some(Value::Undefined) | None => {
            Err(VmError::Thrown(crate::builtins::error(
                Builtin::TypeError,
                &[Value::String(
                    "Cannot convert undefined or null to object".to_string(),
                )],
            )))
        }
        Some(value) => Ok(value),
    }
}
fn owns_property(receiver: &Value, key: &str) -> Result<bool, VmError> {
    Ok(match receiver {
        Value::Object(properties) => object_data_owns(properties, key),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .is_some_and(|properties| object_owns(&properties, key)),
        Value::Array(values) => array_owns(values, key),
        Value::String(value) => {
            key == "length" || valid_index(key, crate::strings::utf16_len(value))
        }
        Value::Builtin(builtin) => builtin_owns_property(*builtin, key),
        Value::Function(function) => function
            .properties
            .borrow()
            .iter()
            .rev()
            .any(|(name, _)| name == key),
        Value::Proxy(_) => {
            crate::proxy::proxy_get_own_property_descriptor(receiver, key)? != Value::Undefined
        }
        _ => false,
    })
}

fn object_data_owns(properties: &Rc<ObjectData>, key: &str) -> bool {
    let deleted = properties
        .iter()
        .any(|(name, _)| name == &crate::builtins::deleted_key(key));
    properties
        .iter()
        .any(|(name, _)| name == key && !super::is_descriptor_key(name))
        || (!deleted
            && crate::vm::is_global_object(&Value::Object(properties.clone()))
            && crate::vm::global_builtin_exists(key))
}

fn array_owns(values: &crate::value::ArrayData, key: &str) -> bool {
    (!values.is_arguments() && key == "length")
        || key
            .parse::<usize>()
            .is_ok_and(|index| values.has_index(index))
        || values.property(key).is_some()
        || (values.is_strict_arguments() && key == "callee")
}

fn object_owns(properties: &Rc<ObjectData>, key: &str) -> bool {
    let deleted = properties
        .iter()
        .any(|(name, _)| name == &crate::builtins::deleted_key(key));
    properties
        .iter()
        .any(|(name, _)| name == key && !super::is_descriptor_key(name))
        || (!deleted
            && crate::vm::is_global_object(&Value::Object(properties.clone()))
            && crate::vm::global_builtin_exists(key))
}
fn builtin_owns_property(builtin: Builtin, key: &str) -> bool {
    if crate::builtins::builtin_prototype_property_is_removed(builtin, key) {
        return false;
    }
    (builtin == Builtin::Object && key == "hasOwn")
        || super::callable_property(builtin, key).is_some()
        || super::special_property(builtin, key).is_some()
}
fn valid_index(key: &str, len: usize) -> bool {
    key.parse::<usize>().is_ok_and(|index| index < len)
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
    let key = crate::conversion::to_property_key(key)?;
    let descriptor = match value {
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
        _ => None,
    };
    Ok(descriptor.unwrap_or(Value::Undefined))
}
fn function_descriptor(function: &crate::value::FunctionValue, key: &str) -> Option<Value> {
    if let Some((_, metadata)) = function
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == &super::descriptor_key(key))
    {
        return Some(public_descriptor(metadata));
    }
    let value = function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then(|| value.clone()))?;
    if key == "prototype" {
        return Some(descriptor_object_with_flags(value, false, false, false));
    }
    Some(descriptor_object(&value))
}
fn object_descriptor(properties: &[(String, Value)], key: &str) -> Option<Value> {
    if let Some((_, metadata)) = properties
        .iter()
        .rev()
        .find(|(name, _)| name == &super::descriptor_key(key))
    {
        return Some(public_descriptor(metadata));
    }
    properties
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| {
            if matches!(value, Value::Builtin(_)) && crate::vm::global_builtin_exists(key) {
                descriptor_object_with_flags(value.clone(), true, false, true)
            } else {
                descriptor_object(value)
            }
        })
}
fn intrinsic_accessor(builtin: Builtin, key: &str) -> Option<Value> {
    let getter = match (builtin, key) {
        (Builtin::Set, "Symbol.species") => Builtin::SetSpeciesGetter,
        (Builtin::Map, "Symbol.species") => Builtin::MapSpeciesGetter,
        (Builtin::SetPrototype, "size") => Builtin::SetSizeGetter,
        (Builtin::MapPrototype, "size") => Builtin::MapSizeGetter,
        _ => return None,
    };
    Some(accessor_descriptor(getter))
}

fn builtin_descriptor(builtin: Builtin, key: &str) -> Option<Value> {
    if let Some(descriptor) = intrinsic_accessor(builtin, key) {
        return Some(descriptor);
    }
    if builtin == Builtin::SymbolPrototype && key == "description" {
        return Some(accessor_descriptor(Builtin::SymbolDescriptionGetter));
    }
    if builtin == Builtin::Symbol && key == "unscopables" {
        return super::special_property(builtin, key)
            .map(|property| descriptor_object_with_flags(property, false, false, false));
    }
    if let Some(descriptor) = crate::builtins::read_intrinsic_override(builtin, key) {
        return Some(public_descriptor(&descriptor));
    }
    let property = super::callable_property(builtin, key)
        .or_else(|| super::special_property(builtin, key))
        .or_else(|| match super::property(builtin, key) {
            Value::Undefined => None,
            value => Some(value),
        })?;
    let writable = !matches!(
        key,
        "length" | "name" | "prototype" | "Symbol.toStringTag" | "unscopables"
    ) && !is_well_known_symbol_property(builtin, key)
        && builtin_property_writable(builtin, key);
    let configurable = !matches!(key, "prototype" | "unscopables")
        && !is_well_known_symbol_property(builtin, key)
        && !crate::conversion::is_symbol(&property);
    Some(descriptor_object_with_flags(
        property,
        writable,
        false,
        configurable,
    ))
}
fn accessor_descriptor(getter: Builtin) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("get".to_string(), Value::Builtin(getter)),
        ("set".to_string(), Value::Undefined),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}
include!("object_property_flags.rs");
include!("object_array_descriptor.rs");
fn string_descriptor(value: &str, key: &str) -> Option<Value> {
    crate::strings::char_at_utf16(value, key.parse::<usize>().ok()?)
        .map(|character| descriptor_object_with_flags(Value::String(character), false, true, false))
}
fn descriptor_object(value: &Value) -> Value {
    descriptor_object_with_flags(public_value(value), true, true, true)
}
fn public_descriptor(descriptor: &Value) -> Value {
    let Value::Object(properties) = descriptor else {
        return descriptor.clone();
    };
    let mut properties = properties.properties.clone();
    if let Some((_, value)) = properties.iter_mut().find(|(name, _)| name == "value") {
        *value = public_value(value);
    }
    Value::Object(Rc::new(ObjectData::new(properties)))
}
fn public_value(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => public_value(&cell.borrow()),
        value => value.clone(),
    }
}
fn descriptor_object_with_flags(
    value: Value,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Value {
    let value = public_value(&value);
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(enumerable)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ])))
}
#[cfg(test)]
mod tests {
    use super::{descriptor, execute_special};
    use crate::{
        execute::{execute_builtin_with_receiver, VmError},
        ops::Builtin,
        value::{FunctionValue, ObjectData, Value},
    };
    use std::{cell::RefCell, rc::Rc};
    #[test]
    fn binding_cell_property_mutates_without_escaping() {
        let cell = Rc::new(RefCell::new(Value::Number(1.0)));
        let binding = Value::BindingCell(Rc::clone(&cell));
        let metadata = Value::Object(Rc::new(ObjectData::new(vec![
            ("value".to_string(), binding.clone()),
            ("writable".to_string(), Value::Boolean(true)),
        ])));
        let object = Value::Object(Rc::new(ObjectData::new(vec![
            ("x".to_string(), binding),
            (crate::builtins::descriptor_key("x"), metadata),
        ])));
        let updated = crate::builtins::set_property(object.clone(), "x", Value::Number(2.0));
        assert!(
            matches!((&object, &updated), (Value::Object(a), Value::Object(b)) if Rc::ptr_eq(a, b))
        );
        assert_eq!(*cell.borrow(), Value::Number(2.0));
        let result = descriptor(Some(&updated), Some(&Value::String("x".to_string())));
        assert_eq!(
            crate::execute::get_property(&result.unwrap(), "value"),
            Value::Number(2.0)
        );
    }
    #[test]
    fn static_has_own_uses_first_argument_as_target() {
        let result = execute_special(
            Builtin::ObjectHasOwnProperty,
            None,
            &[
                Value::Builtin(Builtin::Object),
                Value::String("hasOwn".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }
    #[test]
    fn has_own_throws_on_nullish_target() {
        let error = execute_builtin_with_receiver(
            Builtin::ObjectHasOwnProperty,
            &[Value::Null, Value::String("x".to_string())],
            None,
        )
        .unwrap_err();
        assert!(matches!(error, VmError::Thrown(_)));
    }
    include!("object_tests.rs");
}
