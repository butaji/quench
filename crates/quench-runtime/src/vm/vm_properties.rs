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
        Array(values) => {
            let property = crate::arrays::property(values, key);
            if key == "toLocaleString" {
                crate::vm::bind_receiver_property(property, value)
            } else {
                property
            }
        }
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
            Builtin::IteratorNext
                | Builtin::RegExpStringIteratorNext
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
    let is_async = matches!(value, Value::Generator(generator) if generator.function.is_async);
    let builtin = match key {
        "next" => {
            if is_async {
                crate::ops::Builtin::AsyncGeneratorNext
            } else {
                crate::ops::Builtin::GeneratorNext
            }
        }
        "return" => {
            if is_async {
                crate::ops::Builtin::AsyncGeneratorReturn
            } else {
                crate::ops::Builtin::GeneratorReturn
            }
        }
        "throw" => {
            if is_async {
                crate::ops::Builtin::AsyncGeneratorThrow
            } else {
                crate::ops::Builtin::GeneratorThrow
            }
        }
        "toArray" => crate::ops::Builtin::IteratorToArray,
        "map" => crate::ops::Builtin::IteratorMap,
        "some" => crate::ops::Builtin::IteratorSome,
        _ => return Value::Undefined,
    };
    bind_method(value, Value::Builtin(builtin))
}

fn function_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    if key == "constructor" {
        return function_realm_intrinsic(function, function_constructor(function));
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
    let global = function.captures.get(0);
    crate::vm::realm_id_for_global_value(&global)
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
        .find_map(|(name, value)| (name == "\0function_prototype" || name == "\0prototype").then(|| property_value(value)))
        .map_or_else(
            || function_prototype_property(function, key),
            |prototype| get_property(&prototype, key),
        )
}
fn function_prototype_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    let builtin = if function.is_async {
        Builtin::AsyncFunctionPrototype
    } else {
        Builtin::FunctionPrototype
    };
    function_prototype_property_for_builtin(builtin, key)
}

pub(super) fn function_prototype_property_for_builtin(builtin: Builtin, key: &str) -> Value {
    let value = builtin_property(builtin, key);
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
include!("vm_properties_resolution.rs");
include!("vm_properties_special.rs");
use crate::value::Value::{HostCapability, Promise};

