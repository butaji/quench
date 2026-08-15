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
        ObjectAlias(alias) => object_alias_property(alias, key),
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
            Builtin::RegExpStringIteratorNext | Builtin::SetIteratorNext | Builtin::MapIteratorNext
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
        return Err(crate::value::error::throw_type_error(&format!(
            "Cannot read property `{key}` of null or undefined"
        )));
    }
    if let Some(id) = consume_deferred_namespace_marker(value, key) {
        crate::vm::execute_deferred_module(id)?;
        return get_property_with_receiver(value, key, receiver);
    }
    if crate::builtins::namespace_uninitialized(value, key) {
        return Err(crate::value::error::throw_reference_error(
            "Cannot access an uninitialized module binding",
        ));
    }
    if matches!(value, Value::Proxy(_)) {
        return crate::proxy::proxy_get(value, key, Some(receiver));
    }
    if matches!(value, Value::Array(values) if values.is_strict_arguments() && key == "callee") {
        return Err(crate::value::error::throw_type_error(
            "'callee' is unavailable on strict arguments",
        ));
    }
    if has_restricted_function_property(value, key) {
        return Err(crate::value::error::throw_type_error(
            "'caller' and 'arguments' are unavailable on this function",
        ));
    }
    if let Some(getter) = array_accessor(value, key, "get") {
        if matches!(getter, Value::Undefined) {
            return Ok(Value::Undefined);
        }
        return invoke_accessor(&getter, receiver);
    }
    if let Value::Array(values) = value {
        let has_own = key == "length"
            || crate::arrays::array_index(key)
                .is_some_and(|index| values.has_index(index as usize))
            || values.descriptor(key).is_some()
            || values.property(key).is_some();
        if !has_own {
            if let Some(getter) = crate::arrays::prototype_override_getter(key) {
                if matches!(getter, Value::Undefined) {
                    return Ok(Value::Undefined);
                }
                return invoke_accessor(&getter, receiver);
            }
        }
    }
    if let Some(result) = crate::disposable_stack::accessor(value, key, receiver) {
        return result;
    }
    if let Some(result) = data_view_instance_accessor(value, key) {
        return result;
    }
    if let Ok(descriptor) =
        crate::builtins::object::descriptor(Some(value), Some(&Value::String(key.to_string())))
    {
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
    let getter = crate::property_define::accessor(value, key, "get");
    let Some(getter) = getter else {
        return Ok(receiver_property(value, key, receiver));
    };
    if matches!(getter, Value::Undefined) {
        return Ok(Value::Undefined);
    }
    invoke_accessor(&getter, receiver)
}

pub(crate) fn consume_deferred_namespace_marker(value: &Value, key: &str) -> Option<u32> {
    if key == "then" || key == "Symbol.toStringTag" {
        return None;
    }
    let Value::Object(properties) = value else {
        return None;
    };
    let marker = format!("\0quench:deferred:\0{key}");
    let id = properties.iter().rev().find_map(|(name, value)| {
        if name == "\0quench:deferred-module" {
            return deferred_marker_id(value);
        }
        if name != &marker {
            return None;
        }
        deferred_marker_id(value)
    })?;
    for (name, value) in properties.iter() {
        if name.starts_with("\0quench:deferred:\0") || name == "\0quench:deferred-module" {
            if let Value::BindingCell(cell) = value {
                cell.replace(Value::Undefined);
            }
        }
    }
    Some(id)
}

fn deferred_marker_id(value: &Value) -> Option<u32> {
    let Value::BindingCell(cell) = value else {
        return None;
    };
    let Value::Number(id) = cell.borrow().clone() else {
        return None;
    };
    Some(id as u32)
}

pub(crate) fn deferred_namespace_operation(value: &Value) -> Option<u32> {
    let Value::Object(properties) = value else {
        return None;
    };
    let key = properties.iter().find_map(|(name, _)| {
        name.strip_prefix("\0quench:deferred:\0")
            .map(str::to_string)
    })?;
    consume_deferred_namespace_marker(value, &key)
}

