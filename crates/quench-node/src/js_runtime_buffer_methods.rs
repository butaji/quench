fn buffer_to_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let mut bytes = string_or_bytes(receiver)?;
    let encoding = match arguments.first() {
        None | Some(Value::Undefined) => "utf8".into(),
        Some(Value::String(value)) => value.to_ascii_lowercase(),
        Some(Value::Object(_) | Value::ObjectAlias(_)) => {
            quench_runtime::execute::get_property_result(arguments.first().unwrap(), "toString")
                .ok()
                .and_then(|method| {
                    quench_runtime::execute::call(&method, arguments.first().unwrap(), &[]).ok()
                })
                .and_then(|value| match value {
                    Value::String(value) => Some(value.to_ascii_lowercase()),
                    _ => None,
                })
                .unwrap_or_else(|| "utf8".into())
        }
        Some(value) => {
            return Err(VmError::Thrown(fs_error(
                "ERR_UNKNOWN_ENCODING",
                &format!("Unknown encoding: {}", safe_value_string(value)),
            )))
        }
    };
    if !matches!(
        encoding.as_str(),
        "utf8" | "utf-8" | "hex" | "base64" | "base64url" | "ascii" | "utf16le" | "utf-16le"
    ) {
        return Err(VmError::Thrown(fs_error(
            "ERR_UNKNOWN_ENCODING",
            &format!("Unknown encoding: {encoding}"),
        )));
    }
    let start = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(value.max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(0)
        .min(bytes.len());
    let end = match arguments.get(2) {
        None | Some(Value::Undefined) => bytes.len(),
        Some(Value::Number(value)) => (*value).max(0.0) as usize,
        Some(_) => 0,
    }
    .min(bytes.len());
    bytes = if end >= start {
        bytes[start..end].to_vec()
    } else {
        Vec::new()
    };
    if encoding == "hex" {
        return Ok(Value::String(
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                .into(),
        ));
    }
    if encoding == "base64" {
        return Ok(Value::String(base64_encode(&bytes).into()));
    }
    if encoding == "base64url" {
        return Ok(Value::String(
            base64_encode(&bytes)
                .trim_end_matches('=')
                .replace('+', "-")
                .replace('/', "_")
                .into(),
        ));
    }
    if encoding == "ascii" {
        return Ok(Value::String(
            bytes
                .iter()
                .map(|byte| char::from(*byte & 0x7f))
                .collect::<String>()
                .into(),
        ));
    }
    if encoding == "utf16le" || encoding == "utf-16le" {
        let values = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        return Ok(Value::String(String::from_utf16_lossy(&values).into()));
    }
    Ok(Value::String(
        String::from_utf8_lossy(&bytes).into_owned().into(),
    ))
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(TABLE[((value >> 18) & 63) as usize] as char);
        output.push(TABLE[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn decode_hex(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in (0..text.len()).step_by(2) {
        if index + 1 >= text.len() {
            break;
        }
        let Ok(value) = u8::from_str_radix(&text[index..index + 2], 16) else {
            break;
        };
        bytes.push(value);
    }
    bytes
}

fn stream_iter_value(value: Option<&Value>) -> Result<Value, VmError> {
    match value.ok_or(VmError::NotCallable)? {
        Value::Promise(promise) => match &*promise.state.borrow() {
            quench_runtime::value::PromiseState::Fulfilled(value) => Ok(value.clone()),
            _ => Err(VmError::NotCallable),
        },
        value => Ok(value.clone()),
    }
}

fn stream_iter_text(arguments: &[Value]) -> Result<Value, VmError> {
    let value = stream_iter_value(arguments.first())?;
    let bytes = string_or_bytes(Some(&value))?;
    Ok(fulfilled(Value::String(
        String::from_utf8_lossy(&bytes).into_owned().into(),
    )))
}

fn stream_iter_bytes(arguments: &[Value]) -> Result<Value, VmError> {
    let value = stream_iter_value(arguments.first())?;
    Ok(fulfilled(match value {
        Value::Uint8Array(_) => value,
        Value::String(text) => node_buffer(text.as_bytes()),
        _ => quench_runtime::host_api::bytes(&[]),
    }))
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Number(_) => "number",
        Value::Boolean(_) => "boolean",
        Value::String(_) => "string",
        Value::Object(_) | Value::ObjectAlias(_) => "object",
        Value::Array(_) => "object",
        Value::Undefined => "undefined",
        Value::Null => "object",
        _ => "object",
    }
}

fn buffer_concat(arguments: &[Value]) -> Result<Value, VmError> {
    let list = arguments.first().cloned().unwrap_or(Value::Undefined);
    let Value::Array(_) = list else {
        let received = match &list {
            Value::Undefined => "undefined".into(),
            Value::Null => "null".into(),
            Value::Uint8Array(_) => "an instance of Buffer".into(),
            _ => format!("type {}", type_name(&list)),
        };
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            &format!("The \"list\" argument must be an instance of Array. Received {received}"),
        )));
    };
    let values = array_values(&list)?;
    if let Some(value) = arguments.get(1) {
        match value {
            Value::Number(value) if !value.is_finite() || value.fract() != 0.0 => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    "The \"length\" argument must be an integer",
                )));
            }
            Value::Number(value) if *value < 0.0 => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    "The \"length\" argument must be >= 0",
                )));
            }
            _ => {}
        }
    }
    let mut bytes = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if !matches!(value, Value::Uint8Array(_)) {
            let received = match value {
                Value::String(value) => format!("type string ('{}')", value),
                Value::Number(value) => format!("type number ({value})"),
                _ => format!("type {}", type_name(value)),
            };
            return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", &format!("The \"list[{index}]\" argument must be an instance of Buffer or Uint8Array. Received {received}"))));
        }
        bytes.extend(string_or_bytes(Some(value))?);
    }
    let length = arguments.get(1).and_then(|value| match value {
        Value::Number(value) => Some(*value as usize),
        _ => None,
    });
    if let Some(length) = length {
        let mut output = vec![0; length];
        output[..bytes.len().min(length)].copy_from_slice(&bytes[..bytes.len().min(length)]);
        return Ok(node_buffer(&output));
    }
    Ok(node_buffer(&bytes))
}

