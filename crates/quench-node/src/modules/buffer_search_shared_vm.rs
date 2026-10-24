//! Node Buffer byte search policy; Core owns brands and language coercion.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeArithmetic, NativeContext, RootId, RootedError};

const UTF16_CODE_UNIT_BYTES: usize = std::mem::size_of::<u16>();

#[derive(Clone, Copy)]
#[repr(u8)]
enum SearchKind {
    Forward,
    Backward,
    Includes,
}
impl SearchKind {
    fn forward(self) -> bool {
        !matches!(self, Self::Backward)
    }
}

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    prototype: RootId,
) -> Result<(), RootedError> {
    for (name, kind, source) in [
        (
            "indexOf",
            SearchKind::Forward,
            quench_js_check::checked_js!(
                "op=>function indexOf(value,offset,end,encoding){return op(this,value,offset,end,encoding);}"
            ),
        ),
        (
            "lastIndexOf",
            SearchKind::Backward,
            quench_js_check::checked_js!(
                "op=>function lastIndexOf(value,offset,end,encoding){return op(this,value,offset,end,encoding);}"
            ),
        ),
        (
            "includes",
            SearchKind::Includes,
            quench_js_check::checked_js!(
                "op=>function includes(value,offset,end,encoding){return op(this,value,offset,end,encoding);}"
            ),
        ),
    ] {
        let mode = context.number(kind as u8 as f64);
        let operation = context
            .host_function_with_data(crate::host::shared_vm::operation("bufferSearch"), mode)?;
        let factory = context.evaluate_script_rooted(source, "node:buffer/search-facade.js")?;
        let undefined = context.undefined();
        let method = context.call_rooted(factory, undefined, &[operation])?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(prototype, key, method, true, true, true)?;
    }
    Ok(())
}

fn property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn undefined(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
}
fn invalid(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
    expected: &str,
) -> Result<RootedError, RootedError> {
    super::util_shared_vm::argument_type_error_with_expected(context, value, name, expected)
}

