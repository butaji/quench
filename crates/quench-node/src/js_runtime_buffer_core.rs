fn buffer_module() -> Value {
    let mut buffer = capability_function(HostCapabilityKind::Custom(CapabilityName::BufferFrom));
    buffer = quench_runtime::execute::set_property(
        buffer,
        "Symbol.hasInstance",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::BufferHasInstance,
        )),
    );
    for (name, kind) in [
        (
            "from",
            HostCapabilityKind::Custom(CapabilityName::BufferFrom),
        ),
        (
            "alloc",
            HostCapabilityKind::Custom(CapabilityName::BufferAlloc),
        ),
        (
            "isBuffer",
            HostCapabilityKind::Custom(CapabilityName::BufferIsBuffer),
        ),
        (
            "byteLength",
            HostCapabilityKind::Custom(CapabilityName::BufferByteLength),
        ),
        (
            "concat",
            HostCapabilityKind::Custom(CapabilityName::BufferConcat),
        ),
        ("of", HostCapabilityKind::Custom(CapabilityName::BufferOf)),
        (
            "allocUnsafeSlow",
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafeSlow),
        ),
        (
            "allocUnsafe",
            HostCapabilityKind::Custom(CapabilityName::BufferAllocUnsafe),
        ),
        (
            "isEncoding",
            HostCapabilityKind::Custom(CapabilityName::BufferIsEncoding),
        ),
        (
            "copyBytesFrom",
            HostCapabilityKind::Custom(CapabilityName::BufferCopyBytesFrom),
        ),
        (
            "readBigInt64LE",
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigInt64LE),
        ),
        (
            "readBigUInt64BE",
            HostCapabilityKind::Custom(CapabilityName::BufferReadBigUInt64BE),
        ),
        (
            "writeBigInt64LE",
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigInt64LE),
        ),
        (
            "writeBigUInt64BE",
            HostCapabilityKind::Custom(CapabilityName::BufferWriteBigUInt64BE),
        ),
        (
            "compare",
            HostCapabilityKind::Custom(CapabilityName::BufferCompare),
        ),
    ] {
        buffer = quench_runtime::execute::set_property(buffer, name, capability_function(kind));
    }
    let mut prototype = Value::object(vec![]);
    let read_uint32_be = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 12,
    ));
    prototype =
        quench_runtime::execute::set_property(prototype, "readUInt32BE", read_uint32_be.clone());
    prototype = quench_runtime::execute::set_property(prototype, "readUint32BE", read_uint32_be);
    let write_uint_le = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 19,
    ));
    prototype =
        quench_runtime::execute::set_property(prototype, "writeUIntLE", write_uint_le.clone());
    prototype = quench_runtime::execute::set_property(prototype, "writeUintLE", write_uint_le);
    for (name, capability) in [
        ("copy", CapabilityName::BufferCopy),
        ("swap16", CapabilityName::BufferSwap16),
        ("readBigInt64LE", CapabilityName::BufferReadBigInt64LE),
        ("writeBigInt64LE", CapabilityName::BufferWriteBigInt64LE),
    ] {
        prototype = quench_runtime::execute::set_property(
            prototype,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        );
    }
    buffer = quench_runtime::execute::set_property(buffer, "prototype", prototype);
    let constants = quench_runtime::host_api::object(vec![
        ("MAX_LENGTH".into(), Value::Number(4_294_967_296.0)),
        ("MAX_STRING_LENGTH".into(), Value::Number(536_870_888.0)),
    ]);
    buffer = quench_runtime::execute::set_property(buffer, "constants", constants.clone());
    buffer =
        quench_runtime::execute::set_property(buffer, "kMaxLength", Value::Number(4_294_967_296.0));
    buffer = quench_runtime::execute::set_property(
        buffer,
        "kStringMaxLength",
        Value::Number(536_870_888.0),
    );
    buffer = quench_runtime::execute::set_property(buffer, "poolSize", Value::Number(8192.0));
    buffer
}

