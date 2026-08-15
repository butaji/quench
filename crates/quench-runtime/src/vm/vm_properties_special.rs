fn host_capability_property(value: &Value, capability: HostCapabilityRef, key: &str) -> Value {
    let builtin = Builtin::HostCapability(capability.kind);
    let property = crate::builtins::property(builtin, key);
    if let Value::Builtin(Builtin::AbstractModuleSource) = property {
        return crate::vm::realm_intrinsic(Builtin::AbstractModuleSource);
    }
    if matches!(property, Value::Builtin(_)) {
        return bind_method(value, property);
    }
    property
}
fn bind_callable_property(value: &Value, builtin: Builtin, key: &str) -> Value {
    let property = builtin_property(builtin, key);
    if let Some(result) = constructor_property(builtin, key, property.clone()) {
        return result;
    }
    if !matches!(property, Value::Undefined) {
        return property;
    }
    callable_fallback(value, builtin, key)
}

fn constructor_property(builtin: Builtin, key: &str, property: Value) -> Option<Value> {
    if !matches!(key, "prototype" | "constructor") {
        return None;
    }
    if key == "constructor" {
        return Some(match property {
            Value::Builtin(target) => crate::vm::realm_intrinsic(target),
            Value::Undefined if crate::builtin_meta::constructor_name(builtin).is_some() => {
                Value::Builtin(Builtin::Function)
            }
            property => property,
        });
    }
    Some(property)
}

fn callable_fallback(value: &Value, builtin: Builtin, key: &str) -> Value {
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
    let shadow_wrapper = is_shadow_wrapper(bound);
    let deleted = crate::builtins::deleted_key(key);
    if bound
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == &deleted)
    {
        return Value::Undefined;
    }
    if let Some((_, value)) = bound
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == key)
    {
        return value.clone();
    }
    if intrinsic_target_is_abstract_module_source(bound) {
        return intrinsic_bound_property(bound, key);
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
        bound_function_fallback(bound, shadow_wrapper, key)
    }
}

fn intrinsic_target_is_abstract_module_source(bound: &crate::value::BoundFunctionValue) -> bool {
    bound.target == Value::Builtin(Builtin::AbstractModuleSource)
}

fn intrinsic_bound_property(bound: &crate::value::BoundFunctionValue, key: &str) -> Value {
    let Value::Builtin(builtin) = bound.target else {
        return Value::Undefined;
    };
    builtin_property(builtin, key)
}

fn is_shadow_wrapper(bound: &crate::value::BoundFunctionValue) -> bool {
    bound
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "\0realm")
        && !realm::is_intrinsic(bound)
}

fn bound_function_fallback(
    bound: &crate::value::BoundFunctionValue,
    shadow_wrapper: bool,
    key: &str,
) -> Value {
    if shadow_wrapper {
        return function_prototype_property_for_builtin(Builtin::FunctionPrototype, key);
    }
    if let Value::Builtin(builtin) = bound.target {
        let intrinsic = match (builtin, key) {
            (Builtin::AsyncFunction, "prototype") => Some(Builtin::AsyncFunctionPrototype),
            (Builtin::GeneratorFunction, "prototype") => Some(Builtin::GeneratorFunctionPrototype),
            (Builtin::AsyncGeneratorFunction, "prototype") => {
                Some(Builtin::AsyncGeneratorFunctionPrototype)
            }
            (Builtin::AsyncFunctionPrototype, "constructor") => Some(Builtin::AsyncFunction),
            (Builtin::GeneratorFunctionPrototype, "constructor") => {
                Some(Builtin::GeneratorFunction)
            }
            (Builtin::AsyncGeneratorFunctionPrototype, "constructor") => {
                Some(Builtin::AsyncGeneratorFunction)
            }
            _ => None,
        };
        if let Some(intrinsic) = intrinsic {
            return realm::intrinsic(bound.realm, intrinsic).unwrap_or(Value::Builtin(intrinsic));
        }
    }
    let result = get_property(&bound.target, key);
    if matches!(result, Value::Undefined) {
        function_prototype_property_for_builtin(Builtin::FunctionPrototype, key)
    } else {
        result
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
        realm: crate::vm::current_context_or_default().realm(),
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
        realm: crate::vm::current_context_or_default().realm(),
        target: Value::Builtin(builtin),
        receiver: value.clone(),
        arguments: Vec::new(),
        properties: RefCell::new(Vec::new()),
    }))
}
fn array_buffer_property(buffer: &crate::value::ArrayBufferData, key: &str) -> Value {
    if let Some(value) = buffer.own_property(key) {
        return value;
    }
    if let Some(value) = shared_buffer_property(buffer, key) {
        return value;
    }
    match key {
        "byteLength" => Value::Number(buffer.byte_length() as f64),
        "maxByteLength" => {
            Value::Number(buffer.max_byte_length.unwrap_or(buffer.byte_length()) as f64)
        }
        "resizable" => Value::Boolean(buffer.max_byte_length.is_some()),
        "growable" => Value::Boolean(buffer.shared && buffer.max_byte_length.is_some()),
        "resize" => Value::Builtin(Builtin::ArrayBufferResize),
        "transferToImmutable" => Value::Builtin(Builtin::ArrayBufferTransferToImmutable),
        "constructor" | "Symbol.toStringTag" => {
            crate::builtins::property(Builtin::ArrayBufferPrototype, key)
        }
        _ => crate::builtins::property(Builtin::ArrayBuffer, key),
    }
}

