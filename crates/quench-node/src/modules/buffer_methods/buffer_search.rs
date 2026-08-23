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
    let end = search_end(args.get(2), haystack.len());
    let utf16 = args
        .get(2)
        .or_else(|| {
            args.get(1)
                .filter(|value| matches!(value, Value::String(_) | Value::StringUnits(_)))
        })
        .and_then(|value| match value {
            Value::String(s) => Some(s),
            _ => None,
        })
        .is_some_and(|encoding| {
            matches!(
                encoding.to_ascii_lowercase().as_str(),
                "ucs2" | "ucs-2" | "utf16le" | "utf-16le"
            )
        });
    let found = match needle {
        Needle::Byte(byte) => search_byte(&haystack, byte, args.get(1), end, &mode),
        Needle::Bytes(bytes) => search_bytes(&haystack, &bytes, args.get(1), end, &mode, utf16),
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
                .filter(|value| matches!(value, Value::String(_) | Value::StringUnits(_)))
                .or_else(|| {
                    args.get(1)
                        .filter(|value| matches!(value, Value::String(_) | Value::StringUnits(_)))
                })
                .map(|v| encoding_arg(Some(v)))
                .transpose()?
                .unwrap_or_else(|| "utf8".to_string());
            Ok(Needle::Bytes(enc::encode_value(value, &encoding)?))
        }
        Value::Uint8Array(view) => {
            let bytes = view_bytes(view).to_vec();
            if let Some(encoding) = args
                .get(2)
                .filter(|value| matches!(value, Value::String(_) | Value::StringUnits(_)))
                .map(|value| encoding_arg(Some(value)))
                .transpose()?
            {
                if matches!(encoding.as_str(), "utf16le") && bytes.len() % 2 != 0 {
                    let mut encoded = bytes;
                    encoded.push(0);
                    return Ok(Needle::Bytes(encoded));
                }
                return Ok(Needle::Bytes(bytes));
            }
            Ok(Needle::Bytes(bytes))
        }
        _ => {
            let received = if matches!(value, Value::Array(_)) {
                " Received an instance of Array".to_string()
            } else {
                crate::modules::util::invalid_arg_received(value)
            };
            Err(enc::invalid_arg_type(format!(
                "The \"value\" argument must be one of type number or string or an instance of Buffer or Uint8Array.{received}"
            )))
        }
    }
}

fn search_offset(arg: Option<&Value>, len: usize, last: bool) -> i64 {
    if last && matches!(arg, None | Some(Value::Undefined) | Some(Value::Object(_))) {
        return len as i64 - 1;
    }
    if last && matches!(arg, Some(Value::Number(value)) if value.is_nan()) {
        return len as i64 - 1;
    }
    let raw_value = to_offset(arg);
    if last && raw_value.is_nan() {
        return len as i64 - 1;
    }
    let raw = raw_value as i64;
    if arg.is_none() {
        return if last { len as i64 - 1 } else { 0 };
    }
    if raw < 0 {
        if last && raw < -(len as i64) {
            return -1;
        }
        (len as i64 + raw).max(0)
    } else {
        raw
    }
}

fn search_end(arg: Option<&Value>, len: usize) -> usize {
    match arg {
        Some(Value::Number(value)) if value.is_finite() => {
            (*value as i64).clamp(0, len as i64) as usize
        }
        _ => len,
    }
}

fn search_byte(
    haystack: &[u8],
    byte: u8,
    offset: Option<&Value>,
    end: usize,
    mode: &Search,
) -> i64 {
    match mode {
        Search::LastIndexOf => {
            let start = search_offset(offset, haystack.len(), true).min(end as i64 - 1);
            if start < 0 {
                return -1;
            }
            (0..=start.max(-1) as usize)
                .rev()
                .find(|&i| haystack.get(i) == Some(&byte))
                .map_or(-1, |i| i as i64)
        }
        _ => {
            let start = search_offset(offset, haystack.len(), false) as usize;
            (start..end)
                .find(|&i| haystack[i] == byte)
                .map_or(-1, |i| i as i64)
        }
    }
}

fn search_bytes(
    haystack: &[u8],
    needle: &[u8],
    offset: Option<&Value>,
    end: usize,
    mode: &Search,
    utf16: bool,
) -> i64 {
    if needle.is_empty() {
        return match mode {
            Search::LastIndexOf => search_offset(offset, haystack.len(), true).min(end as i64),
            _ => search_offset(offset, haystack.len(), false).min(end as i64),
        };
    }
    match mode {
        Search::LastIndexOf => {
            let start = search_offset(offset, haystack.len(), true).min(end as i64 - 1);
            if start < 0 {
                return -1;
            }
            (0..=start)
                .rev()
                .find(|&i| (!utf16 || i % 2 == 0) && ends_at(haystack, needle, i as usize))
                .map_or(-1, |i| i)
        }
        _ => {
            let start = search_offset(offset, haystack.len(), false) as usize;
            (start..end)
                .find(|&i| (!utf16 || i % 2 == 0) && ends_at(haystack, needle, i))
                .map_or(-1, |i| i as i64)
        }
    }
}

fn ends_at(haystack: &[u8], needle: &[u8], index: usize) -> bool {
    index + needle.len() <= haystack.len() && &haystack[index..index + needle.len()] == needle
}
