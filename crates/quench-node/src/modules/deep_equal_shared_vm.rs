//! Node's deep equality (`lib/internal/util/comparisons.js`): `isDeepStrictEqual` and the
//! legacy loose `isDeepEqual`.
//!
//! Strict mode compares primitives with SameValue and requires equal prototypes; loose mode
//! compares primitives with `==` and ignores prototypes. Both require equal
//! `Object.prototype.toString` tags, compare dates, regular expressions, errors, boxed
//! primitives and binary data by content, maps and sets without order, and objects by their
//! own enumerable keys (strict mode adds enumerable symbols). A pair already being compared
//! is equal, which ends cycles.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Loose,
    Strict,
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

/// `typeof value === 'object' && value !== null`.
fn is_object(c: &Context<'_>, value: RootId) -> bool {
    c.value_type_rooted(value) == Some(ValueType::Object)
}

fn is_nan(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).and_then(|value| value.as_number()).is_some_and(f64::is_nan)
}

/// Whether `left` and `right` are deeply equal in `mode`.
pub(crate) fn equal(c: &mut Context<'_>, left: RootId, right: RootId, mode: Mode) -> Result<bool, RootedError> {
    let mut comparing = Vec::new();
    compare(c, left, right, mode, &mut comparing)
}

fn compare(c: &mut Context<'_>, left: RootId, right: RootId, mode: Mode, comparing: &mut Vec<(RootId, RootId)>) -> Result<bool, RootedError> {
    match mode {
        Mode::Strict if c.same_value_rooted(left, right)? => return Ok(true),
        Mode::Loose if c.strict_equal_rooted(left, right)? => return Ok(true),
        _ => {}
    }
    if !is_object(c, left) || !is_object(c, right) {
        if mode == Mode::Loose && !is_object(c, left) && !is_object(c, right) {
            return Ok(c.equal_rooted(left, right)? || (is_nan(c, left) && is_nan(c, right)));
        }
        return Ok(false);
    }
    for &(seen_left, seen_right) in comparing.iter() {
        if c.strict_equal_rooted(seen_left, left)? && c.strict_equal_rooted(seen_right, right)? {
            return Ok(true);
        }
    }
    comparing.push((left, right));
    let result = compare_objects(c, left, right, mode, comparing);
    comparing.pop();
    result
}

fn tag(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let prototype = get(c, object, "prototype")?;
    let to_string = get(c, prototype, "toString")?;
    let tag = c.call_rooted(to_string, value, &[])?;
    c.to_string(tag)
}

fn both(c: &mut Context<'_>, left: RootId, right: RootId, brand: BuiltinBrand) -> Result<(bool, bool), RootedError> {
    Ok((c.has_builtin_brand_rooted(left, brand)?, c.has_builtin_brand_rooted(right, brand)?))
}

fn call_method(c: &mut Context<'_>, value: RootId, owner: &str, method: &str) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let constructor = get(c, global, owner)?;
    let prototype = get(c, constructor, "prototype")?;
    let function = get(c, prototype, method)?;
    c.call_rooted(function, value, &[])
}