fn shared_buffer_property(buffer: &crate::value::ArrayBufferData, key: &str) -> Option<Value> {
    if !buffer.shared {
        return None;
    }
    Some(match key {
        "constructor" => Value::Builtin(Builtin::SharedArrayBuffer),
        "grow" => Value::Builtin(Builtin::SharedArrayBufferGrow),
        "slice" => Value::Builtin(Builtin::SharedArrayBufferSlice),
        _ => return None,
    })
}
fn float64_array_property(view: &crate::value::Float64ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index)) {
        return value;
    }
    let detached = typed_array_detached(
        view.length,
        &view.buffer,
        view.byte_offset,
        view.byte_length(),
    );
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

fn typed_array_detached(
    length: usize,
    buffer: &crate::value::ArrayBufferData,
    byte_offset: usize,
    byte_length: usize,
) -> bool {
    length != usize::MAX && buffer.byte_length() < byte_offset.saturating_add(byte_length)
}
fn float32_array_property(view: &crate::value::Float32ArrayData, key: &str) -> Value {
    if let Some(value) = typed_index(key, |index| view.get(index).map(f64::from)) {
        return value;
    }
    let detached = typed_array_detached(
        view.length,
        &view.buffer,
        view.byte_offset,
        view.byte_length(),
    );
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
    let detached = typed_array_detached(
        view.length,
        &view.buffer,
        view.byte_offset,
        view.byte_length(),
    );
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
    let detached = typed_array_detached(
        view.length,
        &view.buffer,
        view.byte_offset,
        view.byte_length(),
    );
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
    let detached = typed_array_detached(
        view.length,
        &view.buffer,
        view.byte_offset,
        view.byte_length(),
    );
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
    let detached = typed_array_detached(
        view.length,
        &view.buffer,
        view.byte_offset,
        view.byte_length(),
    );
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
            if data_view_invalid(view) {
                return Some(Err(crate::value::error::throw_type_error(
                    "Detached DataView",
                )));
            }
            Some(Ok(Value::Number(data_view_length(view, key) as f64)))
        }
        _ => None,
    }
}

fn data_view_length(view: &crate::value::DataViewData, key: &str) -> usize {
    if key == "byteLength" {
        view.byte_length()
    } else {
        view.byte_offset
    }
}

fn data_view_invalid(view: &crate::value::DataViewData) -> bool {
    view.is_detached() || view.is_out_of_bounds()
}

fn data_view_property(view: &crate::value::DataViewData, key: &str) -> Value {
    if let Some(value) = view.own_property(key) {
        return value;
    }
    if let Some(value) = data_view_own_property(view, key) {
        return value;
    }
    if let Some(prototype) = view.prototype() {
        return get_property(&prototype, key);
    }
    data_view_prototype_property(key)
}

fn data_view_own_property(view: &crate::value::DataViewData, key: &str) -> Option<Value> {
    Some(match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(view.byte_length() as f64),
        "byteOffset" => Value::Number(view.byte_offset as f64),
        _ => return None,
    })
}

fn data_view_prototype_property(key: &str) -> Value {
    let value = crate::builtins::property(Builtin::DataViewPrototype, key);
    if matches!(value, Value::Undefined) {
        return crate::builtins::property(Builtin::ObjectPrototype, key);
    }
    value
}

pub(crate) fn array_accessor(value: &Value, key: &str, field: &str) -> Option<Value> {
    let Value::Array(values) = value else {
        return None;
    };
    array_accessor_value(values, key, field)
}

fn array_accessor_value(values: &crate::value::ArrayData, key: &str, field: &str) -> Option<Value> {
    let Value::Object(descriptor) = values.descriptor(key)? else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then(|| value.clone()))
}

fn same_property_receiver(value: &Value, receiver: &Value) -> bool {
    match (value, receiver) {
        (Value::Builtin(left), Value::Builtin(right)) => left == right,
        (Value::Map(left), Value::Map(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Set(left), Value::Set(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Array(left), Value::Array(right)) => std::rc::Rc::ptr_eq(left, right),
        _ => primitive_property_receiver(value, receiver),
    }
}

fn primitive_property_receiver(value: &Value, receiver: &Value) -> bool {
    match (value, receiver) {
        (Value::Number(_), Value::Number(_))
        | (Value::Boolean(_), Value::Boolean(_))
        | (Value::BigInt(_), Value::BigInt(_))
        | (Value::String(_), Value::String(_))
        | (Value::StringUnits(_), Value::StringUnits(_)) => value == receiver,
        _ => false,
    }
}
include!("vm_object_properties.rs");
