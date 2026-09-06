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
    if is_typed_array(left) && is_typed_array(right) {
        return partial_typed_array(left, right, memo);
    }
    if is_date_value(left) != is_date_value(right) && (is_date_value(left) || is_date_value(right))
    {
        return Ok(false);
    }
    if quench_runtime::regexp::has_regexp_internal_slot(left)
        != quench_runtime::regexp::has_regexp_internal_slot(right)
    {
        return Ok(false);
    }
    let error_like = |value: &Value| {
        matches!(
            execute::get_property(value, "\0error_slot"),
            Value::Boolean(true)
        ) || (execute::has_own_property(value, "message")
            && matches!(execute::get_prototype_of(value), Ok(Value::Null)))
    };
    if error_like(left) != error_like(right) {
        return Ok(false);
    }
    if is_url_like(left) || is_url_like(right) {
        return compare_url_like(left, right);
    }
    if is_crypto_key(left) || is_crypto_key(right) {
        return if is_crypto_key(left) && is_crypto_key(right) {
            compare(
                &execute::get_property(left, crate::modules::crypto::KEY_DATA_PROP),
                &execute::get_property(right, crate::modules::crypto::KEY_DATA_PROP),
                true,
                false,
                memo,
            )
        } else {
            Ok(false)
        };
    }
    if is_webcrypto_key(left) || is_webcrypto_key(right) {
        return compare_webcrypto_keys(left, right, memo);
    }
    match (left, right) {
        (Value::Array(a), Value::Array(b)) => {
            if left.is_arguments_object() != right.is_arguments_object() {
                return Ok(false);
            }
            if a.logical_len() < b.logical_len() {
                return Ok(false);
            }
            if seen(memo, left, right) {
                return Ok(true);
            }
            let expected_extra = execute::own_enumerable_keys(right)
                .into_iter()
                .filter(|key| {
                    key.parse::<usize>()
                        .map_or(true, |index| index >= b.logical_len())
                })
                .collect::<Vec<_>>();
            for key in expected_extra {
                if !execute::has_own_property(left, &key)
                    || !compare_partial(
                        &execute::get_property_result(left, &key)?,
                        &execute::get_property_result(right, &key)?,
                        memo,
                    )?
                {
                    return Ok(false);
                }
            }
            if a.logical_len() > b.logical_len()
                && (0..b.logical_len())
                    .all(|index| execute::has_own_property(right, &index.to_string()))
            {
                let mut cursor = 0;
                for index in 0..b.logical_len() {
                    let expected = execute::get_property_result(right, &index.to_string())?;
                    let mut matched = false;
                    while cursor < a.logical_len() {
                        let key = cursor.to_string();
                        cursor += 1;
                        if execute::has_own_property(left, &key)
                            && compare_partial(
                                &execute::get_property_result(left, &key)?,
                                &expected,
                                memo,
                            )?
                        {
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        return Ok(false);
                    }
                }
                return Ok(true);
            }
            for index in 0..b.logical_len() {
                let key = index.to_string();
                let left_has = execute::has_own_property(left, &key);
                let right_has = execute::has_own_property(right, &key);
                if !left_has && !right_has {
                    continue;
                }
                if left_has && right_has {
                    if !compare_partial(
                        &execute::get_property_result(left, &key)?,
                        &execute::get_property_result(right, &key)?,
                        memo,
                    )? {
                        return Ok(false);
                    }
                    continue;
                }
                if left_has {
                    return Ok(true);
                }
                let present = execute::get_property_result(right, &key)?;
                if matches!(present, Value::Undefined) {
                    return Ok(false);
                }
                let prior_overlap = (0..index).any(|prior| {
                    let key = prior.to_string();
                    execute::has_own_property(left, &key) && execute::has_own_property(right, &key)
                });
                if prior_overlap {
                    return Ok(false);
                }
                let later_left_extra = ((index + 1)..b.logical_len()).any(|later| {
                    let key = later.to_string();
                    execute::has_own_property(left, &key) && !execute::has_own_property(right, &key)
                });
                if later_left_extra {
                    return Ok(true);
                }
                let later_overlap = ((index + 1)..b.logical_len()).any(|later| {
                    let key = later.to_string();
                    execute::has_own_property(left, &key) && execute::has_own_property(right, &key)
                });
                if later_overlap {
                    return Ok(false);
                }
                return Ok(true);
            }
            for index in 0..b.logical_len() {
                let key = index.to_string();
                if !execute::has_own_property(left, &key) {
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
            for key in execute::own_enumerable_keys(right) {
                if key.parse::<usize>().is_ok() {
                    continue;
                }
                if !execute::own_enumerable_keys(left).contains(&key)
                    || !compare_partial(
                        &execute::get_property_result(left, &key)?,
                        &execute::get_property_result(right, &key)?,
                        memo,
                    )?
                {
                    return Ok(false);
                }
            }
            partial_enumerable_symbols(left, right)
        }
        (_, _) if is_typed_array(left) && is_typed_array(right) => {
            partial_typed_array(left, right, memo)
        }
        (Value::ArrayBuffer(left), Value::ArrayBuffer(right)) => {
            if left.shared != right.shared {
                return Ok(false);
            }
            let left_bytes = left.bytes.borrow();
            let right_bytes = right.bytes.borrow();
            Ok(left_bytes.len() >= right_bytes.len()
                && left_bytes[..right_bytes.len()] == right_bytes[..])
        }
        (Value::Map(left), Value::Map(right)) => partial_maps(left, right, memo),
        (Value::Set(left), Value::Set(right)) => partial_sets(left, right, memo),
        (Value::Object(_), Value::Object(_)) => {
            if is_url_like(left) || is_url_like(right) {
                return compare_url_like(left, right);
            }
            let left_boxed = execute::get_property(left, "_value");
            let right_boxed = execute::get_property(right, "_value");
            let left_is_boxed = !matches!(left_boxed, Value::Undefined);
            let right_is_boxed = !matches!(right_boxed, Value::Undefined);
            if left_is_boxed != right_is_boxed {
                return Ok(false);
            }
            if left_is_boxed && !compare(&left_boxed, &right_boxed, true, false, memo)? {
                return Ok(false);
            }
            if is_error_value(left)
                || is_error_value(right)
                || is_date_value(left)
                || is_date_value(right)
                || (quench_runtime::regexp::has_regexp_internal_slot(left)
                    || quench_runtime::regexp::has_regexp_internal_slot(right))
            {
                if is_error_value(left) || is_error_value(right) {
                    return compare_partial_error(left, right, memo);
                }
                return compare(left, right, true, false, memo);
            }
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
            if !partial_enumerable_symbols(left, right)? {
                return Ok(false);
            }
            Ok(true)
        }
        _ => compare(left, right, true, false, memo),
    }
}

fn compare_partial_error(
    left: &Value,
    right: &Value,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if !is_error_value(left) || !is_error_value(right) {
        return Ok(false);
    }
    if !same_property(left, right, "name") {
        return Ok(false);
    }
    if has_own_slot(right, "message") && !same_property(left, right, "message") {
        return Ok(false);
    }
    if has_own_slot(right, "cause") {
        let left_cause = execute::get_property_result(left, "cause")?;
        let right_cause = execute::get_property_result(right, "cause")?;
        if !has_own_slot(left, "cause") || !compare_partial(&left_cause, &right_cause, memo)? {
            return Ok(false);
        }
    }
    if is_aggregate_error(right) {
        let left_errors = execute::get_property_result(left, "errors")?;
        let right_errors = execute::get_property_result(right, "errors")?;
        if !compare_partial(&left_errors, &right_errors, memo)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn partial_typed_array(
    left: &Value,
    right: &Value,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if std::mem::discriminant(left) != std::mem::discriminant(right) {
        return Ok(false);
    }
    if value_is_float16(left) != value_is_float16(right) {
        return Ok(false);
    }
    let left_len = typed_array_length(left).unwrap_or(0);
    let right_len = typed_array_length(right).unwrap_or(0);
    if left_len < right_len || seen(memo, left, right) {
        return Ok(left_len >= right_len);
    }
    if left_len == right_len {
        if let (
            Some((left_buffer, left_start, left_end)),
            Some((right_buffer, right_start, right_end)),
        ) = (typed_array_bytes(left), typed_array_bytes(right))
        {
            let left_bytes = left_buffer.bytes.borrow();
            let right_bytes = right_buffer.bytes.borrow();
            if left_bytes.get(left_start..left_end) == right_bytes.get(right_start..right_end) {
                return partial_enumerable_symbols(left, right);
            }
        }
    }
    let mut cursor = 0;
    for index in 0..right_len {
        let expected = execute::get_property_result(right, &index.to_string())?;
        let mut matched = false;
        while cursor < left_len {
            let actual = execute::get_property_result(left, &cursor.to_string())?;
            cursor += 1;
            if compare_partial(&actual, &expected, memo)? {
                matched = true;
                break;
            }
        }
        if !matched {
            return Ok(false);
        }
    }
    partial_enumerable_symbols(left, right)
}

fn typed_array_length(value: &Value) -> Option<usize> {
    match value {
        Value::Float64Array(v) => Some(v.logical_len()),
        Value::Float32Array(v) => Some(v.logical_len()),
        Value::Int8Array(v) => Some(v.logical_len()),
        Value::Int16Array(v) => Some(v.logical_len()),
        Value::Int32Array(v) => Some(v.logical_len()),
        Value::BigInt64Array(v) => Some(v.logical_len()),
        Value::BigUint64Array(v) => Some(v.logical_len()),
        Value::Uint32Array(v) => Some(v.logical_len()),
        Value::Uint8Array(v) => Some(v.logical_len()),
        Value::Uint8ClampedArray(v) => Some(v.logical_len()),
        Value::Uint16Array(v) => Some(v.logical_len()),
        _ => None,
    }
}

fn partial_maps(
    left: &Rc<quench_runtime::value::MapData>,
    right: &Rc<quench_runtime::value::MapData>,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if left.is_weak() || right.is_weak() {
        return Ok(false);
    }
    if seen(memo, &Value::Map(left.clone()), &Value::Map(right.clone())) {
        return Ok(true);
    }
    let left_keys = left.keys.borrow();
    let left_values = left.values.borrow();
    let right_keys = right.keys.borrow();
    let right_values = right.values.borrow();
    let mut used = vec![false; left_keys.len()];
    for (expected_key, expected_value) in right_keys.iter().zip(right_values.iter()) {
        let mut matched_index = None;
        for index in 0..left_keys.len() {
            if used[index]
                || same_cycle_target(&left_keys[index], &Value::Map(left.clone()))
                    != same_cycle_target(expected_key, &Value::Map(right.clone()))
                || same_cycle_target(&left_values[index], &Value::Map(left.clone()))
                    != same_cycle_target(expected_value, &Value::Map(right.clone()))
            {
                continue;
            }
            let mut candidate_memo = memo.clone();
            if compare_partial(&left_keys[index], expected_key, &mut candidate_memo)?
                && compare_partial(&left_values[index], expected_value, &mut candidate_memo)?
            {
                *memo = candidate_memo;
                matched_index = Some(index);
                break;
            }
        }
        if let Some(index) = matched_index {
            used[index] = true;
        } else {
            return Ok(false);
        }
    }
    partial_collection_properties(&Value::Map(left.clone()), &Value::Map(right.clone()), memo)
}

fn partial_collection_properties(
    left: &Value,
    right: &Value,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    for key in execute::own_enumerable_keys(right) {
        let left_value = execute::get_property_result(left, &key)?;
        let right_value = execute::get_property_result(right, &key)?;
        if key.parse::<usize>().is_ok()
            && execute::same_value(&left_value, left)
            && execute::same_value(&right_value, right)
        {
            continue;
        }
        if !execute::has_own_property(left, &key)
            || !compare_partial(&left_value, &right_value, memo)?
        {
            return Ok(false);
        }
    }
    partial_enumerable_symbols(left, right)
}

fn partial_sets(
    left: &Rc<quench_runtime::value::SetData>,
    right: &Rc<quench_runtime::value::SetData>,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if left.is_weak() || right.is_weak() {
        return Ok(false);
    }
    if seen(memo, &Value::Set(left.clone()), &Value::Set(right.clone())) {
        return Ok(true);
    }
    let left_values = left.values.borrow();
    let right_values = right.values.borrow();
    let mut used = vec![false; left_values.len()];
    for expected in right_values.iter() {
        let mut matched_index = None;
        for index in 0..left_values.len() {
            if used[index]
                || same_cycle_target(&left_values[index], &Value::Set(left.clone()))
                    != same_cycle_target(expected, &Value::Set(right.clone()))
            {
                continue;
            }
            let mut candidate_memo = memo.clone();
            if compare_partial(&left_values[index], expected, &mut candidate_memo)? {
                *memo = candidate_memo;
                matched_index = Some(index);
                break;
            }
        }
        if let Some(index) = matched_index {
            used[index] = true;
        } else {
            return Ok(false);
        }
    }
    partial_collection_properties(&Value::Set(left.clone()), &Value::Set(right.clone()), memo)
}

fn is_date_value(value: &Value) -> bool {
    matches!(execute::get_property(value, "timeValue"), Value::Number(_))
        && matches!(
            execute::get_prototype_of(value),
            Ok(Value::Builtin(quench_runtime::ops::Builtin::DatePrototype))
        )
}

fn is_url_like(value: &Value) -> bool {
    matches!(
        execute::get_property(value, "\0url"),
        Value::String(_) | Value::StringUnits(_)
    ) || (matches!(
        execute::get_property(value, "href"),
        Value::String(_) | Value::StringUnits(_)
    ) && matches!(
        execute::get_property(value, "protocol"),
        Value::String(_) | Value::StringUnits(_)
    ))
}

fn compare_url_like(left: &Value, right: &Value) -> Result<bool, VmError> {
    if !is_url_like(left) || !is_url_like(right) {
        return Ok(false);
    }
    for key in [
        "href", "protocol", "username", "password", "host", "hostname", "port", "pathname",
        "search", "hash", "origin",
    ] {
        if !execute::same_value(
            &execute::get_property(left, key),
            &execute::get_property(right, key),
        ) {
            return Ok(false);
        }
    }
    let left_keys = execute::own_enumerable_keys(left);
    let right_keys = execute::own_enumerable_keys(right);
    if left_keys.len() != right_keys.len() || left_keys.iter().any(|key| !right_keys.contains(key))
    {
        return Ok(false);
    }
    for key in left_keys {
        if !execute::same_value(
            &execute::get_property(left, &key),
            &execute::get_property(right, &key),
        ) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn same_property(left: &Value, right: &Value, key: &str) -> bool {
    execute::same_value(
        &execute::get_property(left, key),
        &execute::get_property(right, key),
    )
}

fn is_error_value(value: &Value) -> bool {
    let mut current = execute::get_prototype_of(value).ok();
    while let Some(prototype) = current {
        if matches!(
            prototype,
            Value::Builtin(
                quench_runtime::ops::Builtin::ErrorPrototype
                    | quench_runtime::ops::Builtin::TypeErrorPrototype
                    | quench_runtime::ops::Builtin::RangeErrorPrototype
                    | quench_runtime::ops::Builtin::ReferenceErrorPrototype
                    | quench_runtime::ops::Builtin::SyntaxErrorPrototype
                    | quench_runtime::ops::Builtin::EvalErrorPrototype
                    | quench_runtime::ops::Builtin::URIErrorPrototype
            )
        ) {
            return true;
        }
        current = execute::get_prototype_of(&prototype).ok();
    }
    false
}

fn is_crypto_key(value: &Value) -> bool {
    matches!(
        execute::get_property(value, crate::modules::crypto::KEY_MARKER_PROP),
        Value::Boolean(true)
    )
}

fn is_webcrypto_key(value: &Value) -> bool {
    matches!(
        execute::get_property(value, crate::modules::webcrypto::KEY_MARKER_PROP),
        Value::Boolean(true)
    )
}

fn compare_webcrypto_keys(
    left: &Value,
    right: &Value,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if !is_webcrypto_key(left) || !is_webcrypto_key(right) {
        return Ok(false);
    }
    let left_data = execute::get_property(left, crate::modules::webcrypto::KEY_DATA_PROP);
    let right_data = execute::get_property(right, crate::modules::webcrypto::KEY_DATA_PROP);
    if !compare(&left_data, &right_data, true, false, memo)? {
        return Ok(false);
    }
    let left_meta = execute::get_property(left, "\0quench:webcrypto:key-meta");
    let right_meta = execute::get_property(right, "\0quench:webcrypto:key-meta");
    if !compare(&left_meta, &right_meta, true, false, memo)? {
        return Ok(false);
    }
    compare_objects(left, right, true, false, memo)
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
    if let Value::Proxy(proxy) = left {
        if !*proxy.revoked.borrow() {
            execute::validate_proxy(left)?;
            return compare(&proxy.target, right, strict, skip_prototype, memo);
        }
    }
    if let Value::Proxy(proxy) = right {
        if !*proxy.revoked.borrow() {
            execute::validate_proxy(right)?;
            return compare(left, &proxy.target, strict, skip_prototype, memo);
        }
    }
    if let (Value::ObjectAlias(left_alias), Value::ObjectAlias(right_alias)) = (left, right) {
        if left_alias.0.borrow().upgrade().is_none() && right_alias.0.borrow().upgrade().is_none() {
            // Cyclic literal aliases may be unresolved after their owning
            // object has been reduced. They denote the same cycle marker.
            return Ok(true);
        }
    }
    if unresolved_alias(left) || unresolved_alias(right) {
        let left_unresolved = unresolved_alias(left);
        let concrete = if left_unresolved { right } else { left };
        if let Some(pointer) = identity(concrete) {
            let is_current_cycle_target = memo.last().is_some_and(|(a, b)| {
                if left_unresolved {
                    *a == pointer
                } else {
                    *b == pointer
                }
            });
            if is_current_cycle_target {
                return Ok(true);
            }
        }
    }
    let left = &deref_alias(left);
    let right = &deref_alias(right);
    if execute::same_value(left, right) {
        return Ok(true);
    }
    // Typed-array identity and element semantics are self-contained.  Handle
    // them before the generic object probes below (date/regexp/error/url
    // markers), whose VM property lookups are disproportionately expensive
    // for large views and add no information for this value family.
    if is_typed_array(left) && is_typed_array(right) {
        return compare_typed_arrays(left, right, strict, skip_prototype, memo);
    }
    if is_date_value(left) != is_date_value(right) && (is_date_value(left) || is_date_value(right))
    {
        return Ok(false);
    }
    let left_regexp = quench_runtime::regexp::has_regexp_internal_slot(left);
    let right_regexp = quench_runtime::regexp::has_regexp_internal_slot(right);
    if left_regexp != right_regexp {
        return Ok(false);
    }
    let error_like = |value: &Value| {
        matches!(
            execute::get_property(value, "\0error_slot"),
            Value::Boolean(true)
        ) || (execute::has_own_property(value, "message")
            && matches!(execute::get_prototype_of(value), Ok(Value::Null)))
    };
    if error_like(left) != error_like(right) {
        return Ok(false);
    }
    if is_url_like(left) || is_url_like(right) {
        return compare_url_like(left, right);
    }
    if is_crypto_key(left) || is_crypto_key(right) {
        return if is_crypto_key(left) && is_crypto_key(right) {
            compare(
                &execute::get_property(left, crate::modules::crypto::KEY_DATA_PROP),
                &execute::get_property(right, crate::modules::crypto::KEY_DATA_PROP),
                true,
                false,
                memo,
            )
        } else {
            Ok(false)
        };
    }
    if is_webcrypto_key(left) || is_webcrypto_key(right) {
        return compare_webcrypto_keys(left, right, memo);
    }
    match (left, right) {
        (Value::Object(_), Value::Object(_)) if is_date_value(left) && is_date_value(right) => {
            if !same_property(left, right, "timeValue") {
                return Ok(false);
            }
            compare_objects(left, right, strict, skip_prototype, memo)
        }
        (Value::Object(_), Value::Object(_))
            if quench_runtime::regexp::has_regexp_internal_slot(left)
                && quench_runtime::regexp::has_regexp_internal_slot(right) =>
        {
            if !same_property(left, right, "source") || !same_property(left, right, "flags") {
                return Ok(false);
            }
            if !same_property(left, right, "lastIndex") {
                return Ok(false);
            }
            compare_objects(left, right, strict, skip_prototype, memo)
        }
        (Value::Array(_), Value::Array(_)) => {
            compare_arrays(left, right, strict, skip_prototype, memo)
        }
        (Value::ArrayBuffer(left), Value::ArrayBuffer(right)) => {
            if left.shared != right.shared {
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
        (Value::Map(left), Value::Map(right)) if left.is_weak() || right.is_weak() => Ok(false),
        (Value::Map(left), Value::Map(right)) => {
            compare_maps(left, right, strict, skip_prototype, memo)
        }
        (Value::Set(left), Value::Set(right)) if left.is_weak() || right.is_weak() => Ok(false),
        (Value::Set(left), Value::Set(right)) => {
            compare_sets(left, right, strict, skip_prototype, memo)
        }
        _ if is_typed_array(left) && is_typed_array(right) => {
            compare_typed_arrays(left, right, strict, skip_prototype, memo)
        }
        _ if strict => Ok(false),
        _ if is_primitive(left) && is_primitive(right) => execute::abstract_equal(left, right),
        _ => Ok(false),
    }
}

fn is_primitive(value: &Value) -> bool {
    matches!(
        value,
        Value::Number(_)
            | Value::Boolean(_)
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::BigInt(_)
            | Value::Null
            | Value::Undefined
    )
}

fn compare_sets(
    left: &Rc<quench_runtime::value::SetData>,
    right: &Rc<quench_runtime::value::SetData>,
    strict: bool,
    skip_prototype: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if seen(memo, &Value::Set(left.clone()), &Value::Set(right.clone())) {
        return Ok(true);
    }
    let left_values = left.values.borrow().iter().cloned().collect::<Vec<_>>();
    let right_values = right.values.borrow().iter().cloned().collect::<Vec<_>>();
    if left_values.len() != right_values.len() || left.is_weak() != right.is_weak() {
        return Ok(false);
    }
    let mut used = vec![false; right_values.len()];
    for value in &left_values {
        let mut matched = None;
        for (index, candidate) in right_values.iter().enumerate() {
            if used[index]
                || same_cycle_target(value, &Value::Set(left.clone()))
                    != same_cycle_target(candidate, &Value::Set(right.clone()))
            {
                continue;
            }
            let mut candidate_memo = memo.clone();
            if compare(
                value,
                candidate,
                strict,
                skip_prototype,
                &mut candidate_memo,
            )? {
                *memo = candidate_memo;
                matched = Some(index);
                break;
            }
        }
        let Some(index) = matched else {
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
    if seen(memo, &Value::Map(left.clone()), &Value::Map(right.clone())) {
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
        let mut matched = None;
        for (index, (candidate_key, candidate_value)) in right_entries.iter().enumerate() {
            if used[index]
                || same_cycle_target(key, &Value::Map(left.clone()))
                    != same_cycle_target(candidate_key, &Value::Map(right.clone()))
                || same_cycle_target(value, &Value::Map(left.clone()))
                    != same_cycle_target(candidate_value, &Value::Map(right.clone()))
            {
                continue;
            }
            let mut candidate_memo = memo.clone();
            if compare(
                key,
                candidate_key,
                strict,
                skip_prototype,
                &mut candidate_memo,
            )? && compare(
                value,
                candidate_value,
                strict,
                skip_prototype,
                &mut candidate_memo,
            )? {
                *memo = candidate_memo;
                matched = Some(index);
                break;
            }
        }
        let Some(index) = matched else {
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
    let left_keys = execute::own_enumerable_keys(left)
        .into_iter()
        .filter(|key| strict || !key.starts_with("Symbol."))
        .collect::<Vec<_>>();
    let right_keys = execute::own_enumerable_keys(right)
        .into_iter()
        .filter(|key| strict || !key.starts_with("Symbol."))
        .collect::<Vec<_>>();
    if left_keys.len() != right_keys.len() || left_keys.iter().any(|key| !right_keys.contains(key))
    {
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
    if is_float_typed_array(left) {
        let length = typed_array_length(left).unwrap_or(0);
        if typed_array_length(right).unwrap_or(0) != length {
            return Ok(false);
        }
        // Equal floating-point views are overwhelmingly the common case in
        // deep-comparison workloads.  Compare their backing bytes first so
        // large typed arrays do not cross the VM property boundary once per
        // element.  A byte mismatch falls back to element semantics below,
        // preserving SameValue/SameValueZero handling for signed zero and
        // NaN payloads.
        if let (
            Some((left_buffer, left_start, left_end)),
            Some((right_buffer, right_start, right_end)),
        ) = (typed_array_bytes(left), typed_array_bytes(right))
        {
            let left_bytes = left_buffer.bytes.borrow();
            let right_bytes = right_buffer.bytes.borrow();
            if left_bytes.get(left_start..left_end) == right_bytes.get(right_start..right_end) {
                return if strict {
                    same_enumerable_symbols_shallow(left, right)
                } else {
                    Ok(true)
                };
            }
        }
        for index in 0..length {
            let left_value = execute::get_property(left, &index.to_string());
            let right_value = execute::get_property(right, &index.to_string());
            let equal = if strict {
                execute::same_value(&left_value, &right_value)
            } else {
                same_value_zero(&left_value, &right_value)
            };
            if !equal {
                return Ok(false);
            }
        }
        return if strict {
            same_enumerable_symbols_shallow(left, right)
        } else {
            Ok(true)
        };
    }
    let Some((left_buffer, left_start, left_end)) = typed_array_bytes(left) else {
        return Ok(false);
    };
    let Some((right_buffer, right_start, right_end)) = typed_array_bytes(right) else {
        return Ok(false);
    };
    let left_bytes = left_buffer.bytes.borrow();
    let right_bytes = right_buffer.bytes.borrow();
    if left_bytes.get(left_start..left_end) != right_bytes.get(right_start..right_end) {
        return Ok(false);
    }
    if strict {
        same_enumerable_symbols_shallow(left, right)
    } else {
        Ok(true)
    }
}

fn same_value_zero(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) if *left == 0.0 && *right == 0.0 => true,
        (Value::Number(left), Value::Number(right)) if left.is_nan() && right.is_nan() => true,
        _ => execute::same_value(left, right),
    }
}

fn is_float_typed_array(value: &Value) -> bool {
    matches!(value, Value::Float32Array(_) | Value::Float64Array(_)) || value.is_float16_array()
}

fn value_is_float16(value: &Value) -> bool {
    value.is_float16_array()
}

fn same_enumerable_symbols_shallow(left: &Value, right: &Value) -> Result<bool, VmError> {
    let left_symbols = execute::own_enumerable_symbol_strings(left);
    let right_symbols = execute::own_enumerable_symbol_strings(right);
    if left_symbols.len() != right_symbols.len()
        || left_symbols.iter().any(|key| !right_symbols.contains(key))
    {
        return Ok(false);
    }
    for key in &left_symbols {
        if !execute::same_value(
            &execute::get_property(left, key),
            &execute::get_property(right, key),
        ) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn partial_enumerable_symbols(left: &Value, right: &Value) -> Result<bool, VmError> {
    let left_symbols = execute::own_enumerable_symbol_strings(left);
    for key in execute::own_enumerable_symbol_strings(right) {
        if !left_symbols.contains(&key)
            || !execute::same_value(
                &execute::get_property(left, &key),
                &execute::get_property(right, &key),
            )
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn typed_array_bytes(
    value: &Value,
) -> Option<(&quench_runtime::value::ArrayBufferData, usize, usize)> {
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
    (end <= buffer.bytes.borrow().len()).then_some((buffer.as_ref(), offset, end))
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
    if strict {
        same_enumerable_symbols_shallow(left, right)
    } else {
        Ok(true)
    }
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

fn unresolved_alias(value: &Value) -> bool {
    matches!(
        value,
        Value::ObjectAlias(alias) if alias.0.borrow().upgrade().is_none()
    )
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
    let extra_keys = |value: &Value, length: usize| {
        execute::own_enumerable_keys(value)
            .into_iter()
            .filter(|key| key.parse::<usize>().map_or(true, |index| index >= length))
            .collect::<Vec<_>>()
    };
    let left_extra = extra_keys(left, a.logical_len())
        .into_iter()
        .filter(|key| strict || !key.starts_with("Symbol."))
        .collect::<Vec<_>>();
    let right_extra = extra_keys(right, b.logical_len())
        .into_iter()
        .filter(|key| strict || !key.starts_with("Symbol."))
        .collect::<Vec<_>>();
    if left_extra.len() != right_extra.len()
        || left_extra.iter().any(|key| !right_extra.contains(key))
    {
        return Ok(false);
    }
    for key in left_extra {
        let la = execute::get_property_result(left, &key)?;
        let rb = execute::get_property_result(right, &key)?;
        if !compare(&la, &rb, strict, skip_prototype, memo)? {
            return Ok(false);
        }
    }
    if strict {
        same_enumerable_symbols(left, right, strict, skip_prototype, memo)
    } else {
        Ok(true)
    }
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
    if !strict {
        let left_boxed = execute::get_property(left, "_value");
        let right_boxed = execute::get_property(right, "_value");
        if !matches!(left_boxed, Value::Undefined)
            && !compare(&left_boxed, &right_boxed, false, skip_prototype, memo)?
        {
            return Ok(false);
        }
    }
    if strict && !same_shape(left, right, strict, skip_prototype, memo)? {
        return Ok(false);
    }
    if is_error_value(left) || is_error_value(right) {
        if !same_property(left, right, "name") || !same_property(left, right, "message") {
            return Ok(false);
        }
        let left_aggregate = is_aggregate_error(left);
        let right_aggregate = is_aggregate_error(right);
        if left_aggregate != right_aggregate {
            return Ok(false);
        }
        if left_aggregate {
            let left_errors = execute::get_property_result(left, "errors")?;
            let right_errors = execute::get_property_result(right, "errors")?;
            if !compare(&left_errors, &right_errors, strict, skip_prototype, memo)? {
                return Ok(false);
            }
        }
    }
    let left_cause = execute::get_property_result(left, "cause")?;
    let right_cause = execute::get_property_result(right, "cause")?;
    let left_has_cause = has_own_slot(left, "cause");
    let right_has_cause = has_own_slot(right, "cause");
    if left_has_cause != right_has_cause {
        return Ok(false);
    }
    let cause_present = left_has_cause
        || right_has_cause
        || !matches!(left_cause, Value::Undefined)
        || !matches!(right_cause, Value::Undefined);
    if cause_present {
        if !compare(&left_cause, &right_cause, strict, skip_prototype, memo)? {
            return Ok(false);
        }
    }
    let left_keys = execute::own_enumerable_keys(left)
        .into_iter()
        .filter(|key| strict || !key.starts_with("Symbol."))
        .collect::<Vec<_>>();
    let right_keys = execute::own_enumerable_keys(right)
        .into_iter()
        .filter(|key| strict || !key.starts_with("Symbol."))
        .collect::<Vec<_>>();
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

fn has_own_slot(value: &Value, key: &str) -> bool {
    if let Value::Object(properties) = value {
        if properties.iter().any(|(name, _)| name == key) {
            return true;
        }
    }
    if execute::has_own_property(value, key) {
        return true;
    }
    matches!(
        execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor),
            &Value::Undefined,
            &[value.clone(), Value::String(key.to_string())],
        ),
        Ok(Value::Object(_))
    )
}

fn is_aggregate_error(value: &Value) -> bool {
    let mut current = execute::get_prototype_of(value).ok();
    while let Some(prototype) = current {
        if matches!(
            prototype,
            Value::Builtin(quench_runtime::ops::Builtin::AggregateErrorPrototype)
        ) {
            return true;
        }
        current = execute::get_prototype_of(&prototype).ok();
    }
    false
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
