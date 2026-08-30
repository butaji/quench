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

fn uint8_array_from_bytes(bytes: &[u8]) -> Result<Value, VmError> {
    let buffer = crate::value::ArrayBufferData::try_new(bytes.len())
        .map(Rc::new)
        .ok_or_else(|| crate::value::error::throw_range_error("ArrayBuffer allocation failed"))?;
    buffer.bytes.borrow_mut().copy_from_slice(bytes);
    Ok(Value::Uint8Array(Rc::new(crate::value::Uint8ArrayData::new(
        buffer,
        0,
        bytes.len(),
    ))))
}

fn from_base64(arguments: &[Value]) -> Result<Value, VmError> {
    let input = string_argument(arguments)?;
    let options = base64_options(arguments)?;
    let decoded = decode_base64(&input, options, usize::MAX);
    if decoded.failed {
        return Err(syntax_error());
    }
    uint8_array_from_bytes(&decoded.bytes)
}

fn from_hex(arguments: &[Value]) -> Result<Value, VmError> {
    let input = string_argument(arguments)?;
    let decoded = decode_hex(&input, usize::MAX);
    if decoded.failed {
        return Err(syntax_error());
    }
    uint8_array_from_bytes(&decoded.bytes)
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
        length.saturating_add(index).max(0) as usize
    } else {
        index.min(length) as usize
    })
}

fn subarray(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let view = receiver.cloned().ok_or_else(|| VmError::NotCallable)?;
    if !view.is_typed_array() {
        return Err(crate::value::error::throw_type_error(
            "TypedArray method called on incompatible receiver",
        ));
    }
    let source_buffer = match &view {
        Value::Float64Array(view) => view.buffer.clone(),
        Value::Float32Array(view) => view.buffer.clone(),
        Value::Int8Array(view) => view.buffer.clone(),
        Value::Int16Array(view) => view.buffer.clone(),
        Value::Int32Array(view) => view.buffer.clone(),
        Value::Uint8Array(view) => view.buffer.clone(),
        Value::Uint8ClampedArray(view) => view.buffer.clone(),
        Value::Uint16Array(view) => view.buffer.clone(),
        Value::Uint32Array(view) => view.buffer.clone(),
        Value::BigInt64Array(view) => view.buffer.clone(),
        Value::BigUint64Array(view) => view.buffer.clone(),
        _ => return Err(VmError::NotCallable),
    };
    let source_byte_offset = match &view {
        Value::Float64Array(view) => view.byte_offset,
        Value::Float32Array(view) => view.byte_offset,
        Value::Int8Array(view) => view.byte_offset,
        Value::Int16Array(view) => view.byte_offset,
        Value::Int32Array(view) => view.byte_offset,
        Value::Uint8Array(view) => view.byte_offset,
        Value::Uint8ClampedArray(view) => view.byte_offset,
        Value::Uint16Array(view) => view.byte_offset,
        Value::Uint32Array(view) => view.byte_offset,
        Value::BigInt64Array(view) => view.byte_offset,
        Value::BigUint64Array(view) => view.byte_offset,
        _ => 0,
    };
    let out_of_bounds = crate::typed_array_prototype::is_out_of_bounds(&view);
    let length = if out_of_bounds {
        0
    } else {
        crate::typed_array_ops::logical_len(&view).ok_or_else(|| VmError::NotCallable)?
    };
    let begin = match arguments.first() {
        None | Some(Value::Undefined) => 0,
        Some(_) => relative_index(arguments.first(), length)?,
    };
    let end = relative_index(
        arguments
            .get(1)
            .filter(|value| !matches!(value, Value::Undefined)),
        length,
    )?;
    let length = end.saturating_sub(begin);
    let length_tracking = match &view {
        Value::Float64Array(view) => view.length == usize::MAX,
        Value::Float32Array(view) => view.length == usize::MAX,
        Value::Int8Array(view) => view.length == usize::MAX,
        Value::Int16Array(view) => view.length == usize::MAX,
        Value::Int32Array(view) => view.length == usize::MAX,
        Value::Uint8Array(view) => view.length == usize::MAX,
        Value::Uint8ClampedArray(view) => view.length == usize::MAX,
        Value::Uint16Array(view) => view.length == usize::MAX,
        Value::Uint32Array(view) => view.length == usize::MAX,
        Value::BigInt64Array(view) => view.length == usize::MAX,
        Value::BigUint64Array(view) => view.length == usize::MAX,
        _ => false,
    };
    let end_omitted = arguments
        .get(1)
        .map_or(true, |value| matches!(value, Value::Undefined));
    let default = match view {
        Value::Float64Array(_) => Builtin::Float64Array,
        Value::Float32Array(_) => Builtin::Float32Array,
        Value::Int8Array(_) => Builtin::Int8Array,
        Value::Int16Array(_) => Builtin::Int16Array,
        Value::Int32Array(_) => Builtin::Int32Array,
        Value::Uint8Array(_) => Builtin::Uint8Array,
        Value::Uint8ClampedArray(_) => Builtin::Uint8ClampedArray,
        Value::Uint16Array(_) => Builtin::Uint16Array,
        Value::Uint32Array(_) => Builtin::Uint32Array,
        Value::BigInt64Array(_) => Builtin::BigInt64Array,
        Value::BigUint64Array(_) => Builtin::BigUint64Array,
        _ => return Err(VmError::NotCallable),
    };
    let constructor = crate::arrays::typed_array_species_constructor(&view, default)?;
    let species = match constructor {
        Value::Undefined => Value::Builtin(default),
        Value::Builtin(_) => constructor,
        Value::Null => {
            return Err(crate::value::error::throw_type_error(
                "TypedArray constructor is not an object",
            ));
        }
        constructor if !crate::value::is_object(&constructor) => {
            return Err(crate::value::error::throw_type_error(
                "TypedArray constructor is not an object",
            ));
        }
        constructor => match crate::execute::get_property_result(&constructor, "Symbol.species")? {
            Value::Undefined | Value::Null => Value::Builtin(default),
            species => species,
        },
    };
    if matches!(&species, Value::Builtin(builtin) if *builtin == default) {
        if crate::arrays::typed_array_is_detached(&view) {
            return Err(crate::value::error::throw_type_error(
                "TypedArray buffer is detached",
            ));
        }
        let element_size = match &view {
            Value::Float64Array(_) | Value::BigInt64Array(_) | Value::BigUint64Array(_) => 8,
            Value::Float32Array(_) | Value::Int32Array(_) | Value::Uint32Array(_) => 4,
            Value::Int16Array(_) | Value::Uint16Array(_) => 2,
            _ => 1,
        };
        let begin_offset = source_byte_offset.saturating_add(begin.saturating_mul(element_size));
        if source_buffer.byte_length() < begin_offset {
            return Err(crate::value::error::throw_range_error(
                "TypedArray subarray offset is out of bounds",
            ));
        }
        let result_length = if length_tracking && end_omitted {
            usize::MAX
        } else {
            let required = begin_offset.saturating_add(length.saturating_mul(element_size));
            if source_buffer.byte_length() < required {
                return Err(crate::value::error::throw_range_error(
                    "TypedArray subarray length is out of bounds",
                ));
            }
            length
        };
        return Ok(typed_array_subview(&view, begin, result_length));
    }
    let buffer = Value::ArrayBuffer(source_buffer);
    let byte_offset = source_byte_offset;
    let element_size = match &view {
        Value::Float64Array(_) | Value::BigInt64Array(_) | Value::BigUint64Array(_) => 8,
        Value::Float32Array(_) | Value::Int32Array(_) | Value::Uint32Array(_) => 4,
        Value::Int16Array(_) | Value::Uint16Array(_) => 2,
        _ => 1,
    };
    let begin_offset = Value::Number((byte_offset + begin.saturating_mul(element_size)) as f64);
    let args = if length_tracking && end_omitted {
        vec![buffer, begin_offset]
    } else {
        vec![buffer, begin_offset, Value::Number(length as f64)]
    };
    let result = crate::construct::construct_value(&species, &args)?;
    if !result.is_typed_array() {
        return Err(crate::value::error::throw_type_error(
            "TypedArray species constructor returned a non-TypedArray",
        ));
    }
    Ok(result)
}

