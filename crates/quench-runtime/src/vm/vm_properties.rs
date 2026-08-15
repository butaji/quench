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
    let direct = get_property_value(value, key);
    if !matches!(direct, Value::Undefined) {
        return direct;
    }
    primitive_prototype_property(value, key)
}
fn get_property_value(value: &Value, key: &str) -> Value {
    if let Some(found) = crate::typed_array_prototype::own_property(value, key) {
        return found;
    }
    property_for_value(value, key)
}

fn property_for_value(value: &Value, key: &str) -> Value {
    use Value::*;
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
        Object(properties) => object_property(properties, value, key),
        ObjectAlias(alias) => object_alias_property(alias, value, key),
        String(value) if crate::conversion::is_symbol_string(value) => Value::Undefined,
        String(value) => string_property(value, key),
        StringUnits(units) => string_units_property(units, key),
        Number(value) => number_property(*value, key),
        Boolean(value) => boolean_property(*value, key),
        Function(_) if matches!(key, "apply" | "call" | "bind") => {
            bind_function_property(value, key)
        }
        Function(function) => function_property(function, key),
        BoundFunction(bound) => bound_function_property(value, bound, key),
        Map(data) => map_property(data, key),
        Set(data) if key == "size" => Value::Number(data.values.borrow().len() as f64),
        Set(data) if data.weak => crate::collections::set::weak_property(key),
        Set(_) => crate::collections::set::property(key),
        Iterator(_) => iterator_property(value, key),
        Generator(_) => generator_property(value, key),
        Promise(promise) => promise_value_property(promise, value, key),
        HostCapability(capability) => host_capability_property(value, capability.descriptor, key),
        _ => Value::Undefined,
    }
}


fn iterator_property(value: &Value, key: &str) -> Value {
    let property = crate::collections::iterator::property_for(value, key);
    if matches!(
        property,
        Value::Builtin(
            Builtin::IteratorNext
                | Builtin::RegExpStringIteratorNext
                | Builtin::SetIteratorNext
                | Builtin::MapIteratorNext
        )
    ) {
        return property;
    }
    bind_method(value, property)
}
fn promise_value_property(promise: &crate::value::PromiseData, value: &Value, key: &str) -> Value {
    promise
        .property(key)
        .or_else(|| {
            promise
                .prototype()
                .map(|prototype| get_property(&prototype, key))
        })
        .unwrap_or_else(|| promise_property(value, key))
}
fn map_property(data: &crate::value::MapData, key: &str) -> Value {
    if key == "constructor" {
        return Value::Builtin(if data.weak {
            Builtin::WeakMap
        } else {
            Builtin::Map
        });
    }
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
    if let Value::String(symbol) = value {
        if symbol.starts_with("Symbol.") && key == "description" {
            return Value::String(symbol.clone());
        }
    }
    let prototype = match value {
        Value::Number(_) => Some(Value::Builtin(Builtin::NumberPrototype)),
        Value::Boolean(_) => Some(Value::Builtin(Builtin::BooleanPrototype)),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            Some(Value::Builtin(Builtin::StringPrototype))
        }
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            Some(Value::Builtin(Builtin::SymbolPrototype))
        }
        Value::BigInt(_) => Some(Value::Builtin(Builtin::BigIntPrototype)),
        _ => None,
    };
    let Some(prototype) = prototype else {
        return Value::Undefined;
    };
    if key == "constructor" {
        let builtin = match value {
            Value::Number(_) => Some(Builtin::Number),
            Value::Boolean(_) => Some(Builtin::Boolean),
            Value::String(value) if !crate::conversion::is_symbol_string(value) => {
                Some(Builtin::String)
            }
            Value::String(value) if crate::conversion::is_symbol_string(value) => {
                Some(Builtin::Symbol)
            }
            Value::BigInt(_) => Some(Builtin::BigInt),
            _ => None,
        };
        if let Some(builtin) = builtin {
            return Value::Builtin(builtin);
        }
    }
    get_property(&prototype, key)
}

