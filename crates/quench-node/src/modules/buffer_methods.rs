//! `Buffer.prototype` methods — one host handler per method,
//! dispatched through the capability table. All of them operate on
//! the receiver's `Uint8ArrayData` view directly.
mod buffer_search;

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::{Uint8ArrayData, Value};

use crate::host::HostState;
use crate::modules::buffer_enc as enc;

type HandlerResult = Result<Value, VmError>;

/// `buf.inspect()` — `<Buffer aa bb ...>` hex dump (INSPECT_MAX_BYTES 50).
pub fn inspect(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> HandlerResult {
    const MAX: usize = 50;
    let view = this_view(receiver)?;
    let bytes = view_bytes(&view);
    let shown: Vec<String> = bytes.iter().take(MAX).map(|b| format!("{b:02x}")).collect();
    let label = receiver
        .filter(|value| crate::modules::buffer::is_buffer(std::slice::from_ref(value)))
        .map_or("Uint8Array", |_| "Buffer");
    let mut out = format!("<{label} {}", shown.join(" "));
    if bytes.len() > MAX {
        let rest = bytes.len() - MAX;
        let plural = if rest == 1 { "" } else { "s" };
        out.push_str(&format!(" ... {rest} more byte{plural}"));
    }
    out.push('>');
    Ok(Value::String(out))
}

/// Extract the receiver as a `Uint8Array` view (`ERR_INVALID_THIS`).
pub(crate) fn this_view(receiver: Option<&Value>) -> Result<Rc<Uint8ArrayData>, VmError> {
    match receiver {
        Some(Value::Uint8Array(view)) => Ok(view.clone()),
        Some(other) => Err(enc::invalid_arg_type(format!(
            "The \"this\" argument must be an instance of Buffer.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
        None => Err(enc::invalid_arg_type(
            "The \"this\" argument must be an instance of Buffer. Received undefined".to_string(),
        )),
    }
}

fn view_bytes(view: &Uint8ArrayData) -> std::cell::Ref<'_, [u8]> {
    std::cell::Ref::map(view.buffer.bytes.borrow(), |b| {
        &b[view.byte_offset..view.byte_offset + view.length]
    })
}

/// Coerce a JS value to a byte offset (ToInteger semantics, NaN → 0).
pub(crate) fn to_offset(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => {
            if n.is_nan() {
                0.0
            } else {
                n.trunc()
            }
        }
        Some(Value::Boolean(b)) => f64::from(u8::from(*b)),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        Some(Value::BigInt(s)) => s.parse().unwrap_or(0.0),
        Some(Value::Array(array)) => match array.logical_len() {
            0 => 0.0,
            1 => {
                let value = array.index_value(0);
                to_offset(Some(&value))
            }
            _ => f64::NAN,
        },
        _ => 0.0,
    }
}

fn to_offset_checked(value: Option<&Value>) -> Result<f64, VmError> {
    let Some(value) = value else {
        return Ok(0.0);
    };
    let number = quench_runtime::to_number(value)?;
    Ok(if number.is_nan() { 0.0 } else { number.trunc() })
}

/// Clamped `start`/`end` slice bounds per Node's `toString`/`slice`.
fn clamp_bounds(args: &[Value], at: usize, len: usize, default: f64) -> usize {
    let raw = to_offset(args.get(at));
    let raw = if args.get(at).is_none() || matches!(args.get(at), Some(Value::Undefined)) {
        default
    } else {
        raw
    };
    if raw < 0.0 {
        (len as f64 + raw).max(0.0) as usize
    } else {
        (raw as usize).min(len)
    }
}

/// `buf.toString([encoding[, start[, end]]])`.
pub fn to_string(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    let view = this_view(receiver)?;
    let encoding = encoding_arg(args.first())?;
    let bytes = view_bytes(&view);
    let start = clamp_bounds(args, 1, bytes.len(), 0.0);
    let end = clamp_bounds(args, 2, bytes.len(), bytes.len() as f64);
    Ok(enc::decode_str(&bytes[start..end.max(start)], &encoding))
}

fn encoding_arg(arg: Option<&Value>) -> Result<String, VmError> {
    match arg {
        None | Some(Value::Undefined) => Ok("utf8".to_string()),
        Some(Value::String(s)) => enc::canonical_encoding(s)
            .map(str::to_string)
            .ok_or_else(|| enc::unknown_encoding(s)),
        Some(other) => Err(enc::invalid_arg_type(format!(
            "The \"encoding\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

fn bytes_of(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Uint8Array(view) => Some(view_bytes(view).to_vec()),
        _ => None,
    }
}

fn require_buffer(value: &Value, name: &str) -> Result<Vec<u8>, VmError> {
    bytes_of(value).ok_or_else(|| {
        enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be an instance of Buffer or Uint8Array.{}",
            crate::modules::util::invalid_arg_received(value)
        ))
    })
}

/// `buf.equals(other)`.
pub fn equals(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    let view = this_view(receiver)?;
    let other = args
        .first()
        .and_then(as_byte_view)
        .ok_or_else(|| {
            enc::invalid_arg_type(format!(
                "The \"otherBuffer\" argument must be an instance of Buffer or Uint8Array.{}",
                crate::modules::util::invalid_arg_received(
                    args.first().unwrap_or(&Value::Undefined)
                )
            ))
        })?;
    let bytes = view_bytes(&view);
    let other_bytes = view_bytes(&other);
    Ok(Value::Boolean(bytes[..] == other_bytes[..]))
}

fn compare_ranges(a: &[u8], b: &[u8]) -> f64 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

/// `buf.compare(target[, targetStart[, targetEnd[, sourceStart[, sourceEnd]]]])`.
pub fn compare(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    let view = this_view(receiver)?;
    let target = args.first().unwrap_or(&Value::Undefined);
    let Some(Value::Uint8Array(target_view)) = Some(target) else {
        return Err(enc::invalid_arg_type(format!(
            "The \"target\" argument must be an instance of Buffer or Uint8Array.{}",
            crate::modules::util::invalid_arg_received(target)
        )));
    };
    let source = view_bytes(&view);
    let target_bytes = view_bytes(target_view);
    let t_start = clamp_bounds(args, 1, target_bytes.len(), 0.0);
    let t_end = clamp_bounds(args, 2, target_bytes.len(), target_bytes.len() as f64);
    let s_start = clamp_bounds(args, 3, source.len(), 0.0);
    let s_end = clamp_bounds(args, 4, source.len(), source.len() as f64);
    Ok(Value::Number(compare_ranges(
        &source[s_start..s_end.max(s_start)],
        &target_bytes[t_start..t_end.max(t_start)],
    )))
}

/// `Buffer.compare(a, b)`.
pub fn compare_static(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    let a = require_buffer(args.first().unwrap_or(&Value::Undefined), "buf1")?;
    let b = require_buffer(args.get(1).unwrap_or(&Value::Undefined), "buf2")?;
    Ok(Value::Number(compare_ranges(&a, &b)))
}

/// `buf.copy(target[, targetStart[, sourceStart[, sourceEnd]]])`.
pub fn copy(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    let view = this_view(receiver)?;
    let Some(target) = args.first().and_then(as_byte_view) else {
        return Err(enc::invalid_arg_type(format!(
            "The \"target\" argument must be an instance of Buffer or Uint8Array.{}",
            crate::modules::util::invalid_arg_received(args.first().unwrap_or(&Value::Undefined))
        )));
    };
    let (target_start, source_start, count) = match copy_count(&view, &target, args)? {
        Some(range) => range,
        None => return Ok(Value::Number(0.0)),
    };
    copy_transfer(&view, &target, target_start, source_start, count)
}

fn as_byte_view(value: &Value) -> Option<Rc<Uint8ArrayData>> {
    macro_rules! view {
        ($data:expr) => {{
            Some(Rc::new(Uint8ArrayData::new(
                $data.buffer.clone(),
                $data.byte_offset,
                $data.byte_length(),
            )))
        }};
    }
    match value {
        Value::Uint8Array(data) => Some(data.clone()),
        Value::Float64Array(data) => view!(data),
        Value::Float32Array(data) => view!(data),
        Value::Int8Array(data) => view!(data),
        Value::Int16Array(data) => view!(data),
        Value::Int32Array(data) => view!(data),
        Value::Uint16Array(data) => view!(data),
        Value::Uint32Array(data) => view!(data),
        Value::Uint8ClampedArray(data) => view!(data),
        Value::BigInt64Array(data) => view!(data),
        Value::BigUint64Array(data) => view!(data),
        Value::DataView(data) => view!(data),
        _ => None,
    }
}

fn copy_transfer(
    view: &Uint8ArrayData,
    target: &Uint8ArrayData,
    target_start: usize,
    source_start: usize,
    count: usize,
) -> Result<Value, VmError> {
    let mut source_bytes = view.buffer.bytes.borrow_mut();
    let same = Rc::ptr_eq(&view.buffer, &target.buffer);
    if same {
        source_bytes.copy_within(
            view.byte_offset + source_start..view.byte_offset + source_start + count,
            target.byte_offset + target_start,
        );
    } else {
        let chunk: Vec<u8> = source_bytes
            [view.byte_offset + source_start..view.byte_offset + source_start + count]
            .to_vec();
        drop(source_bytes);
        target.buffer.bytes.borrow_mut()
            [target.byte_offset + target_start..target.byte_offset + target_start + count]
            .copy_from_slice(&chunk);
    }
    Ok(Value::Number(count as f64))
}

fn copy_count(
    view: &Uint8ArrayData,
    target: &Uint8ArrayData,
    args: &[Value],
) -> Result<Option<(usize, usize, usize)>, VmError> {
    let target_start_raw = to_offset_checked(args.get(1))?;
    if args.get(1).is_some() && target_start_raw < 0.0 {
        return Err(enc::out_of_range(
            "targetStart",
            ">= 0",
            &enc::fmt_num(target_start_raw),
        ));
    }
    // Node rejects indexes that do not fit in a 32-bit size_t.
    for (at, name) in [(2, "sourceStart"), (3, "sourceEnd")] {
        let raw = to_offset_checked(args.get(at))?;
        if args.get(at).is_some() && raw < 0.0 {
            return Err(enc::out_of_range(name, ">= 0", &enc::fmt_num(raw)));
        }
        if args.get(at).is_some() && raw > u32::MAX as f64 {
            return Err(enc::out_of_range(
                name,
                &format!(">= 0 && <= {}", u32::MAX),
                &enc::fmt_num(raw),
            ));
        }
    }
    let source_start_raw = to_offset_checked(args.get(2))?;
    let source_end_raw = if args.get(3).is_none() {
        view.length as f64
    } else {
        to_offset_checked(args.get(3))?
    };
    let source_start = source_start_raw.min(view.length as f64) as usize;
    let source_end = source_end_raw.min(view.length as f64) as usize;
    let target_start = target_start_raw.max(0.0) as usize;
    if args.get(2).is_some() && source_start_raw > view.length as f64 {
        return Err(enc::out_of_range(
            "sourceStart",
            &format!("<= {}", view.length),
            &enc::fmt_num(source_start_raw),
        ));
    }
    if target_start >= target.length || source_start >= source_end {
        return Ok(None);
    }
    Ok(Some((
        target_start,
        source_start,
        (source_end - source_start).min(target.length - target_start),
    )))
}

/// `buf.fill(value[, offset[, end]][, encoding])`.
pub fn fill(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    if !matches!(receiver, Some(Value::Uint8Array(_)))
        && matches!(args.first(), Some(Value::Uint8Array(_)))
    {
        let view = match args.first() {
            Some(Value::Uint8Array(view)) => view,
            _ => unreachable!(),
        };
        validate_fill_bound(args.get(1), view.length, "start")?;
        validate_fill_bound(args.get(2), view.length, "end")?;
        if let Some(Value::Number(value)) = args.get(3) {
            if !value.is_finite() || *value < 0.0 || *value > 255.0 {
                return Err(enc::out_of_range(
                    "value",
                    ">= 0 && <= 255",
                    &enc::fmt_num(*value),
                ));
            }
        }
        let reordered = [
            args.get(3).cloned().unwrap_or(Value::Undefined),
            args.get(1).cloned().unwrap_or(Value::Undefined),
            args.get(2).cloned().unwrap_or(Value::Undefined),
            args.get(4).cloned().unwrap_or(Value::Undefined),
        ];
        return fill(state, args.first(), &reordered);
    }
    let view = this_view(receiver)?;
    if let Some(receiver) = receiver {
        if let Ok(Value::Number(length)) =
            quench_runtime::execute::get_property_result(receiver, "length")
        {
            if length != view.length as f64 {
                return Err(enc::buffer_out_of_bounds(
                    "Attempt to access memory outside buffer bounds",
                ));
            }
        }
    }
    let fill = args.first().cloned().unwrap_or(Value::Undefined);
    let (offset_arg, end_arg, encoding_arg) = fill_args(args);
    let encoding = match encoding_arg {
        Some(value) => Some(encoding_arg_from(value, &fill)?),
        None => None,
    };
    let pattern = fill_pattern(&fill, encoding.as_deref())?;
    validate_fill_bound(args.get(offset_arg), view.length, "offset")?;
    validate_fill_bound(args.get(end_arg), view.length, "end")?;
    let offset = clamp_bounds(args, offset_arg, view.length, 0.0);
    let end = clamp_bounds(args, end_arg, view.length, view.length as f64);
    if !pattern.is_empty() {
        let mut bytes = view.buffer.bytes.borrow_mut();
        for i in offset..end {
            bytes[view.byte_offset + i] = pattern[(i - offset) % pattern.len()];
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn validate_fill_bound(value: Option<&Value>, length: usize, name: &str) -> Result<(), VmError> {
    let Some(value) = value else {
        return Ok(());
    };
    let Value::Number(value) = value else {
        if matches!(value, Value::Undefined) {
            return Ok(());
        }
        return Err(enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        )));
    };
    if !value.is_finite() || *value < 0.0 || *value > length as f64 {
        return Err(enc::out_of_range(
            name,
            &format!(">= 0 && <= {length}"),
            &enc::fmt_num(*value),
        ));
    }
    Ok(())
}

/// Resolve the overloaded `fill(value, offset, end, encoding)` tail:
/// returns (offset index, end index, encoding arg) into `args`.
fn fill_args(args: &[Value]) -> (usize, usize, Option<&Value>) {
    let string_at = |i: usize| matches!(args.get(i), Some(Value::String(_)));
    match (string_at(1), string_at(2)) {
        (true, _) => (usize::MAX, usize::MAX, args.get(1)),
        (false, true) => (1, usize::MAX, args.get(2)),
        _ => (1, 2, args.get(3)),
    }
}

fn encoding_arg_from(value: &Value, fill: &Value) -> Result<String, VmError> {
    match value {
        Value::String(s) if matches!(fill, Value::String(_) | Value::StringUnits(_)) => {
            enc::canonical_encoding(s)
                .map(str::to_string)
                .ok_or_else(|| enc::unknown_encoding(s))
        }
        Value::String(s) => Ok(enc::canonical_encoding(s).unwrap_or("utf8").to_string()),
        other => Err(enc::invalid_arg_type(format!(
            "The \"encoding\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

fn fill_pattern(fill: &Value, encoding: Option<&str>) -> Result<Vec<u8>, VmError> {
    match fill {
        Value::Number(n) => Ok(vec![*n as i64 as u8]),
        Value::String(value) if encoding == Some("hex") => {
            let valid = value.chars().count() % 2 == 0
                && value.chars().all(|character| character.is_ascii_hexdigit());
            if !valid {
                return Err(enc::invalid_arg_value(
                    "The argument 'value' is invalid".into(),
                ));
            }
            Ok(enc::encode_value(fill, "hex")?)
        }
        Value::StringUnits(units) if encoding == Some("hex") => {
            let value = String::from_utf16_lossy(units);
            if value.chars().count() % 2 != 0 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(enc::invalid_arg_value(
                    "The argument 'value' is invalid".into(),
                ));
            }
            Ok(enc::encode_value(fill, "hex")?)
        }
        Value::String(_) | Value::StringUnits(_) => {
            enc::encode_value(fill, encoding.unwrap_or("utf8"))
        }
        Value::Uint8Array(view) => Ok(view_bytes(view).to_vec()),
        _ => Ok(Vec::new()),
    }
}

/// `buf.slice([start[, end]])` — shares the backing store.
pub fn slice(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    let view = this_view(receiver)?;
    let start = clamp_bounds(args, 0, view.length, 0.0);
    let end = clamp_bounds(args, 1, view.length, view.length as f64);
    let len = end.saturating_sub(start);
    let raw = Value::Uint8Array(Rc::new(Uint8ArrayData::new(
        view.buffer.clone(),
        view.byte_offset + start,
        len,
    )));
    if receiver.is_some_and(|value| crate::modules::buffer::is_buffer(std::slice::from_ref(value)))
    {
        Ok(crate::modules::buffer_proto::finish_view_for_methods(raw))
    } else {
        Ok(raw)
    }
}

pub fn subarray(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    let view = this_view(receiver)?;
    let start = clamp_bounds(args, 0, view.length, 0.0);
    let end = clamp_bounds(args, 1, view.length, view.length as f64);
    Ok(crate::modules::buffer_proto::make_view(
        view.buffer.clone(),
        view.byte_offset + start,
        end.saturating_sub(start),
    ))
}

fn swap_in_place(view: &Uint8ArrayData, width: usize) -> HandlerResult {
    if view.length % width != 0 {
        return Err(enc::buffer_out_of_bounds(&format!(
            "Buffer size must be a multiple of {}-bits",
            width * 8
        )));
    }
    let mut bytes = view.buffer.bytes.borrow_mut();
    let range = &mut bytes[view.byte_offset..view.byte_offset + view.length];
    for chunk in range.chunks_exact_mut(width) {
        chunk.reverse();
    }
    Ok(Value::Undefined)
}

macro_rules! swap_method {
    ($name:ident, $width:expr) => {
        pub fn $name(
            _state: &Rc<RefCell<HostState>>,
            receiver: Option<&Value>,
            _args: &[Value],
        ) -> HandlerResult {
            let view = this_view(receiver)?;
            swap_in_place(&view, $width)?;
            Ok(receiver.cloned().unwrap_or(Value::Undefined))
        }
    };
}

swap_method!(swap16, 2);
swap_method!(swap32, 4);
swap_method!(swap64, 8);

/// `buf.toJSON()` → `{ type: 'Buffer', data: [...] }`.
pub fn to_json(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> HandlerResult {
    let view = this_view(receiver)?;
    let data = view_bytes(&view)
        .iter()
        .map(|b| Value::Number(f64::from(*b)))
        .collect();
    Ok(host_api::object(vec![
        ("type".to_string(), Value::String("Buffer".to_string())),
        ("data".to_string(), host_api::array(data)),
    ]))
}

/// `buf.includes|indexOf|lastIndexOf(value[, byteOffset][, encoding])`.
pub fn index_of(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    buffer_search::index_of(receiver, args)
}

pub fn last_index_of(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    buffer_search::last_index_of(receiver, args)
}

pub fn includes(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    buffer_search::includes(receiver, args)
}
