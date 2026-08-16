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
        "detached" => Value::Boolean(*buffer.detached.borrow()),
        "immutable" => Value::Boolean(buffer.immutable),
        "slice" => Value::Builtin(if buffer.shared {
            Builtin::SharedArrayBufferSlice
        } else {
            Builtin::ArrayBufferSlice
        }),
        "growable" => Value::Boolean(buffer.shared && buffer.max_byte_length.is_some()),
        "resize" => Value::Builtin(Builtin::ArrayBufferResize),
        "transferToImmutable" => Value::Builtin(Builtin::ArrayBufferTransferToImmutable),
        "transfer" => Value::Builtin(Builtin::ArrayBufferTransfer),
        "transferToFixedLength" => Value::Builtin(Builtin::ArrayBufferTransferToFixedLength),
        "sliceToImmutable" => Value::Builtin(Builtin::ArrayBufferSliceToImmutable),
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
    if let Some(value) = view.meta.property(key) {
        return value;
    }
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