fn generator_property(value: &Value, key: &str) -> Value {
    let is_async = matches!(value, Value::Generator(generator) if generator.function.is_async);
    let builtin = match key {
        "next" if is_async => crate::ops::Builtin::AsyncGeneratorNext,
        "return" if is_async => crate::ops::Builtin::AsyncGeneratorReturn,
        "throw" if is_async => crate::ops::Builtin::AsyncGeneratorThrow,
        "next" => crate::ops::Builtin::GeneratorNext,
        "return" => crate::ops::Builtin::GeneratorReturn,
        "throw" => crate::ops::Builtin::GeneratorThrow,
        "toArray" => crate::ops::Builtin::IteratorToArray,
        "drop" => crate::ops::Builtin::IteratorDrop,
        "map" => crate::ops::Builtin::IteratorMap,
        "every" => crate::ops::Builtin::IteratorEvery,
        "some" => crate::ops::Builtin::IteratorSome,
        "find" => crate::ops::Builtin::IteratorFind,
        "filter" => crate::ops::Builtin::IteratorFilter,
        "take" => crate::ops::Builtin::IteratorTake,
        _ => return Value::Undefined,
    };
    bind_method(value, Value::Builtin(builtin))
}

fn function_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    if key == "constructor" {
        return Value::Builtin(function_constructor(function));
    }
    let properties = function.properties.borrow();
    if let Some((_, value)) = properties.iter().rev().find(|(name, _)| name == key) {
        return property_value(value);
    }
    let inherited = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| property_value(value)))
        .map(|prototype| get_property(&prototype, key));
    inherited.unwrap_or_else(|| function_prototype_property(key))
}
fn function_prototype_property(key: &str) -> Value {
    let value = builtin_property(Builtin::FunctionPrototype, key);
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
    if let Value::BindingCell(cell) = value {
        return get_property_with_receiver(&cell.borrow(), key, receiver);
    }
    if matches!(value, Value::Null | Value::Undefined) {
        return Some(Err(crate::value::error::throw_type_error(&format!(
            "Cannot read property `{key}` of null or undefined"
        ))));
    }
    if let Some(result) = special_property(value, key, receiver) {
        return result;
    }
    let own_descriptor = crate::builtins::object::descriptor(
        Some(value),
        Some(&Value::String(key.to_string())),
    )
    .ok();
    if key == "stack"
        && matches!(own_descriptor, Some(Value::Undefined))
        && crate::vm::has_error_slot(value)
        && (crate::properties::inherits_error_prototype(receiver)
            || is_error_subclass_receiver(receiver))
    {
        return crate::vm::execute_builtin_with_receiver(
            Builtin::ErrorPrototypeStackGetter,
            &[],
            Some(receiver),
        );
    }
    if key == "stack"
        && crate::vm::has_error_slot(value)
        && !crate::vm::has_error_slot(receiver)
    {
        return Ok(Value::Undefined);
    }
    if key == "stack"
        && matches!(own_descriptor, Some(Value::Undefined))
        && crate::vm::has_error_slot(value)
        && crate::properties::inherits_error_prototype(value)
    {
        return crate::vm::execute_builtin_with_receiver(
            Builtin::ErrorPrototypeStackGetter,
            &[],
            Some(receiver),
        );
    }
    if let Some(descriptor) = own_descriptor {
        if !matches!(descriptor, Value::Undefined) {
            if let Value::Object(descriptor) = descriptor {
                if let Some((_, getter)) = descriptor
                    .iter()
                    .rev()
                    .find_map(|(name, value)| (name == "get").then_some((name, value)))
                {
                    return if matches!(getter, Value::Undefined) {
                        Ok(Value::Undefined)
                    } else {
                        invoke_accessor(getter, receiver)
                    };
                }
            }
            return Ok(receiver_property(value, key, receiver));
        }
    }
    if key == "stack"
        && matches!(
            value,
            Value::Builtin(
                Builtin::RangeErrorPrototype
                    | Builtin::ReferenceErrorPrototype
                    | Builtin::SyntaxErrorPrototype
                    | Builtin::EvalErrorPrototype
                    | Builtin::URIErrorPrototype
                    | Builtin::AggregateErrorPrototype
                    | Builtin::TypeErrorPrototype
                    | Builtin::SuppressedErrorPrototype
            )
        )
    {
        return crate::vm::execute_builtin_with_receiver(
            Builtin::ErrorPrototypeStackGetter,
            &[],
            Some(receiver),
        );
    }
    let getter = crate::property_define::accessor(value, key, "get");
    let Some(getter) = getter else {
        return inherited_property(value, key, receiver);
    };
    let has_own = key == "length"
        || crate::arrays::array_index(key).is_some_and(|index| values.has_index(index as usize))
        || values.descriptor(key).is_some()
        || values.property(key).is_some();
    if has_own {
        return None;
    }
    crate::arrays::prototype_override_getter(key).map(|getter| match getter {
        Value::Undefined => Ok(Value::Undefined),
        getter => invoke_accessor(&getter, receiver),
    })
}

