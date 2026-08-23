//! `buf.write(string[, offset[, length]][, encoding])` — the
//! string-writing entry point, with Node's overloaded argument
//! resolution.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::buffer_enc as enc;
use crate::modules::buffer_methods::{this_view, to_offset};

/// `buf.write(string[, offset[, length]][, encoding])`.
pub fn write(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let view = this_view(receiver)?;
    let source = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(source, Value::String(_) | Value::StringUnits(_)) {
        return Err(enc::invalid_arg_type(format!(
            "The \"string\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(&source)
        )));
    }
    let (offset, length, encoding) = write_args(args, view.length)?;
    let encoded = enc::encode_value(&source, &encoding)?;
    let available = view.length - offset;
    let count = encoded
        .len()
        .min(length.map_or(available, |l| l.min(available)));
    // Clamp to whole characters: UCS-2 writes complete 2-byte code
    // units, UTF-8 never splits a multi-byte sequence.
    let count = if encoding == "utf16le" {
        count & !1
    } else if encoding == "utf8" {
        let mut n = count;
        while n > 0 && n < encoded.len() && (encoded[n] & 0xC0) == 0x80 {
            n -= 1;
        }
        n
    } else {
        count
    };
    view.buffer.bytes.borrow_mut()[view.byte_offset + offset..view.byte_offset + offset + count]
        .copy_from_slice(&encoded[..count]);
    Ok(Value::Number(count as f64))
}

pub fn ascii_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    fixed_write(state, receiver, args, "ascii")
}

pub fn latin1_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    fixed_write(state, receiver, args, "latin1")
}

pub fn utf8_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    fixed_write(state, receiver, args, "utf8")
}

fn fixed_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
    encoding: &str,
) -> Result<Value, VmError> {
    let view = this_view(receiver)?;
    let offset = args.get(1).map(|value| to_offset(Some(value))).unwrap_or(0.0);
    let length = args
        .get(2)
        .map(|value| to_offset(Some(value)))
        .unwrap_or(view.length.saturating_sub(offset.max(0.0) as usize) as f64);
    if !offset.is_finite() || offset < 0.0 || offset as usize > view.length {
        return Err(enc::buffer_out_of_bounds("\"offset\" is outside of buffer bounds"));
    }
    if !length.is_finite() || length < 0.0 || length as usize > view.length - offset as usize {
        return Err(enc::buffer_out_of_bounds("\"length\" is outside of buffer bounds"));
    }
    let mut call_args = vec![args.first().cloned().unwrap_or(Value::Undefined)];
    call_args.extend([
        Value::Number(offset),
        Value::Number(length),
        Value::String(encoding.to_string()),
    ]);
    write(state, receiver, &call_args)
}

/// Resolve the overloaded `write` tail; validates offsets.
fn write_args(args: &[Value], len: usize) -> Result<(usize, Option<usize>, String), VmError> {
    let mut tail: &[Value] = &args[1..];
    if let Some(encoding) = encoding_arg(tail.first()) {
        if args.len() == 2 {
            return Ok((0, None, encoding_name(&encoding)?));
        }
        // write(string, 'utf8', 0) — offset must be a number.
        return Err(enc::invalid_arg_type(format!(
            "The \"offset\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(tail.first().unwrap_or(&Value::Undefined))
        )));
    }
    let mut offset = 0.0;
    if let Some(first) = tail.first() {
        offset = to_offset(Some(first));
        tail = &tail[1..];
    }
    let (length, encoding) = write_tail(tail)?;
    if offset < 0.0 || offset as usize > len {
        return Err(enc::out_of_range(
            "offset",
            &format!(">= 0 && <= {len}"),
            &crate::modules::buffer_enc::fmt_num(offset),
        ));
    }
    Ok((offset as usize, length, encoding))
}

fn encoding_arg(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::StringUnits(units)) => Some(String::from_utf16_lossy(units)),
        _ => None,
    }
}

fn encoding_name(raw: &str) -> Result<String, VmError> {
    enc::canonical_encoding(raw)
        .map(str::to_string)
        .ok_or_else(|| enc::unknown_encoding(raw))
}

/// The `[, length | encoding][, encoding]` tail after the offset.
fn write_tail(tail: &[Value]) -> Result<(Option<usize>, String), VmError> {
    match tail.first() {
        None => Ok((None, "utf8".to_string())),
        Some(first) if encoding_arg(Some(first)).is_some() => {
            Ok((None, encoding_name(&encoding_arg(Some(first)).unwrap())?))
        }
        Some(first) => {
            let length = Some(to_offset(Some(first)).max(0.0) as usize);
            match encoding_arg(tail.get(1)) {
                Some(s) => Ok((length, encoding_name(&s)?)),
                None => Ok((length, "utf8".to_string())),
            }
        }
    }
}
