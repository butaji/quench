//! Node's strict deep equality (lib/internal/util/comparisons.js, `isDeepStrictEqual`).
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, BuiltinData, NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

/// How `keyCheck` walks a container beyond its own keys (`kNoIterator`, `kIsArray`, ...).
#[derive(Clone, Copy, PartialEq)]
enum Iteration {
    None,
    Array,
    Set,
    Map,
}

/// The pairs being compared, which a cycle compares equal (`memos`).
struct Comparison {
    in_progress: Vec<(RootId, RootId)>,
}

fn type_of(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

fn is_object(c: &Context<'_>, value: RootId) -> bool {
    matches!(type_of(c, value), ValueType::Object | ValueType::Function)
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn object_tag(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let primordials = super::util_shared_vm::primordials(c)?;
    let to_string = get(c, primordials, "objectToString")?;
    let tag = c.call_rooted(to_string, value, &[])?;
    c.to_string(tag)
}

/// SameValueZero, which `Set.prototype.has` and `Map.prototype.get` use.
fn same_value_zero(c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
    if c.strict_equal_rooted(left, right)? {
        return Ok(true);
    }
    let nan = |value: RootId| c.rooted_value(value).and_then(|value| value.as_number()).is_some_and(f64::is_nan);
    Ok(nan(left) && nan(right))
}

fn is_error(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    super::util_inspect_shared_vm::is_error(c, value)
}

/// `util.isDeepStrictEqual(val1, val2)`.
pub(crate) fn is_deep_strict_equal(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let left = args.first().copied().unwrap_or(undefined);
    let right = args.get(1).copied().unwrap_or(undefined);
    let equal = Comparison { in_progress: Vec::new() }.equal(c, left, right)?;
    Ok(c.boolean(equal))
}

impl Comparison {
    /// `innerDeepEqual(val1, val2, kStrict)`.
    fn equal(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        if c.strict_equal_rooted(left, right)? {
            return c.same_value_rooted(left, right);
        }
        if type_of(c, left) != ValueType::Object {
            let nan = |value: RootId| c.rooted_value(value).and_then(|value| value.as_number()).is_some_and(f64::is_nan);
            return Ok(nan(left) && nan(right));
        }
        if type_of(c, right) != ValueType::Object {
            return Ok(false);
        }
        let left_prototype = c.prototype_rooted(left)?;
        let right_prototype = c.prototype_rooted(right)?;
        if !c.strict_equal_rooted(left_prototype, right_prototype)? {
            return Ok(false);
        }
        if object_tag(c, left)? != object_tag(c, right)? {
            return Ok(false);
        }
        let brand = |c: &mut Context<'_>, value: RootId, brand: BuiltinBrand| c.has_builtin_brand_rooted(value, brand);
        if c.is_array_rooted(left)? {
            if !c.is_array_rooted(right)? || array_length(c, left)? != array_length(c, right)? {
                return Ok(false);
            }
            let left_keys = c.own_non_index_keys_rooted(left, true)?;
            let right_keys = c.own_non_index_keys_rooted(right, true)?;
            if left_keys.len() != right_keys.len() {
                return Ok(false);
            }
            return self.key_check(c, left, right, Iteration::Array, Some(left_keys));
        }
        if object_tag(c, left)? == "[object Object]" && !is_error(c, left)? && !brand(c, left, BuiltinBrand::BoxedPrimitive)? {
            return self.key_check(c, left, right, Iteration::None, None);
        }
        if brand(c, left, BuiltinBrand::Date)? {
            let (Some(BuiltinData::Primitive { value: left_time, .. }), true) = (c.builtin_data_rooted(left)?, brand(c, right, BuiltinBrand::Date)?) else {
                return Ok(false);
            };
            let Some(BuiltinData::Primitive { value: right_time, .. }) = c.builtin_data_rooted(right)? else { return Ok(false) };
            if !c.same_value_rooted(left_time, right_time)? {
                return Ok(false);
            }
        } else if brand(c, left, BuiltinBrand::RegExp)? {
            if !brand(c, right, BuiltinBrand::RegExp)? {
                return Ok(false);
            }
            for name in ["source", "flags", "lastIndex"] {
                let left_value = get(c, left, name)?;
                let right_value = get(c, right, name)?;
                if !c.strict_equal_rooted(left_value, right_value)? {
                    return Ok(false);
                }
            }
        } else if is_error(c, left)? {
            if !is_error(c, right)? {
                return Ok(false);
            }
            for name in ["message", "name"] {
                let left_value = get(c, left, name)?;
                let right_value = get(c, right, name)?;
                if !c.strict_equal_rooted(left_value, right_value)? {
                    return Ok(false);
                }
            }
        } else if brand(c, left, BuiltinBrand::ArrayBufferView)? {
            let left_kind = c.buffer_storage_rooted(left)?.map(|storage| storage.kind);
            let right_kind = c.buffer_storage_rooted(right)?.map(|storage| storage.kind);
            if left_kind != right_kind || c.view_bytes_rooted(left) != c.view_bytes_rooted(right) {
                return Ok(false);
            }
            let left_keys = c.own_non_index_keys_rooted(left, true)?;
            let right_keys = c.own_non_index_keys_rooted(right, true)?;
            if left_keys.len() != right_keys.len() {
                return Ok(false);
            }
            return self.key_check(c, left, right, Iteration::None, Some(left_keys));
        } else if brand(c, left, BuiltinBrand::Set)? {
            if !brand(c, right, BuiltinBrand::Set)? || entries(c, left)?.len() != entries(c, right)?.len() {
                return Ok(false);
            }
            return self.key_check(c, left, right, Iteration::Set, None);
        } else if brand(c, left, BuiltinBrand::Map)? {
            if !brand(c, right, BuiltinBrand::Map)? || entries(c, left)?.len() != entries(c, right)?.len() {
                return Ok(false);
            }
            return self.key_check(c, left, right, Iteration::Map, None);
        } else if brand(c, left, BuiltinBrand::AnyArrayBuffer)? {
            if !brand(c, right, BuiltinBrand::AnyArrayBuffer)? || bytes(c, left)? != bytes(c, right)? {
                return Ok(false);
            }
        } else if brand(c, left, BuiltinBrand::BoxedPrimitive)? {
            let (Some(BuiltinData::Primitive { brand: left_brand, value: left_value }), Some(BuiltinData::Primitive { brand: right_brand, value: right_value })) =
                (c.builtin_data_rooted(left)?, c.builtin_data_rooted(right)?)
            else {
                return Ok(false);
            };
            if left_brand != right_brand || !c.same_value_rooted(left_value, right_value)? {
                return Ok(false);
            }
        } else {
            for kind in [
                BuiltinBrand::ArrayBufferView,
                BuiltinBrand::Set,
                BuiltinBrand::Map,
                BuiltinBrand::Date,
                BuiltinBrand::RegExp,
                BuiltinBrand::AnyArrayBuffer,
                BuiltinBrand::BoxedPrimitive,
            ] {
                if brand(c, right, kind)? {
                    return Ok(false);
                }
            }
            if c.is_array_rooted(right)? || is_error(c, right)? {
                return Ok(false);
            }
        }
        self.key_check(c, left, right, Iteration::None, None)
    }

    /// `keyCheck`: the same enumerable own keys and symbols, then `objEquiv`.
    fn key_check(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, iteration: Iteration, keys: Option<Vec<RootId>>) -> Result<bool, RootedError> {
        let listed = keys.is_some();
        let mut keys = match keys {
            Some(keys) => keys,
            None => {
                let left_keys = c.enumerable_own_keys_rooted(left, false, true)?;
                let right_keys = c.enumerable_own_keys_rooted(right, false, true)?;
                if left_keys.len() != right_keys.len() {
                    return Ok(false);
                }
                left_keys
            }
        };
        for &key in &keys {
            if !enumerable_own(c, right, key)? {
                return Ok(false);
            }
        }
        if !listed {
            let mut count = 0;
            for key in own_symbols(c, left)? {
                if enumerable_own(c, left, key)? {
                    if !enumerable_own(c, right, key)? {
                        return Ok(false);
                    }
                    keys.push(key);
                    count += 1;
                } else if enumerable_own(c, right, key)? {
                    return Ok(false);
                }
            }
            let mut right_count = 0;
            for key in own_symbols(c, right)? {
                if enumerable_own(c, right, key)? {
                    right_count += 1;
                }
            }
            if right_count != count {
                return Ok(false);
            }
        }
        let empty = match iteration {
            Iteration::None => true,
            Iteration::Array => array_length(c, left)? == 0,
            Iteration::Set | Iteration::Map => entries(c, left)?.is_empty(),
        };
        if keys.is_empty() && empty {
            return Ok(true);
        }
        for &(seen_left, seen_right) in &self.in_progress {
            if c.strict_equal_rooted(seen_left, left)? && c.strict_equal_rooted(seen_right, right)? {
                return Ok(true);
            }
        }
        self.in_progress.push((left, right));
        let equal = self.object_equivalent(c, left, right, &keys, iteration);
        self.in_progress.pop();
        equal
    }

    /// `objEquiv`.
    fn object_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, keys: &[RootId], iteration: Iteration) -> Result<bool, RootedError> {
        match iteration {
            Iteration::Set => {
                if !self.set_equivalent(c, left, right)? {
                    return Ok(false);
                }
            }
            Iteration::Map => {
                if !self.map_equivalent(c, left, right)? {
                    return Ok(false);
                }
            }
            Iteration::Array => {
                for index in 0..array_length(c, left)? {
                    let key = c.string_rooted(&index.to_string());
                    let left_own = c.own_property_fields_rooted(left, key)?.is_some();
                    let right_own = c.own_property_fields_rooted(right, key)?.is_some();
                    if left_own != right_own {
                        return Ok(false);
                    }
                    if left_own {
                        let left_value = c.get_property_rooted(left, key)?;
                        let right_value = c.get_property_rooted(right, key)?;
                        if !self.equal(c, left_value, right_value)? {
                            return Ok(false);
                        }
                    }
                }
            }
            Iteration::None => {}
        }
        for &key in keys {
            let left_value = c.get_property_rooted(left, key)?;
            let right_value = c.get_property_rooted(right, key)?;
            if !self.equal(c, left_value, right_value)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `setEquiv`: primitives by SameValueZero, objects by deep equality, each matched once.
    fn set_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        let left_entries = entries(c, left)?;
        let right_entries = entries(c, right)?;
        let mut objects = Vec::new();
        for (value, _) in &left_entries {
            if is_object(c, *value) {
                objects.push(*value);
            } else if !contains(c, &right_entries, *value)? {
                return Ok(false);
            }
        }
        if objects.is_empty() {
            return Ok(true);
        }
        for (value, _) in &right_entries {
            if !is_object(c, *value) {
                continue;
            }
            let mut found = None;
            for (index, candidate) in objects.iter().enumerate() {
                if self.equal(c, *value, *candidate)? {
                    found = Some(index);
                    break;
                }
            }
            match found {
                Some(index) => {
                    objects.remove(index);
                }
                None => return Ok(false),
            }
        }
        Ok(objects.is_empty())
    }

    /// `mapEquiv`.
    fn map_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        let left_entries = entries(c, left)?;
        let right_entries = entries(c, right)?;
        let mut objects = Vec::new();
        for &(key, item) in &left_entries {
            if is_object(c, key) {
                objects.push((key, item));
                continue;
            }
            let mut other = None;
            for &(candidate, value) in &right_entries {
                if same_value_zero(c, key, candidate)? {
                    other = Some(value);
                    break;
                }
            }
            match other {
                Some(other) if self.equal(c, item, other)? => {}
                _ => return Ok(false),
            }
        }
        if objects.is_empty() {
            return Ok(true);
        }
        for &(key, item) in &right_entries {
            if !is_object(c, key) {
                continue;
            }
            let mut found = None;
            for (index, &(candidate, value)) in objects.iter().enumerate() {
                if self.equal(c, key, candidate)? && self.equal(c, item, value)? {
                    found = Some(index);
                    break;
                }
            }
            match found {
                Some(index) => {
                    objects.remove(index);
                }
                None => return Ok(false),
            }
        }
        Ok(objects.is_empty())
    }
}

fn contains(c: &mut Context<'_>, entries: &[(RootId, RootId)], value: RootId) -> Result<bool, RootedError> {
    for (candidate, _) in entries {
        if same_value_zero(c, value, *candidate)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A Map's entries, or a Set's values each paired with itself.
fn entries(c: &mut Context<'_>, value: RootId) -> Result<Vec<(RootId, RootId)>, RootedError> {
    Ok(match c.builtin_data_rooted(value)? {
        Some(BuiltinData::Map(entries)) => entries,
        Some(BuiltinData::Set(values)) => values.into_iter().map(|value| (value, value)).collect(),
        _ => Vec::new(),
    })
}

fn bytes(c: &mut Context<'_>, value: RootId) -> Result<Vec<u8>, RootedError> {
    Ok(match c.builtin_data_rooted(value)? {
        Some(BuiltinData::Bytes { bytes, .. }) => bytes,
        _ => Vec::new(),
    })
}

fn array_length(c: &mut Context<'_>, value: RootId) -> Result<usize, RootedError> {
    let length = get(c, value, "length")?;
    Ok(c.to_number(length)? as usize)
}

fn enumerable_own(c: &mut Context<'_>, object: RootId, key: RootId) -> Result<bool, RootedError> {
    Ok(c.own_property_fields_rooted(object, key)?.is_some_and(|fields| fields.enumerable == Some(true)))
}

fn own_symbols(c: &mut Context<'_>, object: RootId) -> Result<Vec<RootId>, RootedError> {
    let mut symbols = Vec::new();
    for key in c.own_keys_rooted(object)? {
        if c.is_symbol_rooted(key)? {
            symbols.push(key);
        }
    }
    Ok(symbols)
}
