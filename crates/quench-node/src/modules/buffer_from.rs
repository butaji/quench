//! `Buffer.from` / `Buffer.copyBytesFrom` — argument coercion and the
//! array-like / ArrayBuffer / typed-view construction paths.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;
use quench_runtime::vm::get_property;

use crate::modules::buffer::encoding_name;
use crate::modules::buffer_enc as enc;

pub fn from_handler(
    state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    from(state, args)
}

pub fn from(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let first = args.first().cloned().unwrap_or(Value::Undefined);
    match &first {
        Value::Uint8Array(arr) => {
            let bytes =
                arr.buffer.bytes.borrow()[arr.byte_offset..arr.byte_offset + arr.length].to_vec();
            Ok(crate::modules::buffer_proto::make_buffer(&bytes))
        }
        Value::String(s) if is_symbol_payload(s) => Err(first_arg_error(&first)),
        Value::String(_) | Value::StringUnits(_) => {
            // `from` tolerates a non-string encoding (defaults to utf8);
            // an unknown string encoding still throws.
            let encoding = match args.get(1) {
                Some(Value::String(_)) => encoding_name(args.get(1))?,
                _ => "utf8".to_string(),
            };
            Ok(crate::modules::buffer_proto::make_pooled_buffer(
                &enc::encode_value(&first, &encoding)?,
            ))
        }
        Value::Array(_) => from_object(&first, args.get(1)),
        Value::Object(_) => {
            // JSON.stringify(Buffer) produces the canonical `{ type:
            // "Buffer", data: [...] }` record accepted by Buffer.from.
            if matches!(get_property(&first, "type"), Value::String(kind) if kind == "Buffer") {
                let data = get_property(&first, "data");
                if matches!(data, Value::Array(_)) {
                    return from(_state, &[data]);
                }
            }
            from_object(&first, args.get(1))
        }
        Value::ArrayBuffer(buf) => from_array_buffer(buf, args),
        Value::Number(_)
        | Value::Boolean(_)
        | Value::BigInt(_)
        | Value::Function(_)
        | Value::BoundFunction(_)
        | Value::Undefined
        | Value::Null => Err(first_arg_error(&first)),
        // Other typed-array views are interpreted as arrays of
        // integers modulo 256 (Node's `fromArrayLike`).
        _ if is_typed_view(&first) => from_array_like(&first),
        _ => Ok(crate::modules::buffer_proto::make_buffer(&[])),
    }
}

