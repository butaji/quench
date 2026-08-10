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
        .ok_or(VmError::RegisterOutOfBounds(index))
}

pub fn get_property(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
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
        DataView(view) => data_view_property(view, key),
        Object(properties) => object_property(properties, key),
        ObjectAlias(alias) => object_alias_property(alias, key),
        String(value) => string_property(value, key),
        Number(value) => number_property(*value, key),
        Boolean(value) => boolean_property(*value, key),
        Function(function) if key == "length" => Value::Number(f64::from(function.params)),
        Function(_) if matches!(key, "call" | "bind") => bind_function_property(value, key),
        Function(function) => function_property(function, key),
        BoundFunction(_) if matches!(key, "call" | "bind") => bind_function_property(value, key),
        Map(data) if key == "size" => Value::Number(data.keys.len() as f64),
        Map(_) => crate::collections::map::property(key),
        Set(data) if key == "size" => Value::Number(data.values.len() as f64),
        Set(_) => crate::collections::set::property(key),
        Iterator(_) => crate::collections::iterator::property(key),
        Promise(_) => promise_property(value, key),
        HostCapability(capability) => host_capability_property(value, capability.descriptor, key),
        _ => Value::Undefined,
    }
}

fn object_alias_property(alias: &crate::value::ObjectAliasValue, key: &str) -> Value {
    alias
        .0
        .borrow()
        .upgrade()
        .map_or(Value::Undefined, |object| object_property(&object, key))
}

fn function_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map_or(Value::Undefined, |(_, value)| value.clone())
}

pub fn get_property_result(value: &Value, key: &str) -> Result<Value, VmError> {
    if matches!(value, Value::Array(values) if values.is_strict_arguments() && key == "callee") {
        return Err(crate::value::error::throw_type_error(
            "'callee' is unavailable on strict arguments",
        ));
    }
    if let Some(getter) = array_accessor(value, key, "get") {
        if matches!(getter, Value::Undefined) {
            return Ok(Value::Undefined);
        }
        return crate::functions::execute_target(&getter, value, &[]);
    }
    let Value::Object(properties) = value else {
        return Ok(get_property(value, key));
    };
    let Some(getter) = accessor_getter(properties, key) else {
        return Ok(get_property(value, key));
    };
    if matches!(getter, Value::Undefined) {
        return Ok(Value::Undefined);
    }
    crate::functions::execute_target(&getter, value, &[])
}

pub(crate) fn array_accessor(value: &Value, key: &str, field: &str) -> Option<Value> {
    let Value::Array(values) = value else { return None };
    let Value::Object(descriptor) = values.descriptor(key)? else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then(|| value.clone()))
}

fn accessor_getter(properties: &[(String, Value)], key: &str) -> Option<Value> {
    let descriptor_key = crate::builtins::descriptor_key(key);
    let (_, Value::Object(descriptor)) = properties
        .iter()
        .rev()
        .find(|(name, _)| name == &descriptor_key)?
    else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "get").then(|| value.clone()))
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
    if builtin != Builtin::FunctionPrototype && matches!(key, "call" | "bind") {
        return bind_method(value, property);
    }
    property
}

fn bind_function_property(value: &Value, key: &str) -> Value {
    let builtin = match key {
        "call" => Builtin::FunctionCall,
        "bind" => Builtin::FunctionBind,
        _ => return Value::Undefined,
    };
    bind_method(value, Value::Builtin(builtin))
}

fn bind_method(receiver: &Value, property: Value) -> Value {
    let Value::Builtin(builtin) = property else {
        return property;
    };
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(builtin),
        receiver: receiver.clone(),
        arguments: Vec::new(),
    }))
}

fn promise_property(value: &Value, key: &str) -> Value {
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
    }))
}

fn array_buffer_property(buffer: &crate::value::ArrayBufferData, key: &str) -> Value {
    match key {
        "byteLength" => Value::Number(buffer.byte_length() as f64),
        _ => crate::builtins::property(Builtin::ArrayBuffer, key),
    }
}

