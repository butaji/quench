//! `string_decoder` module — UTF-8 byte-to-string converter.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn new_decoder(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let requested = match _args.first() {
        None | Some(Value::Undefined) => "utf8".to_string(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Boolean(value)) => value.to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(_) => "[object Object]".to_string(),
    };
    let encoding = crate::modules::buffer_enc::canonical_encoding(&requested)
        .ok_or_else(|| crate::modules::buffer_enc::unknown_encoding(&requested))?;
    let decoder_id = {
        let mut state = state.borrow_mut();
        let id = state.string_decoder_next_id;
        state.string_decoder_next_id = state.string_decoder_next_id.saturating_add(1);
        state.string_decoder_pending.insert(id, Vec::new());
        state
            .string_decoder_encoding
            .insert(id, encoding.to_string());
        id
    };
    let mut props = Vec::new();
    props.push(("\0decoder_id".to_string(), Value::Number(decoder_id as f64)));
    props.push(("\0pending".to_string(), host_api::bytes(&[])));
    props.push(("encoding".to_string(), Value::String(encoding.into())));
    props.push(("lastNeed".to_string(), Value::Number(0.0)));
    props.push(("lastTotal".to_string(), Value::Number(0.0)));
    props.push((
        "lastChar".to_string(),
        crate::modules::buffer_proto::make_buffer(&[0, 0, 0, 0]),
    ));
    props.push((
        "write".to_string(),
        crate::host::capability(crate::registry::SPEC_STRING_DECODER_WRITE),
    ));
    props.push((
        "end".to_string(),
        crate::host::capability(crate::registry::SPEC_STRING_DECODER_END),
    ));
    props.push((
        "text".to_string(),
        crate::host::capability(crate::registry::SPEC_STRING_DECODER_TEXT),
    ));
    let prototype = host_api::object(vec![
        (
            "write".to_string(),
            crate::host::capability(crate::registry::SPEC_STRING_DECODER_WRITE),
        ),
        (
            "end".to_string(),
            crate::host::capability(crate::registry::SPEC_STRING_DECODER_END),
        ),
        (
            "text".to_string(),
            crate::host::capability(crate::registry::SPEC_STRING_DECODER_TEXT),
        ),
    ]);
    props.push(("\0prototype".to_string(), prototype.clone()));
    props.push(("__proto__".to_string(), prototype));
    Ok(host_api::object(props))
}

pub fn write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let input = args.first().ok_or(VmError::NotCallable)?;
    let key = decoder_key(state, receiver)?;
    let mut bytes = state
        .borrow()
        .string_decoder_pending
        .get(&key)
        .cloned()
        .unwrap_or_default();
    let had_pending = !bytes.is_empty();
    bytes.extend(input_bytes(input)?);
    if bytes.len() > 0x1fffffe8 {
        return Err(crate::modules::buffer_enc::string_too_long());
    }
    let encoding = decoder_encoding(state, key, receiver);
    let (text, pending) = decode_chunk(&bytes, &encoding, had_pending, true);
    state
        .borrow_mut()
        .string_decoder_pending
        .insert(key, pending.clone());
    let total: usize = if encoding == "utf8" {
        bytes
            .first()
            .map(|byte| match byte {
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => 0,
            })
            .unwrap_or(0)
    } else if encoding == "utf16le" {
        2
    } else {
        0
    };
    if total != 0 || !pending.is_empty() {
        let pending_value = host_api::bytes(&pending);
        let updated =
            quench_runtime::execute::set_property(receiver.clone(), "\0pending", pending_value);
        let updated = quench_runtime::execute::set_property(
            updated,
            "lastNeed",
            Value::Number(total.saturating_sub(pending.len()) as f64),
        );
        let updated = quench_runtime::execute::set_property(
            updated,
            "lastTotal",
            Value::Number(total as f64),
        );
        let mut last_char = pending;
        last_char.resize(4, 0);
        let updated = quench_runtime::execute::set_property(
            updated,
            "lastChar",
            crate::modules::buffer_proto::make_buffer(&last_char[..4]),
        );
        quench_runtime::execute::replace_value(receiver, &updated);
    }
    let _ = (state, key);
    Ok(text)
}

fn input_bytes(value: &Value) -> Result<Vec<u8>, VmError> {
    match value {
        Value::Float64Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 8),
        Value::Float32Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 4),
        Value::Int8Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length),
        Value::Int16Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 2),
        Value::Int32Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 4),
        Value::BigInt64Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 8),
        Value::BigUint64Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 8),
        Value::Uint32Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 4),
        Value::Uint8Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length),
        Value::Uint8ClampedArray(view) => view_bytes(&view.buffer, view.byte_offset, view.length),
        Value::Uint16Array(view) => view_bytes(&view.buffer, view.byte_offset, view.length * 2),
        Value::DataView(view) => view_bytes(&view.buffer, view.byte_offset, view.byte_length),
        Value::ArrayBuffer(buffer) => Ok(buffer.bytes.borrow().clone()),
        _ => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"buf\" argument must be an instance of Buffer, TypedArray, or DataView. Received {}",
            received_type(value),
        ))),
    }
}