fn property_from_descriptor(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Result<Value, VmError> {
    let descriptor =
        crate::builtins::object::descriptor(Some(value), Some(&Value::String(key.to_string())))?;
    descriptor_result(&descriptor, receiver, value, key)
        .unwrap_or_else(|| accessor_property(value, key, receiver))
}

fn accessor_property(value: &Value, key: &str, receiver: &Value) -> Result<Value, VmError> {
    let getter = crate::property_define::accessor(value, key, "get");
    match getter {
        None => Ok(receiver_property(value, key, receiver)),
        Some(Value::Undefined) => Ok(Value::Undefined),
        Some(getter) => invoke_accessor(&getter, receiver),
    }
}

fn descriptor_result(
    descriptor: &Value,
    receiver: &Value,
    value: &Value,
    key: &str,
) -> Option<Result<Value, VmError>> {
    let Value::Object(descriptor) = descriptor else {
        return (!matches!(descriptor, Value::Undefined))
            .then_some(Ok(receiver_property(value, key, receiver)));
    };
    let getter = descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "get").then_some(value));
    Some(match getter {
        Some(Value::Undefined) => Ok(Value::Undefined),
        Some(getter) => invoke_accessor(getter, receiver),
        None => Ok(receiver_property(value, key, receiver)),
    })
}

fn is_error_subclass_receiver(value: &Value) -> bool {
    let Ok(prototype) = crate::builtins::object::get_prototype_of(Some(value)) else {
        return false;
    };
    let Ok(constructor) = crate::execute::get_property_result(&prototype, "constructor") else {
        return false;
    };
    let Value::Function(function) = constructor else {
        return false;
    };
    let Ok(super_constructor) = crate::construct::derived_constructor(&function) else {
        return false;
    };
    matches!(
        super_constructor,
        Value::Builtin(
            Builtin::Error
                | Builtin::RangeError
                | Builtin::ReferenceError
                | Builtin::SyntaxError
                | Builtin::EvalError
                | Builtin::URIError
                | Builtin::AggregateError
                | Builtin::TypeError
                | Builtin::SuppressedError
        )
    )
}

fn inherited_property(value: &Value, key: &str, receiver: &Value) -> Result<Value, VmError> {
    let prototype = crate::builtins::object::get_prototype_of(Some(value))?;
    if !matches!(prototype, Value::Null)
        && !crate::builtins::same_value(Some(value), Some(&prototype))
    {
        let inherited = get_property_with_receiver(&prototype, key, receiver)?;
        if !matches!(inherited, Value::Undefined) {
            return Ok(inherited);
        }
    }
    Ok(receiver_property(value, key, receiver))
}

fn special_property(value: &Value, key: &str, receiver: &Value) -> Option<Result<Value, VmError>> {
    if matches!(value, Value::Proxy(_)) {
        return Some(crate::proxy::proxy_get(value, key, Some(receiver)));
    }
    if matches!(value, Value::Array(values) if values.is_strict_arguments() && key == "callee") {
        return Some(Err(crate::value::error::throw_type_error("'callee' is unavailable on strict arguments")));
    }
    if has_restricted_function_property(value, key) {
        return Some(Err(crate::value::error::throw_type_error("'caller' and 'arguments' are unavailable on this function")));
    }
    if let Some(getter) = array_accessor(value, key, "get") {
        return Some(if matches!(getter, Value::Undefined) { Ok(Value::Undefined) } else { invoke_accessor(&getter, receiver) });
    }
    if let Some(result) = array_special(value, key, receiver) {
        return Some(result);
    }
    crate::disposable_stack::accessor(value, key, receiver).or_else(|| data_view_instance_accessor(value, key))
}

