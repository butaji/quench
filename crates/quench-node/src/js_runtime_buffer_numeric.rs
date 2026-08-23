fn buffer_numeric(
    id: u16,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let index = id - CapabilityName::BufferNumericFirst;
    let is_write = matches!(
        index,
        2 | 3 | 6 | 7 | 10 | 11 | 14 | 15 | 18 | 19 | 22 | 23 | 26 | 27 | 28
    );
    let variable = matches!(index, 16 | 17 | 18 | 19 | 24 | 25 | 26 | 27);
    let offset_arg = if is_write { 1 } else { 0 };
    let offset_value = match arguments.get(offset_arg) {
        Some(Value::Number(value)) => *value,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "offset must be a number",
            )))
        }
    };
    let size = if variable {
        match arguments.get(offset_arg + 1) {
            Some(Value::Number(value))
                if *value >= 1.0 && *value <= 6.0 && value.fract() == 0.0 =>
            {
                *value as usize
            }
            _ => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    "byteLength out of range",
                )))
            }
        }
    } else if index <= 3 {
        8
    } else if index <= 7 {
        4
    } else if index <= 11 || (index >= 20 && index <= 23) {
        2
    } else {
        4
    };
    let maximum_offset = view.length.saturating_sub(size);
    let offset_display = if offset_value.is_infinite() {
        if offset_value.is_sign_negative() {
            "-Infinity".into()
        } else {
            "Infinity".into()
        }
    } else {
        offset_value.to_string()
    };
    if !offset_value.is_finite() || offset_value.fract() != 0.0 {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            &format!("The value of \"offset\" is out of range. It must be an integer. Received {offset_display}"),
        )));
    }
    if offset_value < 0.0 || offset_value as usize > maximum_offset {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            &format!("The value of \"offset\" is out of range. It must be >= 0 and <= {maximum_offset}. Received {offset_display}"),
        )));
    }
    let offset = offset_value as usize;
    if offset + size > view.length {
        return Err(VmError::Thrown(fs_error(
            "ERR_BUFFER_OUT_OF_BOUNDS",
            "offset out of bounds",
        )));
    }
    let little = matches!(
        index,
        1 | 3 | 5 | 7 | 9 | 11 | 13 | 15 | 17 | 19 | 21 | 23 | 25 | 27
    );
    let mut bytes = view.buffer.bytes.borrow_mut();
    let slice = &mut bytes[view.byte_offset + offset..view.byte_offset + offset + size];
    if is_write {
        let value = match arguments.first() {
            Some(Value::Number(value)) => *value,
            _ => return Err(VmError::NotCallable),
        };
        let range = match index {
            10 | 11 => Some((0.0, 65535.0)),
            14 | 15 => Some((0.0, 4_294_967_295.0)),
            22 | 23 => Some((-32_768.0, 32_767.0)),
            _ => None,
        };
        if let Some((minimum, maximum)) = range {
            if !value.is_finite() || value.fract() != 0.0 || value < minimum || value > maximum {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    &format!("The value of \"value\" is out of range. It must be >= {minimum} and <= {maximum}. Received {}", value),
                )));
            }
        }
        if index <= 3 || (index >= 6 && index <= 7) {
            if index <= 3 {
                let data = if little {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                };
                slice.copy_from_slice(&data);
            } else {
                let raw = (value as f32).to_bits();
                let data = if little {
                    raw.to_le_bytes()
                } else {
                    raw.to_be_bytes()
                };
                slice.copy_from_slice(&data);
            }
        } else {
            let mut raw = if index >= 20 && index <= 27 {
                (value as i64) as u64
            } else {
                value as u64
            };
            for byte in slice.iter_mut().rev() {
                *byte = (raw & 0xff) as u8;
                raw >>= 8;
            }
            if little {
                slice.reverse();
            }
        }
        Ok(Value::Number((offset + size) as f64))
    } else {
        let mut raw = 0u64;
        if little {
            for (shift, byte) in slice.iter().enumerate() {
                raw |= u64::from(*byte) << (shift * 8);
            }
        } else {
            for byte in slice.iter() {
                raw = (raw << 8) | u64::from(*byte);
            }
        }
        let value = if index <= 1 {
            f64::from_bits(raw)
        } else if index >= 4 && index <= 5 {
            f32::from_bits(raw as u32) as f64
        } else if index <= 7 {
            raw as f64
        } else if index >= 20 && index <= 27 {
            let bits = size * 8;
            let signed = if raw & (1 << (bits - 1)) != 0 {
                raw as i64 - (1i64 << bits)
            } else {
                raw as i64
            };
            signed as f64
        } else {
            raw as f64
        };
        Ok(Value::Number(value))
    }
}