fn typed_array_subview(value: &Value, begin: usize, length: usize) -> Value {
    macro_rules! subview {
        ($variant:ident, $data:ty, $size:expr, $view:expr) => {
            Value::$variant(Rc::new(<$data>::new(
                $view.buffer.clone(),
                $view
                    .byte_offset
                    .saturating_add(begin.saturating_mul($size)),
                length,
            )))
        };
    }
    match value {
        Value::Float64Array(view) => {
            subview!(Float64Array, crate::value::Float64ArrayData, 8, view)
        }
        Value::Float32Array(view) => {
            subview!(Float32Array, crate::value::Float32ArrayData, 4, view)
        }
        Value::Int8Array(view) => subview!(Int8Array, crate::value::Int8ArrayData, 1, view),
        Value::Int16Array(view) => subview!(Int16Array, crate::value::Int16ArrayData, 2, view),
        Value::Int32Array(view) => subview!(Int32Array, crate::value::Int32ArrayData, 4, view),
        Value::BigInt64Array(view) => {
            subview!(BigInt64Array, crate::value::BigInt64ArrayData, 8, view)
        }
        Value::BigUint64Array(view) => {
            subview!(BigUint64Array, crate::value::BigUint64ArrayData, 8, view)
        }
        Value::Uint32Array(view) => subview!(Uint32Array, crate::value::Uint32ArrayData, 4, view),
        Value::Uint8Array(view) => subview!(Uint8Array, crate::value::Uint8ArrayData, 1, view),
        Value::Uint8ClampedArray(view) => subview!(
            Uint8ClampedArray,
            crate::value::Uint8ClampedArrayData,
            1,
            view
        ),
        Value::Uint16Array(view) => subview!(Uint16Array, crate::value::Uint16ArrayData, 2, view),
        _ => Value::Undefined,
    }
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
