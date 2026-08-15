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
        ObjectAlias(alias) => object_alias_property(alias, key, value),
        String(value) if crate::conversion::is_symbol_string(value) => Value::Undefined,
        String(value) => string_property(value, key),
        StringUnits(units) => string_units_property(units, key),
        Number(value) => number_property(*value, key),
        Boolean(value) => boolean_property(*value, key),
        Function(function)
            if function.realm == crate::ops::RealmId::ROOT
                && matches!(key, "apply" | "call" | "bind") =>
        {
            function_prototype_method(function.realm, key)
        }
        Function(_) if matches!(key, "apply" | "call" | "bind") => bind_function_property(value, key),
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

fn function_prototype_method(realm: crate::ops::RealmId, key: &str) -> Value {
    let builtin = match key {
        "apply" => Builtin::FunctionApply,
        "call" => Builtin::FunctionCall,
        "bind" => Builtin::FunctionBind,
        _ => return Value::Undefined,
    };
    crate::vm::intrinsic_for_realm(realm, builtin)
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
        _ => return Value::Undefined,
    };
    bind_method(value, Value::Builtin(builtin))
}

fn function_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    if key == "constructor" {
        let builtin = function_constructor(function);
        return crate::vm::intrinsic_for_global(&function.captures.get(0), builtin)
            .unwrap_or(Value::Builtin(builtin));
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
    if let Some(result) = early_property_access(value, key, receiver) {
        return result;
    }
    property_from_descriptor(value, key, receiver)
}

fn early_property_access(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Some(Err(crate::value::error::throw_type_error(&format!(
            "Cannot read property `{key}` of null or undefined"
        ))));
    }
    if crate::builtins::namespace_uninitialized(value, key) {
        return Err(crate::value::error::throw_reference_error(
            "Cannot access an uninitialized module binding",
        ));
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
    if let Some(result) = array_early_access(value, key, receiver) {
        return Some(result);
    }
    if let Some(result) = crate::disposable_stack::accessor(value, key, receiver) {
        return Some(result);
    }
    if let Some(result) = data_view_instance_accessor(value, key) {
        return Some(result);
    }
    None
}

fn array_early_access(
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
            crate::vm::execute_builtin_with_receiver(*builtin, &[], Some(receiver))
        }
        _ => Err(crate::vm::not_callable()),
    }
}

