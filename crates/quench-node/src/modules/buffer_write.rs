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
    view.buffer.bytes.borrow_mut()[view.byte_offset + offset..view.byte_offset + offset + count]
        .copy_from_slice(&encoded[..count]);
    Ok(Value::Number(count as f64))
}

/// Resolve the overloaded `write` tail; validates offsets.
fn write_args(args: &[Value], len: usize) -> Result<(usize, Option<usize>, String), VmError> {
    let mut tail: &[Value] = &args[1..];
    if let Some(Value::String(s)) = tail.first() {
        if args.len() == 2 {
            return Ok((0, None, encoding_name(s)?));
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

fn encoding_name(raw: &str) -> Result<String, VmError> {
    enc::canonical_encoding(raw)
        .map(str::to_string)
        .ok_or_else(|| enc::unknown_encoding(raw))
}

/// The `[, length | encoding][, encoding]` tail after the offset.
fn write_tail(tail: &[Value]) -> Result<(Option<usize>, String), VmError> {
    match tail.first() {
        None => Ok((None, "utf8".to_string())),
        Some(Value::String(s)) => Ok((None, encoding_name(s)?)),
        Some(first) => {
            let length = Some(to_offset(Some(first)).max(0.0) as usize);
            match tail.get(1) {
                Some(Value::String(s)) => Ok((length, encoding_name(s)?)),
                _ => Ok((length, "utf8".to_string())),
            }
        }
    }
}