/// `Buffer.of(...values)` — numeric values are converted with ToNumber and
/// stored modulo 256, sharing Buffer's canonical byte representation.
pub fn of(args: &[Value]) -> Result<Value, VmError> {
    let mut bytes = Vec::with_capacity(args.len());
    for value in args {
        let number = quench_runtime::to_number(value)?;
        let byte = if number.is_nan() {
            0
        } else {
            (number as i64 & 0xff) as u8
        };
        bytes.push(byte);
    }
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

fn first_arg_error(value: &Value) -> VmError {
    enc::invalid_arg_type(format!(
        "The first argument must be of type string or an instance of Buffer, \
         ArrayBuffer, or Array or an Array-like Object.{}",
        crate::modules::util::invalid_arg_received(value)
    ))
}

fn is_typed_view(value: &Value) -> bool {
    matches!(
        value,
        Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::Uint16Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Float32Array(_)
            | Value::Float64Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::DataView(_)
    )
}

fn from_array_buffer(
    buf: &Rc<quench_runtime::value::ArrayBufferData>,
    args: &[Value],
) -> Result<Value, VmError> {
    let length = buf.bytes.borrow().len() as f64;
    let offset = match args.get(1) {
        Some(value) => crate::modules::buffer_methods::to_offset(Some(value)),
        None => 0.0,
    };
    if offset < 0.0 || offset > length {
        return Err(enc::buffer_out_of_bounds(
            "\"offset\" is outside of buffer bounds",
        ));
    }
    let view_length = match args.get(2) {
        Some(value) => crate::modules::buffer_methods::to_offset(Some(value)),
        None if buf.max_byte_length.is_some() => f64::NAN,
        None => length - offset,
    };
    if view_length < 0.0 || offset + view_length > length {
        return Err(enc::buffer_out_of_bounds(
            "\"length\" is outside of buffer bounds",
        ));
    }
    let view_length = if view_length.is_nan() {
        usize::MAX
    } else {
        view_length as usize
    };
    Ok(crate::modules::buffer_proto::make_view(
        buf.clone(),
        offset as usize,
        view_length,
    ))
}

fn from_array_like(value: &Value) -> Result<Value, VmError> {
    // Dense ordinary arrays are the common Buffer.from(array) path.  Their
    // packed storage is already the authoritative element sequence; reading
    // each index through generic property dispatch needlessly re-resolves the
    // same shape for every byte.
    if let Value::Array(array) = value {
        if let Some(values) = array.packed_values() {
            let bytes: Vec<u8> = values
                .into_iter()
                .map(|value| number_to_byte(to_number(&value)))
                .collect();
            return Ok(crate::modules::buffer_proto::make_buffer(&bytes));
        }
    }
    let length = match get_property(value, "length") {
        Value::Number(n) if n.is_finite() && n > 0.0 => n.trunc().min(u32::MAX as f64) as u32,
        _ => u32::MAX,
    };
    let mut bytes = Vec::new();
    for i in 0..length {
        let v = get_property(value, &i.to_string());
        if matches!(v, Value::Undefined) && length == u32::MAX {
            break;
        }
        bytes.push(number_to_byte(to_number(&v)));
    }
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

fn number_to_byte(number: f64) -> u8 {
    if number.is_finite() {
        number.trunc().rem_euclid(256.0) as u8
    } else {
        0
    }
}

fn is_symbol_payload(text: &str) -> bool {
    text.starts_with("Symbol.") || text.starts_with("Symbol.for.")
}

/// `fromObject`: string-coercible objects encode as strings; objects
/// with a `length` property go through the array-like path; anything
/// else is `ERR_INVALID_ARG_TYPE`.
fn from_object(value: &Value, encoding_arg: Option<&Value>) -> Result<Value, VmError> {
    if let Some(text) = coerce_object_string(value)? {
        let encoding = encoding_name(encoding_arg)?;
        return Ok(crate::modules::buffer_proto::make_buffer(
            &enc::encode_value(&Value::String(text), &encoding)?,
        ));
    }
    if !matches!(get_property(value, "length"), Value::Undefined)
        && (matches!(value, Value::Array(_)) || has_own(value, "length"))
    {
        return from_array_like(value);
    }
    // Node accepts `{ buffer, byteOffset?, length? }` view descriptors.
    if let Value::ArrayBuffer(buf) = get_property(value, "buffer") {
        let byte_offset = to_number(&get_property(value, "byteOffset")).max(0.0) as usize;
        let length = buf.bytes.borrow().len();
        let view_length = match get_property(value, "length") {
            Value::Number(n) if n >= 0.0 => (n as usize).min(length - byte_offset.min(length)),
            _ => length - byte_offset.min(length),
        };
        return Ok(crate::modules::buffer_proto::make_view(
            buf,
            byte_offset,
            view_length,
        ));
    }
    Err(first_arg_error(value))
}

fn has_own(value: &Value, key: &str) -> bool {
    quench_runtime::execute::own_enumerable_keys(value)
        .iter()
        .any(|k| k == key)
}

/// Node's coercion chain: `Symbol.toPrimitive('string')`, else a
/// custom `valueOf()`, else a custom `toString()`; string results win.
fn coerce_object_string(value: &Value) -> Result<Option<String>, VmError> {
    let exotic = quench_runtime::execute::get_property_result(value, "Symbol.toPrimitive")?;
    if quench_runtime::is_callable(&exotic) {
        let primitive =
            quench_runtime::execute::call(&exotic, value, &[Value::String("string".to_string())])?;
        return Ok(string_primitive(&primitive));
    }
    // Boxed String/Number/Boolean: the primitive lives in `_value`.
    let boxed = get_property(value, "_value");
    if !matches!(boxed, Value::Undefined) {
        return Ok(string_primitive(&boxed));
    }
    if has_own(value, "valueOf") {
        let value_of = quench_runtime::execute::get_property_result(value, "valueOf")?;
        if quench_runtime::is_callable(&value_of) {
            let result = quench_runtime::execute::call(&value_of, value, &[])?;
            return Ok(string_primitive(&result));
        }
        return Ok(None);
    }
    if has_own(value, "toString") {
        let to_string = quench_runtime::execute::get_property_result(value, "toString")?;
        if quench_runtime::is_callable(&to_string) {
            let result = quench_runtime::execute::call(&to_string, value, &[])?;
            return Ok(string_primitive(&result));
        }
    }
    Ok(None)
}

fn string_primitive(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !is_symbol_payload(s) => Some(s.clone()),
        Value::StringUnits(units) => Some(String::from_utf16_lossy(units)),
        _ => None,
    }
}

fn to_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        Value::String(s) => s.parse().unwrap_or(0.0),
        Value::Boolean(true) => 1.0,
        Value::Boolean(false) => 0.0,
        _ => 0.0,
    }
}