fn received_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Undefined => "undefined",
        Value::Boolean(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) | Value::StringUnits(_) => "a string",
        _ => "an invalid value",
    }
}

fn view_bytes(
    buffer: &Rc<quench_runtime::value::ArrayBufferData>,
    offset: usize,
    length: usize,
) -> Result<Vec<u8>, VmError> {
    let bytes = buffer.bytes.borrow();
    let end = offset.checked_add(length).ok_or(VmError::NotCallable)?;
    bytes
        .get(offset..end)
        .map(ToOwned::to_owned)
        .ok_or(VmError::NotCallable)
}

fn decode_chunk(
    bytes: &[u8],
    encoding: &str,
    preserve_trailing_high: bool,
    streaming: bool,
) -> (Value, Vec<u8>) {
    match encoding {
        "base64" | "base64url" => {
            // Streaming base64 emits complete three-byte groups and keeps
            // the tail for the next write (or end(), where padding is added).
            let complete = bytes.len() / 3 * 3;
            let text = crate::modules::buffer_enc::decode_str(&bytes[..complete], encoding);
            (text, bytes[complete..].to_vec())
        }
        "hex" => {
            let complete = bytes.len() / 2 * 2;
            let text = crate::modules::buffer_enc::decode_str(&bytes[..complete], encoding);
            (text, bytes[complete..].to_vec())
        }
        "latin1" => (
            Value::String(bytes.iter().map(|byte| *byte as char).collect()),
            Vec::new(),
        ),
        "ascii" => (
            Value::String(bytes.iter().map(|byte| (byte & 0x7f) as char).collect()),
            Vec::new(),
        ),
        "utf16le" => {
            let mut complete = bytes.len() / 2 * 2;
            if complete >= 2 {
                let last = u16::from_le_bytes([bytes[complete - 2], bytes[complete - 1]]);
                if (0xd800..=0xdbff).contains(&last)
                    && (preserve_trailing_high || bytes.len() == complete)
                {
                    complete -= 2;
                }
            }
            let units = bytes[..complete]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            (
                quench_runtime::execute::string_from_units(units),
                bytes[complete..].to_vec(),
            )
        }
        _ => decode_utf8(bytes, preserve_trailing_high, streaming),
    }
}

fn decode_utf8(bytes: &[u8], had_pending: bool, streaming: bool) -> (Value, Vec<u8>) {
    if streaming
        && !had_pending
        && bytes.len() < 3
        && bytes
            .first()
            .is_some_and(|byte| (0xF5..=0xFF).contains(byte))
    {
        return (Value::String(String::new()), bytes.to_vec());
    }
    let mut rest = bytes;
    let mut text = String::new();
    loop {
        match std::str::from_utf8(rest) {
            Ok(value) => {
                text.push_str(value);
                return (Value::String(text), Vec::new());
            }
            Err(error) if error.error_len().is_none() => {
                let valid = error.valid_up_to();
                text.push_str(&String::from_utf8_lossy(&rest[..valid]));
                return (Value::String(text), rest[valid..].to_vec());
            }
            Err(error) => {
                let valid = error.valid_up_to();
                text.push_str(&String::from_utf8_lossy(&rest[..valid]));
                text.push('�');
                let skip = valid + error.error_len().unwrap_or(1);
                rest = &rest[skip.min(rest.len())..];
            }
        }
    }
}

