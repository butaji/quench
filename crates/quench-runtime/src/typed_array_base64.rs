use crate::{execute::VmError, ops::Builtin, value::Value};
use std::rc::Rc;

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

#[derive(Clone, Copy, PartialEq)]
enum LastChunk {
    Loose,
    Strict,
    StopBeforePartial,
}

#[derive(Clone, Copy)]
struct Base64Options {
    url_safe: bool,
    last_chunk: LastChunk,
}

struct Decoded {
    bytes: Vec<u8>,
    read: usize,
    failed: bool,
}

pub(crate) fn execute(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::Uint8ArrayFromBase64 => Some(from_base64(arguments)),
        Builtin::Uint8ArrayFromHex => Some(from_hex(arguments)),
        Builtin::Uint8ArraySetFromBase64 => Some(set_from_base64(receiver, arguments)),
        Builtin::Uint8ArraySetFromHex => Some(set_from_hex(receiver, arguments)),
        Builtin::Uint8ArrayToBase64 => Some(to_base64(receiver, arguments)),
        Builtin::Uint8ArrayToHex => Some(to_hex(receiver)),
        Builtin::Uint8ArraySubarray => Some(subarray(receiver, arguments)),
        _ => None,
    }
}

fn syntax_error() -> VmError {
    crate::value::error::throw_syntax_error("invalid base64/hex input")
}

fn string_argument(arguments: &[Value]) -> Result<String, VmError> {
    match arguments.first() {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::StringUnits(units)) => Ok(String::from_utf16_lossy(units)),
        _ => Err(crate::value::error::throw_type_error(
            "Uint8Array base64/hex input must be a string",
        )),
    }
}

fn option_string(options: &Value, key: &str) -> Result<Option<String>, VmError> {
    let value = crate::vm::get_property_result(options, key)?;
    Ok(match value {
        Value::Undefined => None,
        Value::String(text) => Some(text),
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Uint8Array option must be a string",
            ))
        }
    })
}

fn option_object(arguments: &[Value]) -> Result<Option<&Value>, VmError> {
    let Some(options) = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
    else {
        return Ok(None);
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error(
            "Uint8Array options must be an object",
        ));
    }
    Ok(Some(options))
}

fn alphabet_option(options: Option<&Value>) -> Result<bool, VmError> {
    let Some(options) = options else {
        return Ok(false);
    };
    match option_string(options, "alphabet")?.as_deref() {
        None | Some("base64") => Ok(false),
        Some("base64url") => Ok(true),
        _ => Err(crate::value::error::throw_type_error(
            "Invalid Uint8Array alphabet option",
        )),
    }
}

fn base64_options(arguments: &[Value]) -> Result<Base64Options, VmError> {
    let options = option_object(arguments)?;
    let url_safe = alphabet_option(options)?;
    let last_chunk = match options {
        None => LastChunk::Loose,
        Some(options) => match option_string(options, "lastChunkHandling")?.as_deref() {
            None | Some("loose") => LastChunk::Loose,
            Some("strict") => LastChunk::Strict,
            Some("stop-before-partial") => LastChunk::StopBeforePartial,
            _ => {
                return Err(crate::value::error::throw_type_error(
                    "Invalid Uint8Array lastChunkHandling option",
                ))
            }
        },
    };
    Ok(Base64Options {
        url_safe,
        last_chunk,
    })
}

fn uint8_array_from_bytes(bytes: &[u8]) -> Value {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(bytes.len()));
    buffer.bytes.borrow_mut().copy_from_slice(bytes);
    Value::Uint8Array(Rc::new(crate::value::Uint8ArrayData::new(
        buffer,
        0,
        bytes.len(),
    )))
}

fn from_base64(arguments: &[Value]) -> Result<Value, VmError> {
    let input = string_argument(arguments)?;
    let options = base64_options(arguments)?;
    let decoded = decode_base64(&input, options, usize::MAX);
    if decoded.failed {
        return Err(syntax_error());
    }
    Ok(uint8_array_from_bytes(&decoded.bytes))
}

fn from_hex(arguments: &[Value]) -> Result<Value, VmError> {
    let input = string_argument(arguments)?;
    let decoded = decode_hex(&input, usize::MAX);
    if decoded.failed {
        return Err(syntax_error());
    }
    Ok(uint8_array_from_bytes(&decoded.bytes))
}

fn uint8_receiver(receiver: Option<&Value>) -> Result<&crate::value::Uint8ArrayData, VmError> {
    let Some(Value::Uint8Array(view)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Uint8Array base64/hex method requires a Uint8Array receiver",
        ));
    };
    Ok(view)
}