fn array_special(value: &Value, key: &str, receiver: &Value) -> Option<Result<Value, VmError>> {
    let Value::Array(values) = value else { return None };
    let has_own = key == "length"
        || crate::arrays::array_index(key).is_some_and(|index| values.has_index(index as usize))
        || values.descriptor(key).is_some()
        || values.property(key).is_some();
    if has_own { return None; }
    crate::arrays::prototype_override_getter(key).map(|getter| {
        if matches!(getter, Value::Undefined) { Ok(Value::Undefined) } else { invoke_accessor(&getter, receiver) }
    })
}

/// Invoke a getter using the receiver as `this`. The getter's own
/// `OrdinaryCallEvaluate` semantics handle ToObject coercion for sloppy
/// functions; strict functions keep the receiver as-is.
fn invoke_accessor(getter: &Value, receiver: &Value) -> Result<Value, VmError> {
    match getter {
        Value::Function(function) => crate::functions::execute(function, receiver, &[]),
        Value::BoundFunction(bound)
            if matches!(
                bound.target,
                Value::Builtin(
                    Builtin::ErrorPrototypeStackGetter | Builtin::ErrorPrototypeStackSetter
                )
            ) => crate::vm::execute_builtin_with_receiver(
                match bound.target {
                    Value::Builtin(builtin) => builtin,
                    _ => unreachable!(),
                },
                &[],
                Some(receiver),
            ),
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, &[]),
        Value::Builtin(builtin) => {
            if *builtin == Builtin::IntlNumberFormatFormat && is_number_format_receiver(receiver) {
                return Ok(bind_method(receiver, Value::Builtin(*builtin)));
            }
            crate::vm::execute_builtin_with_receiver(*builtin, &[], Some(receiver))
        }
        _ => Err(crate::vm::not_callable()),
    }
}

fn receiver_property(value: &Value, key: &str, _receiver: &Value) -> Value {
    let property = get_property(value, key);
    if let Value::Object(properties) = value {
        if properties.iter().any(|(name, _)| name == key) {
            return property;
        }
    }
    if matches!(value, Value::Builtin(_)) {
        return property;
    }
    if matches!(value, Value::Object(_)) && crate::vm::is_global_object(value) {
        return property;
    }
    if matches!(value, Value::HostCapability(_)) && key == "AbstractModuleSource" {
        return property;
    }
    if matches!(
        property,
        Value::Builtin(
            Builtin::IntlNumberFormatFormatToParts
                | Builtin::IntlNumberFormatFormatRange
                | Builtin::IntlNumberFormatFormatRangeToParts
        )
    ) {
        return property;
    }
    if matches!(key, "constructor" | "prototype") {
        return property;
    }
    property
}
fn is_iterator_next_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::RegExpStringIteratorNext | Builtin::SetIteratorNext | Builtin::MapIteratorNext
    )
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
pub(crate) fn array_accessor(value: &Value, key: &str, field: &str) -> Option<Value> {
    let Value::Array(values) = value else {
        return None;
    };
    let Value::Object(descriptor) = values.descriptor(key)? else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then(|| value.clone()))
}
fn host_capability_property(value: &Value, capability: HostCapabilityRef, key: &str) -> Value {
    if capability.kind == HostCapabilityKind::GetGlobal && key == "AbstractModuleSource" {
        return Value::Builtin(Builtin::Object);
    }
    let builtin = Builtin::HostCapability(capability.kind);
    let property = crate::builtins::property(builtin, key);
    if matches!(property, Value::Builtin(_)) {
        return bind_method(value, property);
    }
    property
}
fn bind_callable_property(value: &Value, builtin: Builtin, key: &str) -> Value {
    let property = builtin_property(builtin, key);
    if matches!(key, "prototype" | "constructor") {
        return property;
    }
    if !matches!(property, Value::Undefined) {
        return property;
    }
    if builtin != Builtin::FunctionPrototype && matches!(key, "apply" | "call" | "bind") {
        return bind_function_property(value, key);
    }
    if let Some(result) = native_error_stack_property(builtin, key) {
        return result;
    }
    if crate::builtin_meta::is_prototype(builtin) {
        return crate::builtins::property(Builtin::ObjectPrototype, key);
    }
    let inherited = crate::builtins::property(Builtin::FunctionPrototype, key);
    if !matches!(inherited, Value::Undefined) {
        return bind_method(value, inherited);
    }
    bind_method(
        value,
        crate::builtins::property(Builtin::ObjectPrototype, key),
    )
}

include!("vm_properties_tail.rs");