pub(crate) fn search(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mode = context.host_function_data()?;
    let kind = match context
        .rooted_value(mode)
        .and_then(|value| value.as_number())
    {
        Some(value) if value == SearchKind::Backward as u8 as f64 => SearchKind::Backward,
        Some(value) if value == SearchKind::Includes as u8 as f64 => SearchKind::Includes,
        _ => SearchKind::Forward,
    };
    let absent = context.undefined();
    let buffer = args.first().copied().unwrap_or(absent);
    let needle = args.get(1).copied().unwrap_or(absent);
    let mut offset = args.get(2).copied().unwrap_or(absent);
    let mut end = args.get(3).copied().unwrap_or(absent);
    let mut encoding = args.get(4).copied().unwrap_or(absent);
    if context.string_text(end)?.is_some() {
        encoding = end;
        end = property(context, buffer, "length")?;
    } else if undefined(context, end) {
        end = property(context, buffer, "length")?;
    }
    if !context.has_builtin_brand_rooted(buffer, BuiltinBrand::ArrayBufferView)? {
        return Err(invalid(
            context,
            buffer,
            "buffer",
            "an instance of Buffer, TypedArray, or DataView",
        )?);
    }
    if context.string_text(offset)?.is_some() {
        encoding = offset;
        offset = absent;
    } else {
        let maximum = context.number(i32::MAX as f64);
        let minimum = context.number(i32::MIN as f64);
        if context.greater_than_rooted(offset, maximum)? {
            offset = maximum;
        } else if context.greater_than_rooted(minimum, offset)? {
            offset = minimum;
        }
    }
    let mut offset_number = context.to_number(offset)?;
    if offset_number.is_nan() {
        offset_number = if kind.forward() {
            0.0
        } else {
            let length = property(context, buffer, "length")?;
            let length = if context.truthy_rooted(length)? {
                length
            } else {
                property(context, buffer, "byteLength")?
            };
            context.to_number(length)?
        };
    }
    let numeric_needle = context
        .rooted_value(needle)
        .and_then(|value| value.as_number())
        .is_some();
    let mut two_byte = false;
    let mut string_needle = false;
    let needle_bytes = if numeric_needle {
        let mask = context.number(u8::MAX as f64);
        let byte = context.arithmetic_rooted(NativeArithmetic::BitwiseAnd, needle, mask)?;
        vec![context
            .rooted_value(byte)
            .and_then(|value| value.as_number())
            .unwrap_or(0.0) as u8]
    } else {
        let canonical = if undefined(context, encoding) {
            Some("utf8")
        } else {
            let empty = context.string_rooted("");
            let name = context.arithmetic_rooted(NativeArithmetic::Add, encoding, empty)?;
            let name = context.string_text(name)?.unwrap_or_default();
            super::buffer_codec::canonical_encoding(&name)
        };
        if context.string_text(needle)?.is_some() {
            string_needle = true;
            let Some(canonical) = canonical else {
                let name = context.to_string(encoding)?;
                let error = context.type_error_rooted(&format!("Unknown encoding: {name}"))?;
                let key = context.string_rooted("code");
                let code = context.string_rooted("ERR_UNKNOWN_ENCODING");
                context.set_property_rooted(error, key, code, error)?;
                return Err(context.throw(error));
            };
            two_byte = canonical == "utf16le";
            let name = context.string_rooted(canonical);
            super::buffer_shared_vm::encode_bytes(context, needle, Some(name))?
        } else if context.has_builtin_brand_rooted(needle, BuiltinBrand::Uint8Array)? {
            two_byte = canonical == Some("utf16le");
            context.view_bytes_rooted(needle).unwrap_or_default()
        } else {
            return Err(invalid(
                context,
                needle,
                "value",
                "one of type number or string or an instance of Buffer or Uint8Array",
            )?);
        }
    };
    let end_number = if undefined(context, end) {
        f64::INFINITY
    } else {
        context.to_number(end)?
    };
    // Take bytes after original conversion hooks, which may mutate/detach views.
    let bytes = context.view_bytes_rooted(buffer).unwrap_or_default();
    let length = if two_byte && string_needle {
        bytes.len() & !1
    } else {
        bytes.len()
    };
    let end = if end_number.is_nan() {
        0
    } else {
        end_number.max(0.0).trunc().min(length as f64) as usize
    };
    let end = if two_byte { end & !1 } else { end };
    let offset = offset_number.trunc() as i64;
    let position = index_offset(length, offset, needle_bytes.len(), kind.forward());
    let result = if needle_bytes.is_empty() {
        position.min(end as i64)
    } else if position < 0 || length == 0 || needle_bytes.len() > end {
        -1
    } else {
        let position = position as usize;
        let mut position = if !kind.forward() && position >= end {
            end.saturating_sub(1)
        } else {
            position
        };
        // Node rounds forward string searches up to the next complete UTF-16
        // unit. Byte-view needles retain their separate floor-alignment rule.
        if two_byte && string_needle && kind.forward() {
            position += position % UTF16_CODE_UNIT_BYTES;
        }
        if end == 0
            || (kind.forward()
                && (position >= end || needle_bytes.len().saturating_add(position) > end))
        {
            -1
        } else {
            byte_search(
                &bytes[..end],
                &needle_bytes,
                position,
                kind.forward(),
                two_byte,
            )
            .map_or(-1, |value| value as i64)
        }
    };
    Ok(if matches!(kind, SearchKind::Includes) {
        context.boolean(result != -1)
    } else {
        context.number(result as f64)
    })
}

fn index_offset(length: usize, offset: i64, needle: usize, forward: bool) -> i64 {
    let length = length as i64;
    if offset < 0 {
        if offset.saturating_add(length) >= 0 {
            offset + length
        } else if forward || needle == 0 {
            0
        } else {
            -1
        }
    } else if offset.saturating_add(needle as i64) <= length {
        offset
    } else if needle == 0 {
        length
    } else if forward {
        -1
    } else {
        length - 1
    }
}
fn byte_search(
    haystack: &[u8],
    needle: &[u8],
    offset: usize,
    forward: bool,
    two_byte: bool,
) -> Option<usize> {
    let stride = if two_byte { UTF16_CODE_UNIT_BYTES } else { 1 };
    let needle_length = if two_byte {
        needle.len() & !1
    } else {
        needle.len()
    };
    if needle_length == 0 || needle_length > haystack.len() {
        return None;
    }
    let limit = haystack.len() - needle_length;
    let start = offset / stride * stride;
    if forward {
        (start..=limit)
            .step_by(stride)
            .find(|&index| haystack[index..index + needle_length] == needle[..needle_length])
    } else {
        (0..=start.min(limit) / stride)
            .rev()
            .map(|index| index * stride)
            .find(|&index| haystack[index..index + needle_length] == needle[..needle_length])
    }
}