fn buffer_write(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let text = match arguments.first() {
        Some(Value::String(value)) => value,
        _ => return Err(VmError::NotCallable),
    };
    if matches!(arguments.get(1), Some(Value::String(_))) {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "offset must be a number",
        )));
    }
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0);
    let encoding = arguments
        .get(if matches!(arguments.get(2), Some(Value::Number(_))) {
            3
        } else {
            2
        })
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("utf8");
    if !matches!(
        encoding.to_ascii_lowercase().as_str(),
        "utf8" | "utf-8" | "hex" | "utf16le" | "ucs2" | "ucs-2"
    ) {
        return Err(VmError::Thrown(fs_error(
            "ERR_UNKNOWN_ENCODING",
            "Unknown encoding",
        )));
    }
    let bytes = if encoding == "hex" {
        (0..text.len())
            .step_by(2)
            .take_while(|index| *index + 1 < text.len())
            .filter_map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
            .collect::<Vec<_>>()
    } else if encoding == "utf16le" || encoding == "ucs2" || encoding == "ucs-2" {
        text.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        text.as_bytes().to_vec()
    };
    let count = bytes.len().min(view.length.saturating_sub(offset));
    view.buffer.bytes.borrow_mut()[view.byte_offset + offset..view.byte_offset + offset + count]
        .copy_from_slice(&bytes[..count]);
    Ok(Value::Number(count as f64))
}

fn buffer_includes(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let haystack = string_or_bytes(receiver)?;
    let needle = string_or_bytes(arguments.first()).or_else(|_| match arguments.first() {
        Some(Value::Number(value)) => Ok(vec![*value as u8]),
        _ => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "value must be a string, Buffer, or number",
        ))),
    })?;
    if needle.is_empty() {
        return Ok(Value::Boolean(true));
    }
    let offset = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value as isize).max(0) as usize),
            _ => None,
        })
        .unwrap_or(0);
    Ok(Value::Boolean(
        offset <= haystack.len()
            && haystack[offset..]
                .windows(needle.len())
                .any(|window| window == needle),
    ))
}

fn buffer_slice(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let bytes = string_or_bytes(receiver)?;
    let start = arguments
        .first()
        .and_then(|value| match value {
            Value::Number(value) => Some(if *value < 0.0 {
                bytes.len().saturating_sub((-*value) as usize)
            } else {
                *value as usize
            }),
            _ => None,
        })
        .unwrap_or(0)
        .min(bytes.len());
    let end = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(if *value < 0.0 {
                bytes.len().saturating_sub((-*value) as usize)
            } else {
                *value as usize
            }),
            _ => None,
        })
        .unwrap_or(bytes.len())
        .min(bytes.len());
    let Some(Value::Uint8Array(source)) = receiver else {
        return Ok(node_buffer(&bytes[start.min(end)..end]));
    };
    Ok(node_buffer_view(
        source.buffer.clone(),
        source.byte_offset + start.min(end),
        end.saturating_sub(start.min(end)),
    ))
}