pub fn end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !args.is_empty() {
        let receiver = receiver.ok_or(VmError::NotCallable)?;
        let key = decoder_key(state, receiver)?;
        let encoding = decoder_encoding(state, key, receiver);
        if encoding == "utf16le" {
            let pending = state
                .borrow()
                .string_decoder_pending
                .get(&key)
                .cloned()
                .unwrap_or_default();
            let high = pending.len() >= 2
                && (0xd800..=0xdbff).contains(&u16::from_le_bytes([pending[0], pending[1]]));
            if high && pending.len() > 2 {
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0pending",
                    host_api::bytes(&[]),
                );
                quench_runtime::execute::replace_value(receiver, &updated);
                state
                    .borrow_mut()
                    .string_decoder_pending
                    .insert(key, Vec::new());
                return Ok(Value::StringUnits(Rc::new(
                    quench_runtime::value::StringUnitsData::new(vec![u16::from_le_bytes([
                        pending[0], pending[1],
                    ])]),
                )));
            }
            // Keep any incomplete code unit in the decoder state while
            // decoding the supplied final bytes. `write()` prepends that
            // state, which is required for an odd UTF-16 byte split (for
            // example `41 00 42` followed by `00`).
            return write(state, Some(receiver), args);
        }
        let prefix = end(state, Some(receiver), &[])?;
        let suffix = write(state, Some(receiver), args)?;
        let mut units = value_units(&prefix);
        units.extend(value_units(&suffix));
        return Ok(quench_runtime::execute::string_from_units(units));
    }
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let key = decoder_key(state, receiver)?;
    let bytes = state
        .borrow()
        .string_decoder_pending
        .get(&key)
        .cloned()
        .unwrap_or_default();
    let updated =
        quench_runtime::execute::set_property(receiver.clone(), "\0pending", host_api::bytes(&[]));
    quench_runtime::execute::replace_value(receiver, &updated);
    state
        .borrow_mut()
        .string_decoder_pending
        .insert(key, Vec::new());
    let encoding = decoder_encoding(state, key, receiver);
    if matches!(encoding.as_str(), "base64" | "base64url" | "hex") {
        return Ok(crate::modules::buffer_enc::decode_str(&bytes, &encoding));
    }
    let (text, pending) = decode_chunk(&bytes, &encoding, false, false);
    if pending.is_empty() {
        return Ok(text);
    }
    if encoding == "utf16le" {
        let units = pending
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let mut all = value_units(&text);
        all.extend(units);
        return Ok(quench_runtime::execute::string_from_units(all));
    }
    let mut all = value_units(&text);
    all.push(0xfffd);
    Ok(quench_runtime::execute::string_from_units(all))
}

fn value_units(value: &Value) -> Vec<u16> {
    match value {
        Value::String(value) => value.encode_utf16().collect(),
        Value::StringUnits(value) => value.iter().copied().collect(),
        _ => Vec::new(),
    }
}

fn decoder_encoding(state: &Rc<RefCell<HostState>>, key: u64, receiver: &Value) -> String {
    state
        .borrow()
        .string_decoder_encoding
        .get(&key)
        .cloned()
        .or_else(|| {
            quench_runtime::execute::get_property_result(receiver, "encoding")
                .ok()
                .and_then(|value| match value {
                    Value::String(value) => Some(value),
                    _ => None,
                })
        })
        .unwrap_or_else(|| "utf8".into())
}

pub fn text(_receiver: Option<&Value>, args: &[Value]) -> Result<Value, VmError> {
    let input = args.first().ok_or(VmError::NotCallable)?;
    let offset = args
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) => Some((*value).max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(0);
    let bytes = input_bytes(input)?;
    if offset >= bytes.len() {
        return Ok(Value::String(String::new()));
    }
    Ok(Value::String(
        String::from_utf8_lossy(&bytes[offset..]).into_owned(),
    ))
}

fn decoder_key(state: &Rc<RefCell<HostState>>, receiver: &Value) -> Result<u64, VmError> {
    if let Ok(Value::Number(id)) =
        quench_runtime::execute::get_property_result(receiver, "\0decoder_id")
    {
        return Ok(id as u64);
    }
    let mut key = receiver
        .object_identity()
        .ok_or_else(crate::modules::buffer_enc::invalid_this)?;
    for _ in 0..8 {
        let Some(next) = state.borrow().string_decoder_aliases.get(&key).copied() else {
            break;
        };
        if next == key {
            break;
        }
        key = next;
    }
    if key == receiver.object_identity().unwrap_or_default() {
        return Err(crate::modules::buffer_enc::invalid_this());
    }
    Ok(key)
}

pub fn call(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let target = args.first().ok_or(VmError::NotCallable)?.clone();
    let object = new_decoder(state, &args[1..])?;
    quench_runtime::execute::replace_value(&target, &object);
    for key in [
        "\0decoder_id",
        "encoding",
        "lastNeed",
        "lastTotal",
        "lastChar",
        "write",
        "end",
        "text",
    ] {
        if let Ok(value) = quench_runtime::execute::get_property_result(&object, key) {
            let _ = quench_runtime::execute::set_property(target.clone(), key, value);
        }
    }
    Ok(target)
}

pub struct StringDecoder {
    pub buffer: Vec<u8>,
}

impl Default for StringDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StringDecoder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }
}

pub fn build() -> Vec<(String, Value)> {
    let constructor = crate::host::capability(crate::registry::SPEC_STRING_DECODER);
    let call = crate::host::capability(crate::registry::SPEC_STRING_DECODER_CALL);
    let _ = quench_runtime::execute::set_host_capability_property(&constructor, "call", call);
    vec![("StringDecoder".to_string(), constructor)]
}
