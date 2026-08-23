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
    let left = &deref_alias(left);
    let right = &deref_alias(right);
    if execute::same_value(left, right) {
        return Ok(true);
    }
    match (left, right) {
        (Value::Array(_), Value::Array(_)) => {
            compare_arrays(left, right, strict, skip_prototype, memo)
        }
        (Value::Object(_), Value::Object(_)) => {
            compare_objects(left, right, strict, skip_prototype, memo)
        }
        (Value::DataView(_), Value::DataView(_)) => {
            compare_data_views(left, right, strict, skip_prototype, memo)
        }
        _ if is_typed_array(left) && is_typed_array(right) => {
            compare_typed_arrays(left, right, strict, skip_prototype, memo)
        }
        _ if strict => Ok(false),
        _ => execute::abstract_equal(left, right),
    }
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
    let length = |value: &Value| match execute::get_property(value, "length") {
        Value::Number(n) => n as usize,
        _ => 0,
    };
    let (la, lb) = (length(left), length(right));
    if la != lb {
        return Ok(false);
    }
    for index in 0..la {
        let key = index.to_string();
        let la = execute::get_property_result(left, &key)?;
        let rb = execute::get_property_result(right, &key)?;
        if !compare(&la, &rb, strict, skip_prototype, memo)? {
            return Ok(false);
        }
    }
    same_enumerable_symbols(left, right, strict, skip_prototype, memo)
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
    if a.logical_len() != b.logical_len() {
        return Ok(false);
    }
    if seen(memo, left, right) {
        return Ok(true);
    }
    let left_keys = execute::own_enumerable_keys(left);
    let right_keys = execute::own_enumerable_keys(right);
    for index in 0..a.logical_len() {
        let key = index.to_string();
        // A sparse slot is observably different from an own `undefined`
        // element. Reading both yields `undefined`, so compare ownership
        // before comparing values.
        if left_keys.contains(&key) != right_keys.contains(&key) {
            return Ok(false);
        }
        if !left_keys.contains(&key) {
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
        if !compare(&la, &rb, strict, skip_prototype, memo)? {
            return Ok(false);
        }
    }
    if strict && !same_enumerable_symbols(left, right, strict, skip_prototype, memo)? {
        return Ok(false);
    }
    Ok(true)
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