fn buffer_equals(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(other) = arguments.first() else {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "The \"otherBuffer\" argument must be an instance of Buffer or Uint8Array. Received undefined")));
    };
    if !matches!(other, Value::Uint8Array(_)) {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            &format!(
                "The \"otherBuffer\" argument must be an instance of Buffer or Uint8Array. {}",
                buffer_received(other)
            ),
        )));
    }
    Ok(Value::Boolean(
        string_or_bytes(receiver)? == string_or_bytes(arguments.first())?,
    ))
}

fn buffer_compare(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let other = if matches!(receiver, Some(Value::Uint8Array(_))) {
        arguments.first()
    } else {
        arguments.get(1)
    };
    if !matches!(other, Some(Value::Uint8Array(_))) {
        let value = other.cloned().unwrap_or(Value::Undefined);
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            &format!(
                "The \"buf2\" argument must be an instance of Buffer or Uint8Array. {}",
                buffer_received(&value)
            ),
        )));
    }
    let (left, right) = if matches!(receiver, Some(Value::Uint8Array(_))) {
        let left = string_or_bytes(receiver)?;
        let right_full = string_or_bytes(arguments.first())?;
        let target_start = match arguments.get(1) {
            Some(Value::Number(value)) => *value as usize,
            Some(_) => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "targetStart must be a number",
                )))
            }
            None => 0,
        };
        let target_end = match arguments.get(2) {
            Some(Value::Number(value)) => *value as usize,
            Some(_) => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "targetEnd must be a number",
                )))
            }
            None => right_full.len(),
        };
        let source_start = match arguments.get(3) {
            Some(Value::Number(value)) => *value as usize,
            Some(_) => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "sourceStart must be a number",
                )))
            }
            None => 0,
        };
        let source_end = match arguments.get(4) {
            Some(Value::Number(value)) => *value as usize,
            Some(_) => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "sourceEnd must be a number",
                )))
            }
            None => left.len(),
        };
        (
            left[source_start.min(left.len())..source_end.min(left.len())].to_vec(),
            right_full[target_start.min(right_full.len())..target_end.min(right_full.len())]
                .to_vec(),
        )
    } else {
        (
            string_or_bytes(arguments.first())?,
            string_or_bytes(arguments.get(1))?,
        )
    };
    Ok(Value::Number(if left < right {
        -1.0
    } else if left > right {
        1.0
    } else {
        0.0
    }))
}