fn buffer_copy(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let source = string_or_bytes(receiver)?;
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let (target_buffer, target_offset, target_bytes) = match target {
        Value::Uint8Array(target) => (target.buffer.clone(), target.byte_offset, target.length),
        Value::Uint16Array(target) => {
            (target.buffer.clone(), target.byte_offset, target.length * 2)
        }
        Value::Uint32Array(target) => {
            (target.buffer.clone(), target.byte_offset, target.length * 4)
        }
        _ => return Err(VmError::NotCallable),
    };
    let target_start = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0);
    let source_start = arguments
        .get(2)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0);
    let source_end = arguments
        .get(3)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(source.len())
        .min(source.len());
    let count = source_end
        .saturating_sub(source_start)
        .min(target_bytes.saturating_sub(target_start));
    target_buffer.bytes.borrow_mut()
        [target_offset + target_start..target_offset + target_start + count]
        .copy_from_slice(&source[source_start..source_start + count]);
    Ok(Value::Number(count as f64))
}

fn buffer_fill(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Value::Uint8Array(view) = receiver.ok_or(VmError::NotCallable)? else {
        return Err(VmError::NotCallable);
    };
    let mut fill = string_or_bytes(arguments.first()).or_else(|_| match arguments.first() {
        Some(Value::Null) | Some(Value::Undefined) => Ok(vec![0]),
        Some(Value::Number(value)) => Ok(vec![*value as u8]),
        _ => Err(VmError::NotCallable),
    })?;
    let encoding_index = if matches!(arguments.get(1), Some(Value::String(_))) {
        1
    } else {
        3
    };
    if matches!(arguments.get(encoding_index), Some(Value::String(encoding)) if encoding.eq_ignore_ascii_case("hex"))
    {
        let Some(Value::String(value)) = arguments.first() else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_VALUE",
                "invalid hex fill",
            )));
        };
        let decoded = decode_hex(value);
        if decoded.len() * 2 != value.len() {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_VALUE",
                "invalid hex fill",
            )));
        }
        fill = decoded;
    }
    if fill.is_empty() {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    if arguments
        .get(1)
        .is_some_and(|value| !matches!(value, Value::Number(_)))
        || arguments
            .get(2)
            .is_some_and(|value| !matches!(value, Value::Number(_) | Value::String(_)))
    {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "range must be numeric",
        )));
    }
    let start = arguments
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(0)
        .min(view.length);
    let end = arguments
        .get(2)
        .and_then(|value| match value {
            Value::Number(value) => Some(*value as usize),
            _ => None,
        })
        .unwrap_or(view.length)
        .min(view.length);
    let mut bytes = view.buffer.bytes.borrow_mut();
    for (index, byte) in bytes[view.byte_offset + start..view.byte_offset + end]
        .iter_mut()
        .enumerate()
    {
        *byte = fill[index % fill.len()];
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn buffer_is_buffer(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(matches!(
        arguments.first(),
        Some(Value::Uint8Array(_))
    )))
}

fn buffer_is_ascii(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        string_or_bytes(arguments.first())?
            .iter()
            .all(|byte| *byte < 0x80),
    ))
}

fn buffer_is_utf8(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        std::str::from_utf8(&string_or_bytes(arguments.first())?).is_ok(),
    ))
}

fn text_encoder_constructor() -> Result<Value, VmError> {
    Ok(quench_runtime::host_api::object(vec![(
        "encode".into(),
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::TextEncoderEncode,
        )),
    )]))
}

fn text_encoder_encode(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(value)) = arguments.first() else {
        return Ok(quench_runtime::host_api::bytes(&[]));
    };
    Ok(quench_runtime::host_api::bytes(value.as_bytes()))
}

fn text_decoder_constructor() -> Result<Value, VmError> {
    Ok(quench_runtime::host_api::object(vec![(
        "decode".into(),
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::TextDecoderDecode,
        )),
    )]))
}

fn text_decoder_decode(arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(
        String::from_utf8_lossy(&string_or_bytes(arguments.first())?).into(),
    ))
}

fn buffer_inspect(receiver: Option<&Value>) -> Result<Value, VmError> {
    let bytes = string_or_bytes(receiver)?;
    let shown = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Value::String(format!("<Buffer {shown}>").into()))
}