fn float64_array_property(view: &crate::value::Float64ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index)) {
        return value;
    }
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
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
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
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
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => Value::Number(crate::value::Int8ArrayData::BYTES_PER_ELEMENT as f64),
        _ => crate::builtins::property(Builtin::Int8ArrayPrototype, key),
    }
}

fn int16_array_property(view: &crate::value::Int16ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
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
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
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
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
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
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
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
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
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
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
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

fn data_view_property(view: &crate::value::DataViewData, key: &str) -> Value {
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(view.byte_length() as f64),
        "byteOffset" => Value::Number(view.byte_offset as f64),
        _ => crate::builtins::property(Builtin::DataViewPrototype, key),
    }
}

fn object_property(properties: &Rc<Vec<(String, Value)>>, key: &str) -> Value {
    if let Some((_, value)) = properties.iter().rev().find(|(name, _)| name == key) {
        return value.clone();
    }
    if GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .as_ref()
            .is_some_and(|candidate| Rc::ptr_eq(candidate, properties))
    }) {
        return global_property(properties, key);
    }
    let prototype = object_prototype(properties);
    bind_method(
        &Value::Object(properties.clone()),
        crate::builtins::property(prototype, key),
    )
}

fn object_prototype(properties: &[(String, Value)]) -> Builtin {
    if let Some((_, value)) = properties.iter().find(|(name, _)| name == "_value") {
        return match value {
            Value::String(value) if value.contains('\0') => Builtin::SymbolPrototype,
            Value::String(_) => Builtin::StringPrototype,
            Value::Number(_) => Builtin::NumberPrototype,
            Value::Boolean(_) => Builtin::BooleanPrototype,
            Value::BigInt(_) => Builtin::BigIntPrototype,
            _ => Builtin::ObjectPrototype,
        };
    }
    if properties.iter().any(|(name, _)| name == "timeValue") {
        Builtin::DatePrototype
    } else if properties.iter().any(|(name, _)| name == "source")
        && properties.iter().any(|(name, _)| name == "flags")
    {
        Builtin::RegExpPrototype
    } else {
        Builtin::ObjectPrototype
    }
}

fn global_property(properties: &Rc<Vec<(String, Value)>>, key: &str) -> Value {
    if key == "globalThis" {
        return Value::Object(properties.clone());
    }
    if key == "$262" {
        return current_host_capability(HostCapabilityKind::GetGlobal);
    }
    global_builtin(key).map_or_else(
        || crate::builtins::property(Builtin::ObjectPrototype, key),
        Value::Builtin,
    )
}

fn current_host_capability(kind: HostCapabilityKind) -> Value {
    let realm = CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .map_or(RealmId::ROOT, VmContext::realm)
    });
    Value::HostCapability(Rc::new(crate::value::HostCapabilityValue::new(
        HostCapabilityRef { realm, kind },
    )))
}

fn global_builtin(key: &str) -> Option<Builtin> {
    use Builtin::*;
    Some(match key {
        "Array" => Array,
        "ArrayBuffer" => ArrayBuffer,
        "DataView" => DataView,
        "Float32Array" => Float32Array,
        "Float64Array" => Float64Array,
        "Int8Array" => Int8Array,
        "Int16Array" => Int16Array,
        "Int32Array" => Int32Array,
        "Uint8Array" => Uint8Array,
        "Uint8ClampedArray" => Uint8ClampedArray,
        "Uint16Array" => Uint16Array,
        "Uint32Array" => Uint32Array,
        "Object" => Object,
        "Function" => Function,
        "Promise" => Promise,
        "RegExp" => RegExp,
        "Symbol" => Symbol,
        "Number" => Number,
        "Boolean" => Boolean,
        "BigInt" => BigInt,
        "String" => String,
        "Date" => Date,
        "JSON" => Json,
        "Map" => Map,
        "Set" => Set,
        "Error" => Error,
        "TypeError" => TypeError,
        "RangeError" => RangeError,
        "ReferenceError" => ReferenceError,
        "SyntaxError" => SyntaxError,
        "EvalError" => EvalError,
        "URIError" => URIError,
        _ => return None,
    })
}