fn buffer_from(arguments: &[Value]) -> Result<Value, VmError> {
    match arguments.first() {
        Some(Value::String(value)) if value.starts_with("Symbol.") => Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", &format!("The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. {}", buffer_from_received(&Value::String(value.clone())))))),
        Some(Value::String(value)) if matches!(arguments.get(1), Some(Value::String(encoding)) if encoding.eq_ignore_ascii_case("hex")) =>
        {
            let bytes = decode_hex(value);
            Ok(node_buffer(&bytes))
        }
        Some(Value::String(value)) => {
            let encoding = arguments.get(1).and_then(|value| match value { Value::String(value) => Some(value.to_ascii_lowercase()), _ => None }).unwrap_or_else(|| "utf8".into());
            match encoding.as_str() {
                "ascii" | "latin1" | "binary" => Ok(node_buffer(&value.chars().map(|character| character as u32 as u8).collect::<Vec<_>>())),
                "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => Ok(node_buffer(&value.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>())),
                "utf8" | "utf-8" | "hex" | "base64" | "base64url" => Ok(node_buffer(value.as_bytes())),
                _ => Err(VmError::Thrown(fs_error("ERR_UNKNOWN_ENCODING", "Unknown encoding"))),
            }
        }
        Some(Value::ArrayBuffer(buffer)) => {
            let offset = arguments.get(1).and_then(|value| match value { Value::Number(value) => Some((*value).max(0.0) as usize), _ => None }).unwrap_or(0);
            let length = match arguments.get(2) {
                None | Some(Value::Undefined) => buffer.bytes.borrow().len().saturating_sub(offset),
                Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value as usize,
                Some(Value::Number(_)) => return Err(VmError::Thrown(fs_error("ERR_BUFFER_OUT_OF_BOUNDS", "length out of bounds"))),
                Some(_) => 0,
            };
            if offset + length > buffer.bytes.borrow().len() { return Err(VmError::Thrown(fs_error("ERR_BUFFER_OUT_OF_BOUNDS", "offset out of bounds"))); }
            let view = Value::Uint8Array(Rc::new(quench_runtime::value::Uint8ArrayData::new(Rc::clone(buffer), offset, length)));
            let view = quench_runtime::execute::set_property(view, "toString", capability_function(HostCapabilityKind::Custom(CapabilityName::BufferToString)));
            let view = quench_runtime::execute::set_property(view, "parent", Value::ArrayBuffer(buffer.clone()));
            Ok(quench_runtime::execute::set_property(view, "offset", Value::Number(offset as f64)))
        }
        Some(Value::Uint8Array(view)) => Ok(node_buffer(
            &view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length],
        )),
        Some(Value::Uint16Array(_)) | Some(Value::Uint32Array(_)) | Some(Value::Int8Array(_)) | Some(Value::Int16Array(_)) | Some(Value::Int32Array(_)) | Some(Value::Float32Array(_)) | Some(Value::Float64Array(_)) => Ok(node_buffer(&array_values(arguments.first().unwrap())?.into_iter().filter_map(|value| match value { Value::Number(value) => Some((value as i64).rem_euclid(256) as u8), _ => None }).collect::<Vec<_>>())),
        Some(Value::Array(_)) => Ok(node_buffer(
            &array_values(arguments.first().unwrap())?
                .into_iter()
                .filter_map(|value| match value {
                    Value::Number(value) => Some(value as u8),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )),
        Some(Value::Object(_)) => {
            let object = arguments.first().unwrap();
            if let Ok(method) = quench_runtime::execute::get_property_result(object, "Symbol.toPrimitive") {
                if let Ok(value) = quench_runtime::execute::call(&method, object, &[]) { return buffer_from(&[value]); }
            }
            if let Ok(Value::Number(length)) = quench_runtime::execute::get_property_result(object, "length") {
                let mut bytes = Vec::new();
                for index in 0..(length.max(0.0) as usize) {
                    if let Ok(Value::Number(value)) = quench_runtime::execute::get_property_result(object, &index.to_string()) {
                        bytes.push((value as i64).rem_euclid(256) as u8);
                    }
                }
                if length > 0.0 && bytes.len() == length as usize { return Ok(node_buffer(&bytes)); }
            }
            Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "value must be a string, Buffer, or array-like object")))
        }
        Some(value) => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            &format!("The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. {}", buffer_from_received(value)),
        ))),
        None => Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. Received undefined"))),
    }
}