fn buffer_received(value: &Value) -> String {
    match value {
        Value::String(value) => format!("Received type string ('{value}')"),
        Value::Number(value) => format!("Received type number ({value})"),
        Value::Null => "Received null".into(),
        Value::Undefined => "Received undefined".into(),
        _ => format!("Received {}", type_name(value)),
    }
}

fn buffer_search(
    receiver: Option<&Value>,
    arguments: &[Value],
    reverse: bool,
) -> Result<Value, VmError> {
    let haystack = string_or_bytes(receiver)?;
    let needle = match arguments.first() {
        Some(Value::Number(value)) => vec![*value as u8],
        value => string_or_bytes(value)?,
    };
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value as isize).max(0) as usize),
            _ => None,
        })
        .unwrap_or(if reverse { haystack.len() } else { 0 });
    if needle.is_empty() {
        return Ok(Value::Number(offset.min(haystack.len()) as f64));
    }
    let result = if reverse {
        haystack[..offset.min(haystack.len())]
            .windows(needle.len())
            .rposition(|window| window == needle.as_slice())
    } else {
        haystack[offset.min(haystack.len())..]
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
            .map(|index| index + offset.min(haystack.len()))
    };
    Ok(Value::Number(result.map_or(-1.0, |index| index as f64)))
}

fn buffer_to_json(receiver: Option<&Value>) -> Result<Value, VmError> {
    let bytes = string_or_bytes(receiver)?;
    Ok(quench_runtime::host_api::object(vec![
        ("type".into(), Value::String("Buffer".into())),
        (
            "data".into(),
            quench_runtime::host_api::array(
                bytes
                    .into_iter()
                    .map(|byte| Value::Number(byte as f64))
                    .collect(),
            ),
        ),
    ]))
}

fn buffer_swap(receiver: Option<&Value>, width: usize) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    if view.length % width != 0 {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_BUFFER_SIZE",
            "Buffer size must be a multiple of the element size",
        )));
    }
    let mut bytes = view.buffer.bytes.borrow_mut();
    let range = &mut bytes[view.byte_offset..view.byte_offset + view.length];
    for chunk in range.chunks_exact_mut(width) {
        chunk.reverse();
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn buffer_copy_bytes_from(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(source) = arguments.first() else {
        return Err(VmError::NotCallable);
    };
    let (bytes, element_size) = match source {
        Value::Uint8Array(view) => (
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec(),
            1,
        ),
        Value::Uint16Array(view) => (
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length * 2]
                .to_vec(),
            2,
        ),
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "source must be a typed array",
            )))
        }
    };
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value).max(0.0) as usize * element_size),
            _ => None,
        })
        .unwrap_or(0)
        .min(bytes.len());
    let length = arguments
        .get(2)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value).max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(bytes.len() - offset)
        .min(bytes.len() - offset);
    Ok(node_buffer(&bytes[offset..offset + length]))
}

fn buffer_bigint(
    receiver: Option<&Value>,
    arguments: &[Value],
    unsigned: bool,
    little: bool,
) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let write = matches!(arguments.first(), Some(Value::BigInt(_)));
    let offset = if write { 1 } else { 0 };
    let offset = arguments
        .get(offset)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value).max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(0);
    if offset + 8 > view.length {
        return Err(VmError::Thrown(fs_error(
            "ERR_BUFFER_OUT_OF_BOUNDS",
            "offset out of bounds",
        )));
    }
    let mut bytes = view.buffer.bytes.borrow_mut();
    let slice = &mut bytes[view.byte_offset + offset..view.byte_offset + offset + 8];
    if write {
        let value = match arguments.first() {
            Some(Value::BigInt(value)) if unsigned => value.parse::<u64>().unwrap_or(0),
            Some(Value::BigInt(value)) => value.parse::<i64>().unwrap_or(0) as u64,
            _ => return Err(VmError::NotCallable),
        };
        let encoded = if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        slice.copy_from_slice(&encoded);
        Ok(Value::Number((offset + 8) as f64))
    } else {
        let mut encoded = [0u8; 8];
        encoded.copy_from_slice(slice);
        let value = if little {
            u64::from_le_bytes(encoded)
        } else {
            u64::from_be_bytes(encoded)
        };
        let value = if unsigned {
            value as i128
        } else {
            value as i64 as i128
        };
        Ok(Value::BigInt(value.to_string()))
    }
}