fn check_detached(view: &crate::value::Uint8ArrayData) -> Result<(), VmError> {
    if *view.buffer.detached.borrow() {
        return Err(crate::value::error::throw_type_error(
            "Uint8Array buffer is detached",
        ));
    }
    Ok(())
}

fn check_immutable(view: &crate::value::Uint8ArrayData) -> Result<(), VmError> {
    if view.buffer.immutable {
        return Err(crate::value::error::throw_type_error(
            "Uint8Array buffer is immutable",
        ));
    }
    Ok(())
}

fn relative_index(value: Option<&Value>, length: usize) -> Result<usize, VmError> {
    let Some(value) = value else {
        return Ok(length);
    };
    let index = crate::conversion::to_number(value)?.trunc() as i64;
    let length = length as i64;
    Ok(if index < 0 {
        (length + index).max(0) as usize
    } else {
        index.min(length) as usize
    })
}

fn subarray(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let view = uint8_receiver(receiver)?;
    let length = view.logical_len();
    let begin = match arguments.first() {
        None | Some(Value::Undefined) => 0,
        Some(_) => relative_index(arguments.first(), length)?,
    };
    let end = relative_index(arguments.get(1), length)?;
    let view = crate::value::Uint8ArrayData::new(
        view.buffer.clone(),
        view.byte_offset + begin,
        end.saturating_sub(begin),
    );
    Ok(Value::Uint8Array(Rc::new(view)))
}

fn read_written_object(read: usize, written: usize) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("read".to_string(), Value::Number(read as f64)),
        ("written".to_string(), Value::Number(written as f64)),
    ])))
}

fn write_decoded(view: &crate::value::Uint8ArrayData, decoded: Decoded) -> Result<Value, VmError> {
    for (index, byte) in decoded.bytes.iter().enumerate() {
        view.set(index, *byte);
    }
    if decoded.failed {
        return Err(syntax_error());
    }
    Ok(read_written_object(decoded.read, decoded.bytes.len()))
}

fn set_from_base64(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let view = uint8_receiver(receiver)?;
    let input = string_argument(arguments)?;
    check_immutable(view)?;
    let options = base64_options(arguments)?;
    check_detached(view)?;
    let decoded = decode_base64(&input, options, view.logical_len());
    write_decoded(view, decoded)
}

fn set_from_hex(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let view = uint8_receiver(receiver)?;
    let input = string_argument(arguments)?;
    check_immutable(view)?;
    check_detached(view)?;
    let decoded = decode_hex(&input, view.logical_len());
    write_decoded(view, decoded)
}

fn encode_base64(bytes: &[u8], url_safe: bool) -> String {
    let alphabet = if url_safe {
        BASE64URL_ALPHABET
    } else {
        BASE64_ALPHABET
    };
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(*chunk.get(1).unwrap_or(&0));
        let third = u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(alphabet[(first >> 2) as usize] as char);
        output.push(alphabet[((first << 4 | second >> 4) & 0x3F) as usize] as char);
        encode_tail(&mut output, alphabet, chunk.len(), second, third);
    }
    output
}

fn encode_tail(output: &mut String, alphabet: &[u8; 64], len: usize, second: u32, third: u32) {
    match len {
        3 => {
            output.push(alphabet[((second << 2 | third >> 6) & 0x3F) as usize] as char);
            output.push(alphabet[(third & 0x3F) as usize] as char);
        }
        2 => {
            output.push(alphabet[((second << 2) & 0x3F) as usize] as char);
            output.push('=');
        }
        _ => output.push_str("=="),
    }
}

fn receiver_bytes(view: &crate::value::Uint8ArrayData) -> Vec<u8> {
    (0..view.logical_len())
        .filter_map(|index| view.get(index))
        .collect()
}

fn to_base64_options(arguments: &[Value]) -> Result<(bool, bool), VmError> {
    let Some(options) = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
    else {
        return Ok((false, false));
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error(
            "Uint8Array options must be an object",
        ));
    }
    let url_safe = alphabet_option(Some(options))?;
    let omit_padding =
        crate::vm::is_truthy(&crate::vm::get_property_result(options, "omitPadding")?);
    Ok((url_safe, omit_padding))
}

fn to_base64(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let view = uint8_receiver(receiver)?;
    let (url_safe, omit_padding) = to_base64_options(arguments)?;
    check_detached(view)?;
    let mut encoded = encode_base64(&receiver_bytes(view), url_safe);
    if omit_padding {
        encoded.truncate(encoded.trim_end_matches('=').len());
    }
    Ok(Value::String(encoded))
}

fn to_hex(receiver: Option<&Value>) -> Result<Value, VmError> {
    let view = uint8_receiver(receiver)?;
    check_detached(view)?;
    let encoded = receiver_bytes(view)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(Value::String(encoded))
}

include!("typed_array_base64_decode.rs");
