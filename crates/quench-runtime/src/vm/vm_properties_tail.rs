fn native_error_stack_property(builtin: Builtin, key: &str) -> Option<Value> {
    if key != "stack"
        || !matches!(
            builtin,
            Builtin::RangeErrorPrototype
                | Builtin::ReferenceErrorPrototype
                | Builtin::SyntaxErrorPrototype
                | Builtin::EvalErrorPrototype
                | Builtin::URIErrorPrototype
                | Builtin::AggregateErrorPrototype
                | Builtin::TypeErrorPrototype
                | Builtin::SuppressedErrorPrototype
        )
    {
        return None;
    }
    Some(crate::vm::get_property(
        &Value::Builtin(Builtin::ErrorPrototype),
        key,
    ))
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
    if key == "prototype" {
        if let Some(prototype) = intrinsic_error_prototype(value, bound) {
            bound
                .properties
                .borrow_mut()
                .push((key.to_string(), prototype.clone()));
            return prototype;
        }
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
