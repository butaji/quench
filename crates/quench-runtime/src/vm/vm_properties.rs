pub fn copy_register(registers: &mut Vec<Value>, dst: u16, src: u16) -> Result<(), VmError> {
    let value = read_register(registers, src)?;
    write_value(registers, dst, value);
    Ok(())
}
pub fn write_value(registers: &mut Vec<Value>, index: u16, value: Value) {
    let index = usize::from(index);
    if registers.len() <= index {
        registers.resize(index + 1, Value::Undefined);
    }
    registers[index] = value;
}
pub fn read_register(registers: &[Value], index: u16) -> Result<Value, VmError> {
    registers
        .get(usize::from(index))
        .cloned()
        .map(crate::locals::resolved_replacement)
        .ok_or(VmError::RegisterOutOfBounds(index))
}
pub fn get_property(value: &Value, key: &str) -> Value {
    if let Value::BindingCell(cell) = value {
        return get_property(&cell.borrow(), key);
    }
    direct_or_primitive_property(value, key)
}

fn direct_or_primitive_property(value: &Value, key: &str) -> Value {
    let direct = get_property_value(value, key);
    if !matches!(direct, Value::Undefined) {
        return direct;
    }
    primitive_prototype_property(value, key)
}
fn get_property_value(value: &Value, key: &str) -> Value {
    use Value::*;
    if let Some(found) = crate::typed_array_prototype::own_property(value, key) {
        return found;
    }
    match value {
        Builtin(builtin) if crate::intl::tolocale::symbol::name(*builtin).is_some() => {
            bind_callable_property(
                &Value::Builtin(crate::ops::Builtin::SymbolPrototype),
                crate::ops::Builtin::SymbolPrototype,
                key,
            )
        }
        Builtin(builtin) => bind_callable_property(value, *builtin, key),
        Array(values) => crate::arrays::property(values, key),
        ArrayBuffer(buffer) => array_buffer_property(buffer, key),
        _ => get_property_value_typed_tail(value, key),
    }
}

fn get_property_value_typed_tail(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Float64Array(view) => float64_array_property(view, key),
        Float32Array(view) => float32_array_property(view, key),
        Int8Array(view) => int8_array_property(view, key),
        Int16Array(view) => int16_array_property(view, key),
        Uint16Array(view) => uint16_array_property(view, key),
        Int32Array(view) => int32_array_property(view, key),
        Uint8Array(view) => uint8_array_property(view, key),
        Uint32Array(view) => uint32_array_property(view, key),
        Uint8ClampedArray(view) => uint8_clamped_array_property(view, key),
        BigInt64Array(_) | BigUint64Array(_) => vm_typed_bigint::property(value, key),
        DataView(view) => data_view_property(view, key),
        _ => get_property_value_tail(value, key),
    }
}

fn get_property_value_tail(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Object(properties) => object_property(properties, value, key),
        ObjectAlias(alias) => object_alias_property(alias, key),
        String(value) if crate::conversion::is_symbol_string(value) => Value::Undefined,
        String(value) => string_property(value, key),
        StringUnits(units) => string_units_property(units, key),
        Number(value) => number_property(*value, key),
        Boolean(value) => boolean_property(*value, key),
        _ => get_property_value_object_tail(value, key),
    }
}

fn get_property_value_object_tail(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Function(_) if matches!(key, "apply" | "call" | "bind") => {
            bind_function_property(value, key)
        }
        Function(function) => function_property(function, key),
        BoundFunction(bound) => bound_function_property(value, bound, key),
        _ => get_property_value_collection_tail(value, key),
    }
}

fn get_property_value_collection_tail(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Map(data) => map_property(data, key),
        Set(data) => set_property(data, key),
        Iterator(_) => iterator_property(value, key),
        Generator(_) => generator_property(value, key),
        _ => get_property_value_async_tail(value, key),
    }
}

