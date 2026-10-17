//! Native UTF-8 operations used by Node's encoding algorithms.
use crate::host::NodeHost;
use quench_runtime::{BuiltinData, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0binding:encoding";
    let shared = c.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(value);
    }
    let binding = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let u32 = get(c, global, "Uint32Array")?;
        let two = c.number(2.0);
        let results = c.construct_rooted(u32, u32, &[two])?;
        let u8 = get(c, global, "Uint8Array")?;
        let prototype = get(c, u8, "prototype")?;
        let typed = c.prototype_rooted(prototype)?;
        let set = get(c, typed, "set")?;
        let key = c.string_rooted("length");
        let descriptor = c.own_property_descriptor_rooted(typed, key)?;
        let length = get(c, descriptor, "get")?;
        let data = c.object_rooted()?;
        put(c, data, "set", set)?;
        put(c, data, "length", length)?;
        put(c, data, "results", results)?;
        let binding = c.object_rooted()?;
        put(c, binding, "encodeIntoResults", results)?;
        for (name, operation) in [
            ("encodeUtf8String", "encodingUtf8"),
            ("encodeInto", "encodingInto"),
            ("decodeUTF8", "encodingDecodeUtf8"),
            ("toASCII", "encodingToAscii"),
            ("toUnicode", "encodingToUnicode"),
        ] {
            let function =
                c.host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
            let key = c.string_rooted("name");
            let value = c.string_rooted(name);
            c.define_data_property_rooted(function, key, value, false, false, true)?;
            put(c, binding, name, function)?;
        }
        Ok(binding)
    })?;
    let retained = c.retain(binding)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.to_owned(), retained);
    Ok(binding)
}
pub(crate) fn encode(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let units = c
        .string_code_units(args[0])?
        .ok_or_else(|| RootedError::host("UTF-8 input must be a string"))?;
    c.uint8_array_rooted(String::from_utf16_lossy(&units).as_bytes())
}
pub(crate) fn encode_into(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let units = c
        .string_code_units(args[0])?
        .ok_or_else(|| RootedError::host("UTF-8 input must be a string"))?;
    let data = c.host_function_data()?;
    let length = get(c, data, "length")?;
    let length = c.call_rooted(length, args[1], &[])?;
    let capacity = c.to_number(length)? as usize;
    let mut bytes = Vec::with_capacity(capacity.min(units.len().saturating_mul(3)));
    let mut read = 0;
    for decoded in char::decode_utf16(units.iter().copied()) {
        let valid = decoded.is_ok();
        let character = decoded.unwrap_or(char::REPLACEMENT_CHARACTER);
        if character.len_utf8() > capacity - bytes.len() {
            break;
        }
        let mut encoded = [0; 4];
        bytes.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
        read += if valid { character.len_utf16() } else { 1 };
    }
    // An empty/detached destination must not invoke TypedArray.set.
    if !bytes.is_empty() {
        let source = c.uint8_array_rooted(&bytes)?;
        let set = get(c, data, "set")?;
        c.call_rooted(set, args[1], &[source])?;
    }
    let results = get(c, data, "results")?;
    for (index, count) in [("0", read), ("1", bytes.len())] {
        let key = c.string_rooted(index);
        let value = c.number(count as f64);
        c.set_property_rooted(results, key, value, results)?;
    }
    Ok(c.undefined())
}
pub(crate) fn decode(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(BuiltinData::Bytes { bytes, .. }) = c.builtin_data_rooted(args[0])? else {
        let error = c.type_error_rooted("The \"list\" argument must be an instance of SharedArrayBuffer, ArrayBuffer or ArrayBufferView.")?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_ARG_TYPE")?;
        return Err(c.throw(error));
    };
    let is_true = |c: &Context<'_>, index| {
        args.get(index)
            .and_then(|value| c.rooted_value(*value))
            .is_some_and(|v| v.as_bool() == Some(true))
    };
    let ignore_bom = is_true(c, 1);
    let fatal = is_true(c, 2);
    let bytes = if !ignore_bom {
        bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)
    } else {
        &bytes
    };
    let text = if fatal {
        match std::str::from_utf8(bytes) {
            Ok(text) => text.into(),
            Err(_) => {
                let error =
                    c.type_error_rooted("The encoded data was not valid for encoding utf-8")?;
                super::node_errors_shared_vm::decorate_type_error(
                    c,
                    error,
                    "ERR_ENCODING_INVALID_ENCODED_DATA",
                )?;
                return Err(c.throw(error));
            }
        }
    } else {
        String::from_utf8_lossy(bytes)
    };
    Ok(c.string_rooted(&text))
}

/// `toASCII(domain)`: UTS #46 ToASCII, or the empty string on failure (`ada::idna::to_ascii`).
pub(crate) fn to_ascii(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = match args.first() {
        Some(value) => c.to_string(*value)?,
        None => String::new(),
    };
    let ascii = idna::domain_to_ascii(&input).unwrap_or_default();
    Ok(c.string_rooted(&ascii))
}

/// `toUnicode(domain)`: UTS #46 ToUnicode (`ada::idna::to_unicode`).
pub(crate) fn to_unicode(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = match args.first() {
        Some(value) => c.to_string(*value)?,
        None => String::new(),
    };
    Ok(c.string_rooted(&idna::domain_to_unicode(&input).0))
}
