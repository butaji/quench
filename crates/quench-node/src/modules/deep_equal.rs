//! Deep structural equality for the `assert` module.
//!
//! Primitives use `Object.is` semantics in strict mode and `==` in
//! legacy mode; objects compare own enumerable string-keyed
//! properties recursively. Prototype chains are compared loosely
//! (identity only through `same_value`), which is sufficient for the
//! host's conformance scope.

use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

/// `deepStrictEqual` (strict) / `deepEqual` (legacy) comparison.
pub fn deep_equal(left: &Value, right: &Value, strict: bool) -> Result<bool, VmError> {
    deep_equal_opts(left, right, strict, false)
}

/// Partial strict equality: every structure present in `right` must occur in
/// `left`; extra properties/elements on the actual value are permitted.
pub fn partial_deep_equal(left: &Value, right: &Value) -> Result<bool, VmError> {
    let mut memo = Vec::new();
    compare_partial(left, right, &mut memo)
}

fn compare_partial(
    left: &Value,
    right: &Value,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if execute::same_value(left, right) {
        return Ok(true);
    }
    match (left, right) {
        (Value::Array(a), Value::Array(b)) => {
            if left.is_arguments_object() != right.is_arguments_object() {
                return Ok(false);
            }
            if a.logical_len() != b.logical_len() || seen(memo, left, right) {
                return Ok(a.logical_len() == b.logical_len());
            }
            for index in 0..b.logical_len() {
                let key = index.to_string();
                let left_has = execute::has_own_property(left, &key);
                let right_has = execute::has_own_property(right, &key);
                if !left_has && !right_has {
                    continue;
                }
                if !right_has || !left_has {
                    let present = if left_has {
                        execute::get_property_result(left, &key)?
                    } else if right_has {
                        execute::get_property_result(right, &key)?
                    } else {
                        Value::Undefined
                    };
                    if matches!(present, Value::Undefined) {
                        return Ok(false);
                    }
                    continue;
                }
                if !compare_partial(
                    &execute::get_property_result(left, &key)?,
                    &execute::get_property_result(right, &key)?,
                    memo,
                )? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (Value::Object(_), Value::Object(_)) => {
            if seen(memo, left, right) {
                return Ok(true);
            }
            for key in execute::own_enumerable_keys(right) {
                if !execute::has_own_property(left, &key) {
                    return Ok(false);
                }
                let left_value = execute::get_property_result(left, &key)?;
                let right_value = execute::get_property_result(right, &key)?;
                let cycle_shape_matches = same_cycle_target(&left_value, left)
                    == same_cycle_target(&right_value, right)
                    || same_cycle_target(&left_value, right)
                    || same_cycle_target(&right_value, left);
                if !cycle_shape_matches {
                    return Ok(false);
                }
                if !compare_partial(&left_value, &right_value, memo)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => compare(left, right, true, false, memo),
    }
}

/// `util.isDeepStrictEqual(left, right, options)` — `skip_prototype`
/// mirrors Node's `skipPrototype` option.
pub fn deep_equal_opts(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
) -> Result<bool, VmError> {
    let mut memo: Vec<(*const (), *const ())> = Vec::new();
    compare(left, right, strict, skip_prototype, &mut memo)
}

fn compare(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if let (Value::ObjectAlias(left_alias), Value::ObjectAlias(right_alias)) = (left, right) {
        if left_alias.0.borrow().upgrade().is_none() && right_alias.0.borrow().upgrade().is_none() {
            // Cyclic literal aliases may be unresolved after their owning
            // object has been reduced. They denote the same cycle marker.
            return Ok(true);
        }
    }
    let left = &deref_alias(left);
    let right = &deref_alias(right);
    if execute::same_value(left, right) {
        return Ok(true);
    }
    match (left, right) {
        (Value::Array(_), Value::Array(_)) => {
            compare_arrays(left, right, strict, skip_prototype, memo)
        }
        (Value::ArrayBuffer(left), Value::ArrayBuffer(right)) => {
            if strict && left.shared != right.shared {
                return Ok(false);
            }
            Ok(left.bytes.borrow().as_slice() == right.bytes.borrow().as_slice())
        }
        (Value::Object(_), Value::Object(_)) => {
            compare_objects(left, right, strict, skip_prototype, memo)
        }
        (Value::DataView(_), Value::DataView(_)) => {
            compare_data_views(left, right, strict, skip_prototype, memo)
        }
        (Value::Map(left), Value::Map(right)) => compare_maps(left, right, strict, skip_prototype, memo),
        (Value::Set(left), Value::Set(right)) => compare_sets(left, right, strict, skip_prototype, memo),
        _ if is_typed_array(left) && is_typed_array(right) => {
            compare_typed_arrays(left, right, strict, skip_prototype, memo)
        }
        _ if strict => Ok(false),
        _ => execute::abstract_equal(left, right),
    }
}

fn compare_sets(
    left: &Rc<quench_runtime::value::SetData>,
    right: &Rc<quench_runtime::value::SetData>,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if seen(
        memo,
        &Value::Set(left.clone()),
        &Value::Set(right.clone()),
    ) {
        return Ok(true);
    }
    let left_values = left.values.borrow().iter().cloned().collect::<Vec<_>>();
    let right_values = right.values.borrow().iter().cloned().collect::<Vec<_>>();
    if left_values.len() != right_values.len() || left.is_weak() != right.is_weak() {
        return Ok(false);
    }
    let mut used = vec![false; right_values.len()];
    for value in &left_values {
        let Some(index) = right_values
            .iter()
            .enumerate()
            .position(|(index, candidate)| {
                !used[index]
                    && same_cycle_target(value, &Value::Set(left.clone()))
                        == same_cycle_target(candidate, &Value::Set(right.clone()))
                    && compare(value, candidate, strict, skip_prototype, memo).unwrap_or(false)
            })
        else {
            return Ok(false);
        };
        used[index] = true;
    }
    compare_collection_properties(
        &Value::Set(left.clone()),
        &Value::Set(right.clone()),
        strict,
        skip_prototype,
        memo,
    )
}

fn compare_maps(
    left: &Rc<quench_runtime::value::MapData>,
    right: &Rc<quench_runtime::value::MapData>,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if seen(
        memo,
        &Value::Map(left.clone()),
        &Value::Map(right.clone()),
    ) {
        return Ok(true);
    }
    let left_entries = left
        .keys
        .borrow()
        .iter()
        .cloned()
        .zip(left.values.borrow().iter().cloned())
        .collect::<Vec<_>>();
    let right_entries = right
        .keys
        .borrow()
        .iter()
        .cloned()
        .zip(right.values.borrow().iter().cloned())
        .collect::<Vec<_>>();
    if left_entries.len() != right_entries.len() || left.is_weak() != right.is_weak() {
        return Ok(false);
    }
    let mut used = vec![false; right_entries.len()];
    for (key, value) in &left_entries {
        let Some(index) = right_entries
            .iter()
            .enumerate()
            .position(|(index, (candidate_key, candidate_value))| {
                !used[index]
                    && same_cycle_target(key, &Value::Map(left.clone()))
                        == same_cycle_target(candidate_key, &Value::Map(right.clone()))
                    && same_cycle_target(value, &Value::Map(left.clone()))
                        == same_cycle_target(candidate_value, &Value::Map(right.clone()))
                    && compare(key, candidate_key, strict, skip_prototype, memo).unwrap_or(false)
                    && compare(value, candidate_value, strict, skip_prototype, memo)
                        .unwrap_or(false)
            })
        else {
            return Ok(false);
        };
        used[index] = true;
    }
    compare_collection_properties(
        &Value::Map(left.clone()),
        &Value::Map(right.clone()),
        strict,
        skip_prototype,
        memo,
    )
}

fn compare_collection_properties(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    let left_keys = execute::own_enumerable_keys(left);
    let right_keys = execute::own_enumerable_keys(right);
    if left_keys.len() != right_keys.len() || left_keys.iter().any(|key| !right_keys.contains(key)) {
        return Ok(false);
    }
    for key in &left_keys {
        let left_value = execute::get_property_result(left, key)?;
        let right_value = execute::get_property_result(right, key)?;
        if !compare(&left_value, &right_value, strict, skip_prototype, memo)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn compare_data_views(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
    _memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    let (Value::DataView(a), Value::DataView(b)) = (left, right) else {
        return Ok(false);
    };
    if strict && !skip_prototype {
        let left_proto = execute::get_prototype_of(left)?;
        let right_proto = execute::get_prototype_of(right)?;
        if !execute::same_value(&left_proto, &right_proto) {
            return Ok(false);
        }
    }
    if a.byte_length != b.byte_length {
        return Ok(false);
    }
    let left_bytes = a.buffer.bytes.borrow();
    let right_bytes = b.buffer.bytes.borrow();
    let left_end = a.byte_offset.saturating_add(a.byte_length);
    let right_end = b.byte_offset.saturating_add(b.byte_length);
    if left_end > left_bytes.len() || right_end > right_bytes.len() {
        return Ok(false);
    }
    Ok(left_bytes[a.byte_offset..left_end] == right_bytes[b.byte_offset..right_end])
}

/// Typed-array views compare element content plus own symbol props.
fn compare_typed_arrays(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if std::mem::discriminant(left) != std::mem::discriminant(right) {
        return Ok(false);
    }
    if strict && !skip_prototype {
        let left_proto = execute::get_prototype_of(left)?;
        let right_proto = execute::get_prototype_of(right)?;
        if !execute::same_value(&left_proto, &right_proto) {
            return Ok(false);
        }
    }
    let Some(left_bytes) = typed_array_bytes(left) else {
        return Ok(false);
    };
    let Some(right_bytes) = typed_array_bytes(right) else {
        return Ok(false);
    };
    if left_bytes != right_bytes {
        return Ok(false);
    }
    same_enumerable_symbols(left, right, strict, skip_prototype, memo)
}

fn typed_array_bytes(value: &Value) -> Option<Vec<u8>> {
    let (buffer, offset, length, element_size) = match value {
        Value::Float64Array(view) => (&view.buffer, view.byte_offset, view.length, 8),
        Value::Float32Array(view) => (&view.buffer, view.byte_offset, view.length, 4),
        Value::Int8Array(view) => (&view.buffer, view.byte_offset, view.length, 1),
        Value::Int16Array(view) => (&view.buffer, view.byte_offset, view.length, 2),
        Value::Int32Array(view) => (&view.buffer, view.byte_offset, view.length, 4),
        Value::BigInt64Array(view) => (&view.buffer, view.byte_offset, view.length, 8),
        Value::BigUint64Array(view) => (&view.buffer, view.byte_offset, view.length, 8),
        Value::Uint32Array(view) => (&view.buffer, view.byte_offset, view.length, 4),
        Value::Uint8Array(view) => (&view.buffer, view.byte_offset, view.length, 1),
        Value::Uint8ClampedArray(view) => (&view.buffer, view.byte_offset, view.length, 1),
        Value::Uint16Array(view) => (&view.buffer, view.byte_offset, view.length, 2),
        _ => return None,
    };
    let end = offset.checked_add(length.checked_mul(element_size)?)?;
    buffer.bytes.borrow().get(offset..end).map(ToOwned::to_owned)
}

/// Both sides must own the same enumerable symbol keys with equal values.
fn same_enumerable_symbols(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    let left_symbols = execute::own_enumerable_symbol_strings(left);
    let right_symbols = execute::own_enumerable_symbol_strings(right);
    if left_symbols.len() != right_symbols.len()
        || left_symbols.iter().any(|key| !right_symbols.contains(key))
    {
        return Ok(false);
    }
    for key in &left_symbols {
        let la = execute::get_property_result(left, key)?;
        let rb = execute::get_property_result(right, key)?;
        if !compare(&la, &rb, strict, skip_prototype, memo)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn is_typed_array(value: &Value) -> bool {
    matches!(
        value,
        Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
    )
}

/// Object identity for cycle detection; `None` for non-containers.
fn deref_alias(value: &Value) -> Value {
    match value {
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map(Value::Object)
            .unwrap_or_else(|| value.clone()),
        _ => value.clone(),
    }
}
fn identity(value: &Value) -> Option<*const ()> {
    match value {
        Value::Object(object) => Some(Rc::as_ptr(object).cast()),
        Value::Array(array) => Some(Rc::as_ptr(array).cast()),
        Value::Map(map) => Some(Rc::as_ptr(map).cast()),
        Value::Set(set) => Some(Rc::as_ptr(set).cast()),
        _ => None,
    }
}

fn seen(memo: &mut Vec<(*const (), *const ())>, left: &Value, right: &Value) -> bool {
    let (Some(a), Some(b)) = (identity(left), identity(right)) else {
        return false;
    };
    if memo.contains(&(a, b)) {
        return true;
    }
    memo.push((a, b));
    false
}

fn compare_arrays(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    let (Value::Array(a), Value::Array(b)) = (left, right) else {
        return Ok(false);
    };
    if left.is_arguments_object() != right.is_arguments_object() {
        return Ok(false);
    }
    if a.logical_len() != b.logical_len() {
        return Ok(false);
    }
    if seen(memo, left, right) {
        return Ok(true);
    }
    for index in 0..a.logical_len() {
        let key = index.to_string();
        if execute::has_own_property(left, &key) != execute::has_own_property(right, &key) {
            return Ok(false);
        }
        if !execute::has_own_property(left, &key) {
            continue;
        }
        let la = execute::get_property_result(left, &key)?;
        let rb = execute::get_property_result(right, &key)?;
        if !compare(&la, &rb, strict, skip_prototype, memo)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn compare_objects(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if seen(memo, left, right) {
        return Ok(true);
    }
    if !strict && !same_loose_object_brand(left, right) {
        return Ok(false);
    }
    if strict && !same_shape(left, right, strict, skip_prototype, memo)? {
        return Ok(false);
    }
    let left_keys = execute::own_enumerable_keys(left);
    let right_keys = execute::own_enumerable_keys(right);
    if left_keys.len() != right_keys.len() || left_keys.iter().any(|key| !right_keys.contains(key))
    {
        return Ok(false);
    }
    for key in &left_keys {
        let la = execute::get_property_result(left, key)?;
        let rb = execute::get_property_result(right, key)?;
        let cycle_shape_matches = same_cycle_target(&la, left) == same_cycle_target(&rb, right)
            || same_cycle_target(&la, right)
            || same_cycle_target(&rb, left);
        if !cycle_shape_matches {
            return Ok(false);
        }
        if !compare(&la, &rb, strict, skip_prototype, memo)? {
            return Ok(false);
        }
    }
    if strict && !same_enumerable_symbols(left, right, strict, skip_prototype, memo)? {
        return Ok(false);
    }
    Ok(true)
}

fn same_cycle_target(value: &Value, target: &Value) -> bool {
    execute::same_identity(value, target)
}

fn same_loose_object_brand(left: &Value, right: &Value) -> bool {
    let left_boxed = execute::get_property(left, "_value");
    let right_boxed = execute::get_property(right, "_value");
    if matches!(left_boxed, Value::Undefined) != matches!(right_boxed, Value::Undefined) {
        return false;
    }
    let constructor_name = |value: &Value| {
        let constructor = execute::get_property(value, "constructor");
        match execute::get_property(&constructor, "name") {
            Value::String(name) => name,
            _ => String::new(),
        }
    };
    let left_name = constructor_name(left);
    let right_name = constructor_name(right);
    left_name == right_name
}

/// Strict-mode structural checks Node applies before enumerating
/// properties: prototypes, boxed `_value` payloads, `Symbol.toStringTag`,
/// and RegExp source/flags.
fn same_shape(
    left: &Value,
    right: &Value,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    let left_boxed = execute::get_property(left, "_value");
    let right_boxed = execute::get_property(right, "_value");
    if !matches!(left_boxed, Value::Undefined) || !matches!(right_boxed, Value::Undefined) {
        return compare(&left_boxed, &right_boxed, strict, skip_prototype, memo);
    }
    if !same_value_property(left, right, "source") || !same_value_property(left, right, "flags") {
        return Ok(false);
    }
    if !skip_prototype {
        let left_proto = execute::get_prototype_of(left)?;
        let right_proto = execute::get_prototype_of(right)?;
        if !execute::same_value(&left_proto, &right_proto) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether both sides read the same (strictly) value for `key`.
fn same_value_property(left: &Value, right: &Value, key: &str) -> bool {
    execute::same_value(
        &execute::get_property(left, key),
        &execute::get_property(right, key),
    )
}