fn set_property(data: &crate::value::SetData, key: &str) -> Value {
    if key == "size" && !data.weak {
        return Value::Number(data.values.borrow().len() as f64);
    }
    if data.weak {
        crate::collections::set::weak_property(key)
    } else {
        crate::collections::set::property(key)
    }
}

fn get_property_value_async_tail(value: &Value, key: &str) -> Value {
    match value {
        Promise(promise) => promise_value_property(promise, value, key),
        HostCapability(capability) => host_capability_property(value, capability.descriptor, key),
        _ => Value::Undefined,
    }
}

fn iterator_property(value: &Value, key: &str) -> Value {
    let property = crate::collections::iterator::property_for(value, key);
    iterator_property_value(value, property)
}

fn iterator_property_value(value: &Value, property: Value) -> Value {
    if matches!(
        property,
        Value::Builtin(
            Builtin::RegExpStringIteratorNext
                | Builtin::StringIteratorNext
                | Builtin::SetIteratorNext
                | Builtin::MapIteratorNext
        )
    ) {
        return property;
    }
    bind_method(value, property)
}
fn promise_value_property(promise: &crate::value::PromiseData, value: &Value, key: &str) -> Value {
    promise_property_value(promise, key).unwrap_or_else(|| promise_property(value, key))
}

fn promise_property_value(promise: &crate::value::PromiseData, key: &str) -> Option<Value> {
    promise.property(key).or_else(|| {
        promise
            .prototype()
            .map(|prototype| get_property(&prototype, key))
    })
}
fn map_property(data: &crate::value::MapData, key: &str) -> Value {
    if key == "constructor" {
        return map_constructor(data.weak);
    }
    map_collection_property(data, key)
}

fn map_constructor(weak: bool) -> Value {
    Value::Builtin(if weak { Builtin::WeakMap } else { Builtin::Map })
}

fn map_collection_property(data: &crate::value::MapData, key: &str) -> Value {
    if key == "size" && !data.weak {
        return Value::Number(data.keys.borrow().len() as f64);
    }
    if data.weak {
        crate::collections::map::weak_property(key)
    } else {
        crate::collections::map::property(key)
    }
}
/// Look up a property on the boxed prototype for a primitive value.
///
/// Returns the result when the prototype or its chain owns `key`; otherwise
/// `Undefined`. Used to surface `constructor` and prototype methods for
/// primitive values like `1`, `true`, and `null`-shaped access.
fn primitive_prototype_property(value: &Value, key: &str) -> Value {
    let Some(prototype) = primitive_prototype(value) else {
        return Value::Undefined;
    };
    if key == "constructor" {
        if let Some(builtin) = primitive_constructor(value) {
            return Value::Builtin(builtin);
        }
    }
    get_property(&prototype, key)
}

fn primitive_prototype(value: &Value) -> Option<Value> {
    Some(match value {
        Value::Number(_) => Value::Builtin(Builtin::NumberPrototype),
        Value::Boolean(_) => Value::Builtin(Builtin::BooleanPrototype),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            Value::Builtin(Builtin::StringPrototype)
        }
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            Value::Builtin(Builtin::SymbolPrototype)
        }
        Value::BigInt(_) => Value::Builtin(Builtin::BigIntPrototype),
        _ => return None,
    })
}

fn primitive_constructor(value: &Value) -> Option<Builtin> {
    match value {
        Value::Number(_) => Some(Builtin::Number),
        Value::Boolean(_) => Some(Builtin::Boolean),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            Some(Builtin::String)
        }
        Value::String(value) if crate::conversion::is_symbol_string(value) => Some(Builtin::Symbol),
        Value::BigInt(_) => Some(Builtin::BigInt),
        _ => None,
    }
}

fn generator_property(value: &Value, key: &str) -> Value {
    let builtin = match key {
        "next" => crate::ops::Builtin::GeneratorNext,
        "return" => crate::ops::Builtin::GeneratorReturn,
        "throw" => crate::ops::Builtin::GeneratorThrow,
        _ => return Value::Undefined,
    };
    bind_method(value, Value::Builtin(builtin))
}