fn buffer_from_received(value: &Value) -> String {
    match value {
        Value::Undefined => "Received undefined".into(),
        Value::Null => "Received null".into(),
        Value::BigInt(value) => format!("Received type bigint ({}n)", value),
        Value::String(value) if value.contains("Symbol") => {
            format!("Received type symbol ({})", value.replace('\0', ""))
        }
        Value::Function(_) | Value::BoundFunction(_) => "Received function ".into(),
        Value::Boolean(_) | Value::Number(_) => format!(
            "Received type {} ({})",
            type_name(value),
            safe_value_string(value)
        ),
        Value::Object(_) | Value::ObjectAlias(_) => {
            if matches!(
                quench_runtime::execute::call(
                    &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                    &Value::Undefined,
                    &[value.clone()]
                ),
                Ok(Value::Null)
            ) {
                return "Received [Object: null prototype] {}".into();
            }
            let name = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                &Value::Undefined,
                &[value.clone()],
            )
            .ok()
            .and_then(|prototype| {
                quench_runtime::execute::get_property_result(&prototype, "constructor").ok()
            })
            .and_then(|constructor| {
                quench_runtime::execute::get_property_result(&constructor, "name").ok()
            })
            .and_then(|name| match name {
                Value::String(name) => Some(name),
                _ => None,
            })
            .unwrap_or_else(|| "Object".into());
            format!("Received an instance of {name}")
        }
        _ => format!("Received type {}", type_name(value)),
    }
}

fn buffer_alloc(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(length)) = arguments.first() else {
        return Err(VmError::EvalError("Buffer.alloc expects a length".into()));
    };
    if !length.is_finite() || *length < 0.0 {
        return Err(VmError::EvalError("invalid buffer length".into()));
    }
    let pattern = match arguments.get(1) {
        Some(Value::Number(value)) => vec![*value as u8],
        Some(Value::String(value)) if matches!(arguments.get(2), Some(Value::String(encoding)) if encoding.eq_ignore_ascii_case("hex")) => {
            decode_hex(value)
        }
        Some(Value::String(value)) => value.as_bytes().to_vec(),
        _ => vec![0],
    };
    let pattern = if pattern.is_empty() { vec![0] } else { pattern };
    Ok(node_buffer(
        &(0..*length as usize)
            .map(|index| pattern[index % pattern.len()])
            .collect::<Vec<_>>(),
    ))
}

fn buffer_of(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(node_buffer(
        &arguments
            .iter()
            .map(|value| match value {
                Value::Number(value) => (*value as i64).rem_euclid(256) as u8,
                _ => 0,
            })
            .collect::<Vec<_>>(),
    ))
}

fn buffer_alloc_unsafe(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(length)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "size must be a number",
        )));
    };
    if !length.is_finite() || *length < 0.0 {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            "size out of range",
        )));
    }
    Ok(node_buffer(&vec![0; *length as usize]))
}

fn buffer_is_encoding(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(value)) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    Ok(Value::Boolean(matches!(
        value.to_ascii_lowercase().as_str(),
        "utf8"
            | "utf-8"
            | "utf16le"
            | "ucs2"
            | "ucs-2"
            | "latin1"
            | "binary"
            | "ascii"
            | "base64"
            | "base64url"
            | "hex"
    )))
}

fn node_buffer(bytes: &[u8]) -> Value {
    let buffer = Rc::new(ArrayBufferData::new(bytes.len()));
    buffer.bytes.borrow_mut().copy_from_slice(bytes);
    node_buffer_view(buffer, 0, bytes.len())
}