/// `Buffer.copyBytesFrom(view[, offset[, length]])` — copies the raw
/// bytes of any ArrayBuffer view; offset/length are in elements.
pub fn copy_bytes_from(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let view = args.first().cloned().unwrap_or(Value::Undefined);
    let Some((buffer, byte_offset, elements, element_size)) = view_parts(&view) else {
        return Err(enc::invalid_arg_type(format!(
            "The \"view\" argument must be an instance of a TypedArray or DataView.{}",
            crate::modules::util::invalid_arg_received(&view)
        )));
    };
    let offset = index_arg(args.get(1), "offset")?.unwrap_or(0).min(elements);
    let count = index_arg(args.get(2), "length")?
        .unwrap_or(elements - offset)
        .min(elements - offset);
    let start = byte_offset + offset * element_size;
    let bytes = buffer.bytes.borrow()[start..start + count * element_size].to_vec();
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

/// (buffer, byte_offset, element count, bytes per element) of a view.
fn view_parts(
    value: &Value,
) -> Option<(
    Rc<quench_runtime::value::ArrayBufferData>,
    usize,
    usize,
    usize,
)> {
    macro_rules! parts {
        ($data:expr, $size:expr) => {
            ($data.buffer.clone(), $data.byte_offset, $data.length, $size)
        };
    }
    Some(match value {
        Value::Uint8Array(d) => parts!(d, 1),
        Value::Int8Array(d) => parts!(d, 1),
        Value::Uint8ClampedArray(d) => parts!(d, 1),
        Value::Int16Array(d) => parts!(d, 2),
        Value::Uint16Array(d) => parts!(d, 2),
        Value::Int32Array(d) => parts!(d, 4),
        Value::Uint32Array(d) => parts!(d, 4),
        Value::Float32Array(d) => parts!(d, 4),
        Value::Float64Array(d) => parts!(d, 8),
        Value::BigInt64Array(d) => parts!(d, 8),
        Value::BigUint64Array(d) => parts!(d, 8),
        Value::DataView(d) => (d.buffer.clone(), d.byte_offset, d.byte_length, 1),
        _ => return None,
    })
}

fn index_arg(value: Option<&Value>, name: &str) -> Result<Option<usize>, VmError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Value::Number(n) = value else {
        return Err(enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    };
    if n.is_nan() || !n.is_finite() || n.fract() != 0.0 || *n < 0.0 {
        return Err(enc::out_of_range(
            name,
            &format!(">= 0 && <= {}", u32::MAX),
            &enc::fmt_num(*n),
        ));
    }
    Ok(Some(*n as usize))
}