fn function_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    if key == "constructor" {
        let builtin = function_constructor(function);
        return function_realm_intrinsic(function, builtin);
    }
    let properties = function.properties.borrow();
    if let Some((_, value)) = properties.iter().rev().find(|(name, _)| name == key) {
        return property_value(value);
    }
    function_inherited_property(function, &properties, key)
}

fn function_realm_intrinsic(
    function: &crate::value::FunctionValue,
    builtin: crate::ops::Builtin,
) -> Value {
    function
        .captures
        .get(0)
        .and_then(|global| crate::vm::realm_id_for_global_value(&global))
        .and_then(|realm| crate::vm::realm::intrinsic(realm, builtin))
        .unwrap_or(Value::Builtin(builtin))
}

fn function_inherited_property(
    function: &crate::value::FunctionValue,
    properties: &[(String, Value)],
    key: &str,
) -> Value {
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| property_value(value)))
        .map_or_else(
            || function_prototype_property(function, key),
            |prototype| get_property(&prototype, key),
        )
}
fn function_prototype_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    let prototype = if function.is_async {
        Builtin::AsyncFunctionPrototype
    } else {
        Builtin::FunctionPrototype
    };
    let value = builtin_property(prototype, key);
    value_or_object_prototype(value, key)
}

fn value_or_object_prototype(value: Value, key: &str) -> Value {
    if matches!(value, Value::Undefined) {
        return builtin_property(Builtin::ObjectPrototype, key);
    }
    value
}

fn property_value(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => property_value(&cell.borrow()),
        value => value.clone(),
    }
}

fn function_constructor(function: &crate::value::FunctionValue) -> Builtin {
    match (function.kind, function.is_async) {
        (crate::ops::FunctionKind::Generator, true) => Builtin::AsyncGeneratorFunction,
        (crate::ops::FunctionKind::Generator, false) => Builtin::GeneratorFunction,
        (_, true) => Builtin::AsyncFunction,
        (_, false) => Builtin::Function,
    }
}
pub fn get_property_result(value: &Value, key: &str) -> Result<Value, VmError> {
    get_property_with_receiver(value, key, value)
}

pub(crate) fn get_property_with_receiver(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Result<Value, VmError> {
    if let Some(result) = early_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = array_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = crate::disposable_stack::accessor(value, key, receiver) {
        return result;
    }
    if let Some(result) = data_view_instance_accessor(value, key) {
        return result;
    }
    if let Some(result) = descriptor_property_result(value, key, receiver) {
        return result;
    }
    finish_property_access(value, key, receiver)
}

fn finish_property_access(value: &Value, key: &str, receiver: &Value) -> Result<Value, VmError> {
    match crate::property_define::accessor(value, key, "get") {
        None => Ok(receiver_property(value, key, receiver)),
        Some(Value::Undefined) => Ok(Value::Undefined),
        Some(getter) => invoke_accessor(&getter, receiver),
    }
}

fn early_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Some(Err(crate::value::error::throw_type_error(&format!(
            "Cannot read property `{key}` of null or undefined"
        ))));
    }
    if matches!(value, Value::Proxy(_)) {
        return Some(crate::proxy::proxy_get(value, key, Some(receiver)));
    }
    if matches!(value, Value::Array(values) if values.is_strict_arguments() && key == "callee") {
        return Some(Err(crate::value::error::throw_type_error(
            "'callee' is unavailable on strict arguments",
        )));
    }
    if has_restricted_function_property(value, key) {
        return Some(Err(crate::value::error::throw_type_error(
            "'caller' and 'arguments' are unavailable on this function",
        )));
    }
    None
}

fn array_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    if let Some(getter) = array_accessor(value, key, "get") {
        return Some(match getter {
            Value::Undefined => Ok(Value::Undefined),
            getter => invoke_accessor(&getter, receiver),
        });
    }
    let Value::Array(values) = value else {
        return None;
    };
    if array_has_own_property(values, key) {
        return None;
    }
    crate::arrays::prototype_override_getter(key).map(|getter| match getter {
        Value::Undefined => Ok(Value::Undefined),
        getter => invoke_accessor(&getter, receiver),
    })
}

