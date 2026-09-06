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
    let haystack = view_bytes(&view);
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
        Needle::Bytes(ref bytes) => search_bytes(&haystack, bytes, args.get(1), end, &mode, utf16),
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
    if last
        && matches!(arg, Some(Value::String(encoding)) if enc::canonical_encoding(encoding).is_some())
    {
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
            Search::LastIndexOf => {
                let start = if matches!(
                    offset,
                    None | Some(Value::Undefined) | Some(Value::Object(_))
                ) || matches!(offset, Some(Value::Number(value)) if value.is_nan())
                {
                    haystack.len() as i64
                } else {
                    search_offset(offset, haystack.len(), true)
                };
                start.min(end as i64)
            }
            _ => search_offset(offset, haystack.len(), false).min(end as i64),
        };
    }
    let max_start = match mode {
        Search::LastIndexOf => search_offset(offset, haystack.len(), true).min(end as i64 - 1),
        _ => search_offset(offset, haystack.len(), false),
    };
    if max_start < 0 {
        return -1;
    }
    match mode {
        Search::LastIndexOf => {
            let limit = (max_start as usize + needle.len())
                .min(end)
                .min(haystack.len());
            kmp_search(haystack, needle, 0, limit, max_start as usize, utf16, true)
        }
        _ => kmp_search(
            haystack,
            needle,
            max_start as usize,
            end.min(haystack.len()),
            usize::MAX,
            utf16,
            false,
        ),
    }
}

fn kmp_search(
    haystack: &[u8],
    needle: &[u8],
    scan_start: usize,
    scan_end: usize,
    reverse_limit: usize,
    utf16: bool,
    reverse: bool,
) -> i64 {
    if scan_start >= scan_end || needle.len() > scan_end.saturating_sub(scan_start) {
        return -1;
    }
    if needle.len() > 4096 {
        return rolling_search(
            haystack,
            needle,
            scan_start,
            scan_end,
            reverse_limit,
            utf16,
            reverse,
        );
    }
    let mut prefix = vec![0usize; needle.len()];
    for index in 1..needle.len() {
        let mut matched = prefix[index - 1];
        while matched > 0 && needle[index] != needle[matched] {
            matched = prefix[matched - 1];
        }
        if needle[index] == needle[matched] {
            matched += 1;
        }
        prefix[index] = matched;
    }
    let mut matched = 0usize;
    let mut last = None;
    for (absolute, byte) in haystack[scan_start..scan_end].iter().copied().enumerate() {
        while matched > 0 && byte != needle[matched] {
            matched = prefix[matched - 1];
        }
        if byte == needle[matched] {
            matched += 1;
        }
        if matched == needle.len() {
            let index = scan_start + absolute + 1 - needle.len();
            if (!utf16 || index % 2 == 0) && (!reverse || index <= reverse_limit) {
                if !reverse {
                    return index as i64;
                }
                last = Some(index as i64);
            }
            matched = prefix[matched - 1];
        }
    }
    last.unwrap_or(-1)
}

fn rolling_search(
    haystack: &[u8],
    needle: &[u8],
    scan_start: usize,
    scan_end: usize,
    reverse_limit: usize,
    utf16: bool,
    reverse: bool,
) -> i64 {
    let length = needle.len();
    let last_start = scan_end - length;
    let base = 257u64;
    let mut high = 1u64;
    for _ in 1..length {
        high = high.wrapping_mul(base);
    }
    let hash = |bytes: &[u8]| {
        bytes.iter().fold(0u64, |hash, byte| {
            hash.wrapping_mul(base).wrapping_add(*byte as u64)
        })
    };
    let reverse_hash = |bytes: &[u8]| {
        bytes.iter().rev().fold(0u64, |hash, byte| {
            hash.wrapping_mul(base).wrapping_add(*byte as u64)
        })
    };
    let needle_hash = hash(needle);
    if reverse {
        let start_limit = last_start.min(reverse_limit);
        let needle_hash = reverse_hash(needle);
        let mut window_hash = reverse_hash(&haystack[start_limit..start_limit + length]);
        for start in (0..=start_limit).rev() {
            if window_hash == needle_hash
                && (!utf16 || start % 2 == 0)
                && haystack[start..start + length] == *needle
            {
                return start as i64;
            }
            if start > 0 {
                window_hash = window_hash
                    .wrapping_sub((haystack[start + length - 1] as u64).wrapping_mul(high))
                    .wrapping_mul(base)
                    .wrapping_add(haystack[start - 1] as u64);
            }
        }
        -1
    } else {
        let mut window_hash = hash(&haystack[scan_start..scan_start + length]);
        for start in scan_start..=last_start {
            if window_hash == needle_hash
                && (!utf16 || start % 2 == 0)
                && haystack[start..start + length] == *needle
            {
                return start as i64;
            }
            if start < last_start {
                window_hash = window_hash
                    .wrapping_sub((haystack[start] as u64).wrapping_mul(high))
                    .wrapping_mul(base)
                    .wrapping_add(haystack[start + length] as u64);
            }
        }
        -1
    }
}