fn receiver_property(value: &Value, key: &str, _receiver: &Value) -> Value {
    let property = get_property(value, key);
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
/// Accessor getters/setters carry their `this` at invocation time; binding
/// them to the object they were read from (e.g. a property descriptor's
/// `.get`) would call them with the wrong receiver.
fn is_accessor_builtin(builtin: Builtin) -> bool {
    if builtin == Builtin::ThrowTypeError {
        return true;
    }
    let name = crate::builtins::builtin_name(builtin);
    name.starts_with("get ") || name.starts_with("set ")
}
fn same_property_receiver(value: &Value, receiver: &Value) -> bool {
    match (value, receiver) {
        (Value::Builtin(left), Value::Builtin(right)) => left == right,
        (Value::Object(left), Value::Object(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Map(left), Value::Map(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Set(left), Value::Set(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Array(left), Value::Array(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Number(_), Value::Number(_))
        | (Value::Boolean(_), Value::Boolean(_))
        | (Value::BigInt(_), Value::BigInt(_))
        | (Value::String(_), Value::String(_))
        | (Value::StringUnits(_), Value::StringUnits(_)) => value == receiver,
        _ => false,
    }
}

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
    if let Value::HostCapability(value) = value {
        if let Some(property) = value.property(key) {
            return property;
        }
    }
    if capability.kind == crate::ops::HostCapabilityKind::GetGlobal && key == "agent" {
        let Value::HostCapability(parent) = value else {
            return Value::Undefined;
        };
        return Value::HostCapability(Rc::new(parent.child(HostCapabilityRef {
            realm: capability.realm,
            kind: crate::ops::HostCapabilityKind::Agent,
        })));
    }
    if capability.kind == crate::ops::HostCapabilityKind::Agent && key == "timeouts" {
        return Value::object(vec![
            ("yield".into(), Value::Number(1.0)),
            ("small".into(), Value::Number(100.0)),
            ("long".into(), Value::Number(1_000.0)),
            ("huge".into(), Value::Number(10_000.0)),
        ]);
    }
    if capability.kind == crate::ops::HostCapabilityKind::GetGlobal && key == "AbstractModuleSource"
    {
        return Value::Builtin(Builtin::AbstractModuleSource);
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
        if key == "constructor"
            && matches!(property, Value::Undefined)
            && crate::builtin_meta::constructor_name(builtin).is_some()
        {
            return Value::Builtin(Builtin::Function);
        }
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
fn bind_function_property(value: &Value, key: &str) -> Value {
    let builtin = function_builtin_property(key);
    bind_method(value, Value::Builtin(builtin))
}

fn function_builtin_property(key: &str) -> Builtin {
    match key {
        "apply" => Builtin::FunctionApply,
        "call" => Builtin::FunctionCall,
        "bind" => Builtin::FunctionBind,
        _ => Builtin::FunctionCall,
    }
}
fn bound_function_property(
    value: &Value,
    bound: &crate::value::BoundFunctionValue,
    key: &str,
) -> Value {
    if let Some((_, value)) = bound
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == key)
    {
        return value.clone();
    }
    match key {
        "apply" | "call" | "bind" => bound_method(value, bound, key),
        "length" => bound_length(bound),
        "name" => bound_name(bound),
        "prototype" => bound_prototype(bound),
        _ => bound_other_property(bound, key),
    }
}

fn bound_method(value: &Value, bound: &crate::value::BoundFunctionValue, key: &str) -> Value {
    if realm::is_intrinsic(bound)
        && matches!(bound.target, Value::Builtin(Builtin::FunctionPrototype))
    {
        return crate::vm::intrinsic_for_realm(bound.realm, function_builtin_property(key));
    }
    bind_function_property(value, key)
}

fn bound_length(bound: &crate::value::BoundFunctionValue) -> Value {
    if realm::is_intrinsic(bound) {
        return Value::Undefined;
    }
    match &bound.target {
        Value::Builtin(builtin) => {
            crate::builtins::props::callable(*builtin, "length").unwrap_or(Value::Number(0.0))
        }
        target => get_property(target, "length"),
    }
}

fn bound_name(bound: &crate::value::BoundFunctionValue) -> Value {
    if realm::is_intrinsic(bound) {
        get_property(&bound.target, "name")
    } else {
        Value::String(String::new())
    }
}

fn bound_prototype(bound: &crate::value::BoundFunctionValue) -> Value {
    if matches!(bound.target, Value::Builtin(Builtin::Function)) {
        return crate::vm::intrinsic_for_realm(bound.realm, Builtin::FunctionPrototype);
    }
    match get_property(&bound.target, "prototype") {
        Value::Builtin(builtin) => realm::intrinsic_value(bound, builtin)
            .unwrap_or_else(|| crate::vm::intrinsic_for_realm(bound.realm, builtin)),
        result => result,
    }
}

fn bound_other_property(bound: &crate::value::BoundFunctionValue, key: &str) -> Value {
    match get_property(&bound.target, key) {
        Value::Builtin(builtin) => crate::vm::intrinsic_for_realm(bound.realm, builtin),
        result if !matches!(result, Value::Undefined) => result,
        _ => function_prototype_property(key),
    }
}

fn intrinsic_error_prototype(
    value: &Value,
    bound: &crate::value::BoundFunctionValue,
) -> Option<Value> {
    if !realm::is_intrinsic(bound) {
        return None;
    }
    let Value::Builtin(constructor) = bound.target else {
        return None;
    };
    let prototype = crate::builtin_meta::prototype(constructor)?;
    if !matches!(
        prototype,
        Builtin::ErrorPrototype
            | Builtin::RangeErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::EvalErrorPrototype
            | Builtin::URIErrorPrototype
            | Builtin::AggregateErrorPrototype
            | Builtin::TypeErrorPrototype
    ) {
        return None;
    }
    let mut properties = vec![
        ("constructor".to_string(), value.clone()),
        ("name".to_string(), crate::builtins::property(prototype, "name")),
        ("message".to_string(), Value::String(String::new())),
        ("toString".to_string(), Value::Builtin(Builtin::ErrorPrototypeToString)),
        ("\0prototype".to_string(), if prototype == Builtin::ErrorPrototype {
            Value::Builtin(Builtin::ObjectPrototype)
        } else {
            Value::Builtin(Builtin::ErrorPrototype)
        }),
    ];
    if prototype == Builtin::ErrorPrototype {
        properties.push((
            crate::builtins::descriptor_key("stack"),
            Value::Object(Rc::new(crate::value::ObjectData::new(vec![
                (
                    "get".to_string(),
                    error_accessor_function(
                        Builtin::ErrorPrototypeStackGetter,
                        bound.receiver.clone(),
                    ),
                ),
                (
                    "set".to_string(),
                    error_accessor_function(
                        Builtin::ErrorPrototypeStackSetter,
                        bound.receiver.clone(),
                    ),
                ),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]))),
        ));
    }
    Some(Value::Object(Rc::new(crate::value::ObjectData::new(properties))))
}

fn error_accessor_function(builtin: Builtin, receiver: Value) -> Value {
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(builtin),
        receiver,
        arguments: Vec::new(),
        properties: std::cell::RefCell::new(Vec::new()),
    }))
}
fn bind_method(receiver: &Value, property: Value) -> Value {
    let Value::Builtin(builtin) = property else {
        return property;
    };
    let properties = if matches!(
        builtin,
        Builtin::GeneratorNext | Builtin::GeneratorReturn | Builtin::GeneratorThrow
    ) {
        let name = crate::builtins::builtin_name(builtin).to_string();
        RefCell::new(vec![
            ("length".to_string(), Value::Number(1.0)),
            ("name".to_string(), Value::String(name)),
            (
                crate::builtins::descriptor_key("length"),
                bound_function_descriptor(Value::Number(1.0)),
            ),
            (
                crate::builtins::descriptor_key("name"),
                bound_function_descriptor(Value::String(
                    crate::builtins::builtin_name(builtin).to_string(),
                )),
            ),
        ])
    } else if builtin == Builtin::IntlNumberFormatFormat {
        RefCell::new(number_format_bound_properties())
    } else {
        RefCell::new(Vec::new())
    };
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm: match receiver {
            Value::BoundFunction(bound) => bound.realm,
            Value::Function(function) => crate::vm::realm_id_for_global(&function.captures.get(0))
                .unwrap_or_else(|| crate::vm::current_context_or_default().realm()),
            _ => crate::vm::realm_id_for_intrinsic_receiver(Some(receiver))
                .unwrap_or_else(|| crate::vm::current_context_or_default().realm()),
        },
        target: Value::Builtin(builtin),
        receiver: receiver.clone(),
        arguments: Vec::new(),
        properties,
    }))
}

