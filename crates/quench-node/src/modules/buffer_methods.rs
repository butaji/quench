//! `Buffer.prototype` methods — one host handler per method,
//! dispatched through the capability table. All of them operate on
//! the receiver's `Uint8ArrayData` view directly.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::{Uint8ArrayData, Value};

use crate::host::HostState;
use crate::modules::buffer_enc as enc;

type HandlerResult = Result<Value, VmError>;

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
        _ => 0.0,
    }
}

/// Clamped `start`/`end` slice bounds per Node's `toString`/`slice`.
fn clamp_bounds(args: &[Value], at: usize, len: usize, default: f64) -> usize {
    let raw = to_offset(args.get(at));
    let raw = if args.get(at).is_none() { default } else { raw };
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
    let other = require_buffer(args.first().unwrap_or(&Value::Undefined), "otherBuffer")?;
    let bytes = view_bytes(&view);
    Ok(Value::Boolean(bytes[..] == other[..]))
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
    let Some(Value::Uint8Array(target)) = args.first() else {
        return Err(enc::invalid_arg_type(format!(
            "The \"target\" argument must be an instance of Buffer or Uint8Array.{}",
            crate::modules::util::invalid_arg_received(args.first().unwrap_or(&Value::Undefined))
        )));
    };
    let target_start = to_offset(args.get(1)).max(0.0) as usize;
    let source_start = clamp_bounds(args, 2, view.length, 0.0);
    let source_end = clamp_bounds(args, 3, view.length, view.length as f64);
    if target_start >= target.length || source_start >= source_end {
        return Ok(Value::Number(0.0));
    }
    let count = (source_end - source_start).min(target.length - target_start);
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

/// `buf.fill(value[, offset[, end]][, encoding])`.
pub fn fill(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    let view = this_view(receiver)?;
    let fill = args.first().cloned().unwrap_or(Value::Undefined);
    let (offset_arg, end_arg, encoding_arg) = fill_args(args);
    let encoding = match encoding_arg {
        Some(value) => Some(encoding_arg_from(value, &fill)?),
        None => None,
    };
    let pattern = fill_pattern(&fill, encoding.as_deref())?;
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
        _ => Ok("utf8".to_string()),
    }
}

fn fill_pattern(fill: &Value, encoding: Option<&str>) -> Result<Vec<u8>, VmError> {
    match fill {
        Value::Number(n) => Ok(vec![*n as i64 as u8]),
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
    Ok(crate::modules::buffer_proto::make_view(
        view.buffer.clone(),
        view.byte_offset + start,
        len,
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
    search(receiver, args, Search::IndexOf)
}

pub fn last_index_of(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    search(receiver, args, Search::LastIndexOf)
}

pub fn includes(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> HandlerResult {
    search(receiver, args, Search::Includes)
}

enum Search {
    IndexOf,
    LastIndexOf,
    Includes,
}

fn search(receiver: Option<&Value>, args: &[Value], mode: Search) -> HandlerResult {
    let view = this_view(receiver)?;
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let haystack = view_bytes(&view).to_vec();
    let needle = search_needle(&value, args)?;
    let found = match needle {
        Needle::Byte(byte) => search_byte(&haystack, byte, args.get(1), &mode),
        Needle::Bytes(bytes) => search_bytes(&haystack, &bytes, args.get(1), &mode),
    };
    Ok(match mode {
        Search::Includes => Value::Boolean(found >= 0),
        _ => Value::Number(found as f64),
    })
}

enum Needle {
    Byte(u8),
    Bytes(Vec<u8>),
}

fn search_needle(value: &Value, args: &[Value]) -> Result<Needle, VmError> {
    match value {
        Value::Number(n) => Ok(Needle::Byte(*n as i64 as u8)),
        Value::BigInt(s) => Ok(Needle::Byte(s.parse::<i64>().unwrap_or(0) as u8)),
        Value::String(_) | Value::StringUnits(_) => {
            let encoding = match args.get(2) {
                Some(v) => encoding_arg(Some(v))?,
                None => "utf8".to_string(),
            };
            Ok(Needle::Bytes(enc::encode_value(value, &encoding)?))
        }
        Value::Uint8Array(view) => Ok(Needle::Bytes(view_bytes(view).to_vec())),
        _ => Err(enc::invalid_arg_type(format!(
            "The \"value\" argument must be one of type string, Buffer, TypedArray, or DataView.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
    }
}

fn search_offset(arg: Option<&Value>, len: usize, last: bool) -> i64 {
    let raw = to_offset(arg) as i64;
    if arg.is_none() {
        return if last { len as i64 - 1 } else { 0 };
    }
    if raw < 0 {
        (len as i64 + raw).max(0)
    } else {
        raw
    }
}

fn search_byte(haystack: &[u8], byte: u8, offset: Option<&Value>, mode: &Search) -> i64 {
    match mode {
        Search::LastIndexOf => {
            let start = search_offset(offset, haystack.len(), true).min(haystack.len() as i64 - 1);
            (0..=start.max(-1) as usize)
                .rev()
                .find(|&i| haystack.get(i) == Some(&byte))
                .map_or(-1, |i| i as i64)
        }
        _ => {
            let start = search_offset(offset, haystack.len(), false) as usize;
            (start..haystack.len())
                .find(|&i| haystack[i] == byte)
                .map_or(-1, |i| i as i64)
        }
    }
}

fn search_bytes(haystack: &[u8], needle: &[u8], offset: Option<&Value>, mode: &Search) -> i64 {
    if needle.is_empty() {
        return match mode {
            Search::LastIndexOf => search_offset(offset, haystack.len(), true),
            _ => search_offset(offset, haystack.len(), false).min(haystack.len() as i64),
        };
    }
    match mode {
        Search::LastIndexOf => {
            let start = search_offset(offset, haystack.len(), true);
            (0..=start)
                .rev()
                .find(|&i| ends_at(haystack, needle, i as usize))
                .map_or(-1, |i| i)
        }
        _ => {
            let start = search_offset(offset, haystack.len(), false) as usize;
            (start..haystack.len())
                .find(|&i| ends_at(haystack, needle, i))
                .map_or(-1, |i| i as i64)
        }
    }
}

fn ends_at(haystack: &[u8], needle: &[u8], index: usize) -> bool {
    index + needle.len() <= haystack.len() && &haystack[index..index + needle.len()] == needle
}