/// Invoke a getter using the receiver as `this`. The getter's own
/// `OrdinaryCallEvaluate` semantics handle ToObject coercion for sloppy
/// functions; strict functions keep the receiver as-is.
fn invoke_accessor(getter: &Value, receiver: &Value) -> Result<Value, VmError> {
    match getter {
        Value::Function(function) => crate::functions::execute(function, receiver, &[]),
        Value::BoundFunction(bound) => crate::functions::execute_bound(bound, &[]),
        Value::Builtin(builtin) => {
            crate::vm::execute_builtin_with_receiver(*builtin, &[], Some(receiver))
        }
        _ => Err(crate::vm::not_callable()),
    }
}

fn receiver_property(value: &Value, key: &str, receiver: &Value) -> Value {
    let property = get_property(value, key);
    if matches!(value, Value::Builtin(_)) {
        return property;
    }
    if matches!(value, Value::Object(_)) && crate::vm::is_global_object(value) {
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
    if same_property_receiver(value, receiver) {
        return property;
    }
    match property {
        Value::Builtin(builtin)
            if !is_accessor_builtin(builtin)
                && crate::intl::tolocale::symbol::name(builtin).is_none() =>
        {
            bind_method(receiver, property)
        }
        other => other,
    }
}
/// Accessor getters/setters carry their `this` at invocation time; binding
/// them to the object they were read from (e.g. a property descriptor's
/// `.get`) would call them with the wrong receiver.
fn is_accessor_builtin(builtin: Builtin) -> bool {
    let name = crate::builtins::builtin_name(builtin);
    name.starts_with("get ") || name.starts_with("set ")
}
fn same_property_receiver(value: &Value, receiver: &Value) -> bool {
    match (value, receiver) {
        (Value::Builtin(left), Value::Builtin(right)) => left == right,
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
    let builtin = match key {
        "apply" => Builtin::FunctionApply,
        "call" => Builtin::FunctionCall,
        "bind" => Builtin::FunctionBind,
        _ => return Value::Undefined,
    };
    bind_method(value, Value::Builtin(builtin))
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
    if matches!(key, "apply" | "call" | "bind") {
        bind_function_property(value, key)
    } else if key == "length" && !realm::is_intrinsic(bound) {
        match &bound.target {
            Value::Builtin(builtin) => {
                crate::builtins::props::callable(*builtin, key).unwrap_or(Value::Number(0.0))
            }
            target => get_property(target, key),
        }
    } else if key == "name" && !realm::is_intrinsic(bound) {
        Value::String(String::new())
    } else {
        let result = get_property(&bound.target, key);
        if !matches!(result, Value::Undefined) {
            return result;
        }
        function_prototype_property(key)
    }
}
fn bind_method(receiver: &Value, property: Value) -> Value {
    let Value::Builtin(builtin) = property else {
        return property;
    };
    let properties = if builtin == Builtin::IntlNumberFormatFormat {
        RefCell::new(number_format_bound_properties())
    } else {
        RefCell::new(Vec::new())
    };
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(builtin),
        receiver: receiver.clone(),
        arguments: Vec::new(),
        properties,
    }))
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

fn data_view_property(view: &crate::value::DataViewData, key: &str) -> Value {
    if let Some(value) = view.own_property(key) {
        return value;
    }
    match key {
        "buffer" => return Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => return Value::Number(view.byte_length() as f64),
        "byteOffset" => return Value::Number(view.byte_offset as f64),
        _ => {}
    }
    if let Some(prototype) = view.prototype() {
        return get_property(&prototype, key);
    }
    let value = crate::builtins::property(Builtin::DataViewPrototype, key);
    if matches!(value, Value::Undefined) {
        return crate::builtins::property(Builtin::ObjectPrototype, key);
    }
    value
}
include!("vm_object_properties.rs");