fn array_has_own_property(values: &crate::value::ArrayData, key: &str) -> bool {
    key == "length"
        || crate::arrays::array_index(key).is_some_and(|index| values.has_index(index as usize))
        || values.descriptor(key).is_some()
        || values.property(key).is_some()
}

fn descriptor_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    let Ok(descriptor) =
        crate::builtins::object::descriptor(Some(value), Some(&Value::String(key.to_string())))
    else {
        return None;
    };
    if matches!(descriptor, Value::Undefined) {
        return None;
    }
    if let Value::Object(descriptor) = descriptor {
        if let Some(getter) = descriptor
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "get").then_some(value))
        {
            return Some(match getter {
                Value::Undefined => Ok(Value::Undefined),
                getter => invoke_accessor(getter, receiver),
            });
        }
    }
    Some(Ok(receiver_property(value, key, receiver)))
}

/// Invoke a getter using the receiver as `this`. The getter's own
/// `OrdinaryCallEvaluate` semantics handle ToObject coercion for sloppy
/// functions; strict functions keep the receiver as-is.
fn invoke_accessor(getter: &Value, receiver: &Value) -> Result<Value, VmError> {
    match getter {
        Value::Function(_) | Value::BoundFunction(_) => {
            crate::functions::execute_target(getter, receiver, &[])
        }
        Value::Builtin(builtin) => {
            crate::vm::execute_builtin_with_receiver(*builtin, &[], Some(receiver))
        }
        _ => Err(crate::vm::not_callable()),
    }
}

fn receiver_property(value: &Value, key: &str, receiver: &Value) -> Value {
    let property = get_property(value, key);
    if should_preserve_receiver_property(value, key, &property)
        || same_property_receiver(value, receiver)
    {
        return property;
    }
    bind_receiver_property(property, receiver)
}

fn should_preserve_receiver_property(value: &Value, key: &str, property: &Value) -> bool {
    if object_has_property(value, key) {
        return true;
    }
    matches!(value, Value::Builtin(_))
        || matches!(value, Value::Object(_)) && crate::vm::is_global_object(value)
        || is_intl_number_format_property(property)
        || matches!(key, "constructor" | "prototype")
}

fn object_has_property(value: &Value, key: &str) -> bool {
    matches!(value, Value::Object(properties) if properties.iter().rev().any(|(name, _)| name == key))
}

fn is_intl_number_format_property(property: &Value) -> bool {
    matches!(
        property,
        Value::Builtin(
            Builtin::IntlNumberFormatFormatToParts
                | Builtin::IntlNumberFormatFormatRange
                | Builtin::IntlNumberFormatFormatRangeToParts
        )
    )
}

fn bind_receiver_property(property: Value, receiver: &Value) -> Value {
    match property {
        Value::Builtin(builtin)
            if !is_accessor_builtin(builtin)
                && !is_iterator_next_builtin(builtin)
                && crate::intl::tolocale::symbol::name(builtin).is_none() =>
        {
            bind_method(receiver, Value::Builtin(builtin))
        }
        other => other,
    }
}

fn is_iterator_next_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::RegExpStringIteratorNext
            | Builtin::StringIteratorNext
            | Builtin::SetIteratorNext
            | Builtin::MapIteratorNext
    )
}

/// Accessor getters/setters carry their `this` at invocation time; binding
/// them to the object they were read from (e.g. a property descriptor's
/// `.get`) would call them with the wrong receiver.
fn is_accessor_builtin(builtin: Builtin) -> bool {
    let name = crate::builtins::builtin_name(builtin);
    name.starts_with("get ") || name.starts_with("set ")
}
include!("vm_properties_special.rs");
use crate::value::Value::{HostCapability, Promise};
