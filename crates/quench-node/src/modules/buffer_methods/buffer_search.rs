use quench_runtime::execute::VmError;
use quench_runtime::value::{Uint8ArrayData, Value};

use super::{encoding_arg, this_view, to_offset};
use crate::modules::buffer_enc as enc;

type HandlerResult = Result<Value, VmError>;

pub fn index_of(receiver: Option<&Value>, args: &[Value]) -> HandlerResult {
    search(receiver, args, Search::IndexOf)
}

pub fn last_index_of(receiver: Option<&Value>, args: &[Value]) -> HandlerResult {
    search(receiver, args, Search::LastIndexOf)
}

pub fn includes(receiver: Option<&Value>, args: &[Value]) -> HandlerResult {
    search(receiver, args, Search::Includes)
}

enum Search {
    IndexOf,
    LastIndexOf,
    Includes,
}
enum Needle {
    Byte(u8),
    Bytes(Vec<u8>),
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

fn view_bytes(view: &Uint8ArrayData) -> std::cell::Ref<'_, [u8]> {
    std::cell::Ref::map(view.buffer.bytes.borrow(), |b| {
        &b[view.byte_offset..view.byte_offset + view.length]
    })
}

fn search_needle(value: &Value, args: &[Value]) -> Result<Needle, VmError> {
    match value {
        Value::Number(n) => Ok(Needle::Byte(*n as i64 as u8)),
        Value::BigInt(s) => Ok(Needle::Byte(s.parse::<i64>().unwrap_or(0) as u8)),
        Value::String(_) | Value::StringUnits(_) => {
            let encoding = args
                .get(2)
                .map(|v| encoding_arg(Some(v)))
                .transpose()?
                .unwrap_or_else(|| "utf8".to_string());
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