fn bound_function_descriptor(value: Value) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}
fn number_format_bound_properties() -> Vec<(String, Value)> {
    [
        ("length", Value::Number(1.0)),
        ("name", Value::String(String::new())),
    ]
    .into_iter()
    .flat_map(|(key, value)| {
        let metadata = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
            ("value".to_string(), value.clone()),
            ("writable".to_string(), Value::Boolean(false)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ])));
        [
            (key.to_string(), value),
            (crate::builtins::descriptor_key(key), metadata),
        ]
    })
    .collect()
}
fn promise_property(value: &Value, key: &str) -> Value {
    if key == "finally" {
        return Value::Builtin(Builtin::PromiseFinally);
    }
    let Some(builtin @ (Builtin::PromiseThen | Builtin::PromiseCatch | Builtin::PromiseFinally)) =
        (match crate::builtins::property(Builtin::PromisePrototype, key) {
            Value::Builtin(builtin) => Some(builtin),
            _ => None,
        })
    else {
        return crate::builtins::property(Builtin::PromisePrototype, key);
    };
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm: crate::vm::current_context_or_default().realm(),
        target: Value::Builtin(builtin),
        receiver: value.clone(),
        arguments: Vec::new(),
        properties: RefCell::new(Vec::new()),
    }))
}
fn array_buffer_property(buffer: &crate::value::ArrayBufferData, key: &str) -> Value {
    match key {
        "byteLength" => Value::Number(buffer.byte_length() as f64),
        "maxByteLength" => {
            Value::Number(buffer.max_byte_length.unwrap_or(buffer.byte_length()) as f64)
        }
        "resizable" => Value::Boolean(buffer.max_byte_length.is_some()),
        "immutable" => Value::Boolean(buffer.immutable),
        "resize" => Value::Builtin(Builtin::ArrayBufferResize),
        "grow" if buffer.shared => Value::Builtin(Builtin::ArrayBufferResize),
        "transferToImmutable" => Value::Builtin(Builtin::ArrayBufferTransferToImmutable),
        _ => crate::builtins::property(Builtin::ArrayBuffer, key),
    }
}
fn float64_array_property(view: &crate::value::Float64ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Float64ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Float64ArrayPrototype, key),
    }
}
fn float32_array_property(view: &crate::value::Float32ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Float32ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Float32ArrayPrototype, key),
    }
}
fn int8_array_property(view: &crate::value::Int8ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => Value::Number(crate::value::Int8ArrayData::BYTES_PER_ELEMENT as f64),
        _ => crate::builtins::property(Builtin::Int8ArrayPrototype, key),
    }
}
fn int16_array_property(view: &crate::value::Int16ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Int16ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Int16ArrayPrototype, key),
    }
}
fn int32_array_property(view: &crate::value::Int32ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Int32ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Int32ArrayPrototype, key),
    }
}
fn uint16_array_property(view: &crate::value::Uint16ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Uint16ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Uint16ArrayPrototype, key),
    }
}
fn uint8_array_property(view: &crate::value::Uint8ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Uint8ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Uint8ArrayPrototype, key),
    }
}
fn uint32_array_property(view: &crate::value::Uint32ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Uint32ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Uint32ArrayPrototype, key),
    }
}
fn uint8_clamped_array_property(view: &crate::value::Uint8ClampedArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.length != usize::MAX
        && view.buffer.byte_length() < view.byte_offset.saturating_add(view.byte_length());
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.logical_len() } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Uint8ClampedArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Uint8ClampedArrayPrototype, key),
    }
}
fn typed_index(key: &str, get: impl FnOnce(usize) -> Option<f64>) -> Option<Value> {
    let index = key.parse().ok()?;
    Some(get(index).map_or(Value::Undefined, Value::Number))
}
fn data_view_instance_accessor(value: &Value, key: &str) -> Option<Result<Value, VmError>> {
    let Value::DataView(view) = value else {
        return None;
    };
    if view.prototype().is_some() {
        return None;
    }
    match key {
        "buffer" => Some(Ok(Value::ArrayBuffer(view.buffer.clone()))),
        "byteLength" | "byteOffset" => {
            if view.is_detached() || view.is_out_of_bounds() {
                return Some(Err(crate::value::error::throw_type_error(
                    "Detached DataView",
                )));
            }
            let value = if key == "byteLength" {
                view.byte_length()
            } else {
                view.byte_offset
            };
            Some(Ok(Value::Number(value as f64)))
        }
        _ => None,
    }
}

include!("vm_properties_tail.rs");