fn compare_objects(c: &mut Context<'_>, left: RootId, right: RootId, mode: Mode, comparing: &mut Vec<(RootId, RootId)>) -> Result<bool, RootedError> {
    if mode == Mode::Strict {
        let left_prototype = c.prototype_rooted(left)?;
        let right_prototype = c.prototype_rooted(right)?;
        if !c.strict_equal_rooted(left_prototype, right_prototype)? {
            return Ok(false);
        }
    }
    if tag(c, left)? != tag(c, right)? {
        return Ok(false);
    }
    let left_array = c.is_array_rooted(left)?;
    if left_array != c.is_array_rooted(right)? {
        return Ok(false);
    }
    if left_array {
        let left_length = get(c, left, "length")?;
        let right_length = get(c, right, "length")?;
        if !c.strict_equal_rooted(left_length, right_length)? {
            return Ok(false);
        }
        return keys_equal(c, left, right, mode, comparing);
    }
    match both(c, left, right, BuiltinBrand::Date)? {
        (true, true) => {
            let left_time = call_method(c, left, "Date", "getTime")?;
            let right_time = call_method(c, right, "Date", "getTime")?;
            if !c.same_value_rooted(left_time, right_time)? {
                return Ok(false);
            }
        }
        (false, false) => {}
        _ => return Ok(false),
    }
    match both(c, left, right, BuiltinBrand::RegExp)? {
        (true, true) => {
            for name in ["source", "flags", "lastIndex"] {
                let left_part = get(c, left, name)?;
                let right_part = get(c, right, name)?;
                if !c.strict_equal_rooted(left_part, right_part)? {
                    return Ok(false);
                }
            }
        }
        (false, false) => {}
        _ => return Ok(false),
    }
    match both(c, left, right, BuiltinBrand::NativeError)? {
        (true, true) => {
            for name in ["message", "name"] {
                let left_part = get(c, left, name)?;
                let right_part = get(c, right, name)?;
                if !compare(c, left_part, right_part, mode, comparing)? {
                    return Ok(false);
                }
            }
        }
        (false, false) => {}
        _ => return Ok(false),
    }
    match both(c, left, right, BuiltinBrand::BoxedPrimitive)? {
        (true, true) => {
            let left_value = boxed_value(c, left)?;
            let right_value = boxed_value(c, right)?;
            if !c.same_value_rooted(left_value, right_value)? {
                return Ok(false);
            }
        }
        (false, false) => {}
        _ => return Ok(false),
    }
    let left_binary = c.has_builtin_brand_rooted(left, BuiltinBrand::ArrayBufferView)? || c.has_builtin_brand_rooted(left, BuiltinBrand::AnyArrayBuffer)?;
    let right_binary = c.has_builtin_brand_rooted(right, BuiltinBrand::ArrayBufferView)? || c.has_builtin_brand_rooted(right, BuiltinBrand::AnyArrayBuffer)?;
    if left_binary != right_binary {
        return Ok(false);
    }
    if left_binary {
        let left_bytes = bytes(c, left)?;
        if left_bytes != bytes(c, right)? {
            return Ok(false);
        }
        return keys_equal(c, left, right, mode, comparing);
    }
    let left_collection = c.collection_preview_rooted(left)?;
    let right_collection = c.collection_preview_rooted(right)?;
    match (left_collection, right_collection) {
        (Some(left_entries), Some(right_entries)) => {
            if left_entries.is_key_value != right_entries.is_key_value || left_entries.entries.len() != right_entries.entries.len() {
                return Ok(false);
            }
            let width = if left_entries.is_key_value { 2 } else { 1 };
            if !entries_match(c, &left_entries.entries, &right_entries.entries, width, mode, comparing)? {
                return Ok(false);
            }
        }
        (None, None) => {}
        _ => return Ok(false),
    }
    keys_equal(c, left, right, mode, comparing)
}

/// A boxed primitive's value (`valueOf` of its class).
fn boxed_value(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    match c.builtin_data_rooted(value)? {
        Some(quench_runtime::BuiltinData::Primitive { value, .. }) => Ok(value),
        _ => Ok(c.undefined()),
    }
}

fn bytes(c: &mut Context<'_>, value: RootId) -> Result<Vec<u8>, RootedError> {
    let bytes = if c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)? {
        c.with_view_bytes_rooted(value, <[u8]>::to_vec)
    } else {
        c.with_buffer_bytes_rooted(value, <[u8]>::to_vec)
    };
    Ok(bytes.unwrap_or_default())
}

/// Every entry (a value, or a key and value when `width` is 2) of `left` matches a distinct
/// entry of `right`.
fn entries_match(c: &mut Context<'_>, left: &[RootId], right: &[RootId], width: usize, mode: Mode, comparing: &mut Vec<(RootId, RootId)>) -> Result<bool, RootedError> {
    let mut used = vec![false; right.len() / width];
    'entries: for entry in left.chunks(width) {
        for (index, candidate) in right.chunks(width).enumerate() {
            if used[index] {
                continue;
            }
            let mut matched = true;
            for (part, other) in entry.iter().zip(candidate) {
                if !compare(c, *part, *other, mode, comparing)? {
                    matched = false;
                    break;
                }
            }
            if matched {
                used[index] = true;
                continue 'entries;
            }
        }
        return Ok(false);
    }
    Ok(true)
}

/// The own enumerable keys agree in number and membership, and their values deeply.
fn keys_equal(c: &mut Context<'_>, left: RootId, right: RootId, mode: Mode, comparing: &mut Vec<(RootId, RootId)>) -> Result<bool, RootedError> {
    let symbols = mode == Mode::Strict;
    let left_keys = c.enumerable_own_keys_rooted(left, symbols, true)?;
    let right_keys = c.enumerable_own_keys_rooted(right, symbols, true)?;
    if left_keys.len() != right_keys.len() {
        return Ok(false);
    }
    for key in left_keys {
        let Some(fields) = c.own_property_fields_rooted(right, key)? else { return Ok(false) };
        if fields.enumerable != Some(true) {
            return Ok(false);
        }
        let left_value = c.get_property_rooted(left, key)?;
        let right_value = c.get_property_rooted(right, key)?;
        if !compare(c, left_value, right_value, mode, comparing)? {
            return Ok(false);
        }
    }
    Ok(true)
}