fn node_buffer_view(buffer: Rc<ArrayBufferData>, offset: usize, length: usize) -> Value {
    let value = quench_runtime::execute::set_property(
        Value::Uint8Array(Rc::new(Uint8ArrayData::new(buffer.clone(), offset, length))),
        "toString",
        capability_function(HostCapabilityKind::Custom(CapabilityName::BufferToString)),
    );
    let value = quench_runtime::execute::set_property(
        value,
        "equals",
        capability_function(HostCapabilityKind::Custom(CapabilityName::BufferEquals)),
    );
    let mut value = value;
    value =
        quench_runtime::execute::set_property(value, "parent", Value::ArrayBuffer(buffer.clone()));
    value = quench_runtime::execute::set_property(
        value,
        "constructor",
        Value::object(vec![("name".into(), Value::String("NodeBuffer".into()))]),
    );
    let inspect = capability_function(HostCapabilityKind::Custom(CapabilityName::BufferInspect));
    value = quench_runtime::execute::set_property(value, "inspect", inspect.clone());
    value = quench_runtime::execute::set_property(
        value,
        "Symbol.for.nodejs.util.inspect.custom\0",
        inspect,
    );
    for (name, capability) in [
        ("readBigInt64LE", CapabilityName::BufferReadBigInt64LE),
        ("readBigUInt64BE", CapabilityName::BufferReadBigUInt64BE),
        ("writeBigInt64LE", CapabilityName::BufferWriteBigInt64LE),
        ("writeBigUInt64BE", CapabilityName::BufferWriteBigUInt64BE),
    ] {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        );
    }
    for (name, capability) in [
        ("compare", CapabilityName::BufferCompare),
        ("indexOf", CapabilityName::BufferIndexOf),
        ("lastIndexOf", CapabilityName::BufferLastIndexOf),
        ("toJSON", CapabilityName::BufferToJson),
        ("swap16", CapabilityName::BufferSwap16),
        ("swap32", CapabilityName::BufferSwap32),
        ("swap64", CapabilityName::BufferSwap64),
    ] {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        );
    }
    for (name, capability) in [
        ("write", CapabilityName::BufferWrite),
        ("includes", CapabilityName::BufferIncludes),
        ("slice", CapabilityName::BufferSlice),
        ("subarray", CapabilityName::BufferSlice),
        ("copy", CapabilityName::BufferCopy),
        ("fill", CapabilityName::BufferFill),
    ] {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        );
    }
    for (index, name) in [
        "readDoubleBE",
        "readDoubleLE",
        "writeDoubleBE",
        "writeDoubleLE",
        "readFloatBE",
        "readFloatLE",
        "writeFloatBE",
        "writeFloatLE",
        "readUInt16BE",
        "readUInt16LE",
        "writeUInt16BE",
        "writeUInt16LE",
        "readUInt32BE",
        "readUInt32LE",
        "writeUInt32BE",
        "writeUInt32LE",
        "readUIntBE",
        "readUIntLE",
        "writeUIntBE",
        "writeUIntLE",
        "readInt16BE",
        "readInt16LE",
        "writeInt16BE",
        "writeInt16LE",
        "readIntBE",
        "readIntLE",
        "writeIntBE",
        "writeIntLE",
        "readUint32BE",
        "readUint32LE",
        "writeUintLE",
    ]
    .iter()
    .enumerate()
    {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::BufferNumericFirst + index as u16,
            )),
        );
    }
    for (name, index) in [
        ("_quenchReadUInt16BE", 8),
        ("_quenchReadUInt16LE", 9),
        ("_quenchWriteUInt16BE", 10),
        ("_quenchWriteUInt16LE", 11),
        ("_quenchReadInt16BE", 20),
        ("_quenchReadInt16LE", 21),
        ("_quenchWriteInt16BE", 22),
        ("_quenchWriteInt16LE", 23),
    ] {
        value = quench_runtime::execute::set_property(
            value,
            name,
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::BufferNumericFirst + index,
            )),
        );
    }
    let mut prototype = Value::object(vec![]);
    for (index, name) in [
        "readDoubleBE",
        "readDoubleLE",
        "writeDoubleBE",
        "writeDoubleLE",
        "readFloatBE",
        "readFloatLE",
        "writeFloatBE",
        "writeFloatLE",
        "readUInt16BE",
        "readUInt16LE",
        "writeUInt16BE",
        "writeUInt16LE",
        "readUInt32BE",
        "readUInt32LE",
        "writeUInt32BE",
        "writeUInt32LE",
        "readUIntBE",
        "readUIntLE",
        "writeUIntBE",
        "writeUIntLE",
        "readInt16BE",
        "readInt16LE",
        "writeInt16BE",
        "writeInt16LE",
        "readIntBE",
        "readIntLE",
        "writeIntBE",
        "writeIntLE",
    ]
    .iter()
    .enumerate()
    {
        prototype = quench_runtime::execute::set_property(
            prototype,
            name,
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::BufferNumericFirst + index as u16,
            )),
        );
    }
    let read_uint32_be = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 12,
    ));
    prototype =
        quench_runtime::execute::set_property(prototype, "readUInt32BE", read_uint32_be.clone());
    prototype = quench_runtime::execute::set_property(prototype, "readUint32BE", read_uint32_be);
    let write_uint_le = capability_function(HostCapabilityKind::Custom(
        CapabilityName::BufferNumericFirst + 19,
    ));
    prototype =
        quench_runtime::execute::set_property(prototype, "writeUIntLE", write_uint_le.clone());
    prototype = quench_runtime::execute::set_property(prototype, "writeUintLE", write_uint_le);
    value = quench_runtime::execute::set_property(value, "prototype", prototype);
    value
}
