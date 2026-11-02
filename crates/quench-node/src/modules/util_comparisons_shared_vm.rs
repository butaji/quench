//! Node's deep equality (lib/internal/util/comparisons.js: `isDeepEqual`,
//! `isDeepStrictEqual` and `isPartialStrictEqual`) and its line diff (`util.diff`,
//! internal/assert/myers_diff.js).
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, BuiltinData, NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

/// `kLoose`, `kStrict` and `kPartial`: primitives by `==` without prototypes, by
/// SameValue with constructors, or strictly with `actual` allowed to hold more.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Loose,
    Strict,
    Partial,
}

/// How `keyCheck` walks a container beyond its own keys (`kNoIterator`, `kIsArray`, ...).
#[derive(Clone, Copy, PartialEq)]
enum Iteration {
    None,
    Array,
    Set,
    Map,
}

/// The constructors whose identity strict mode compares in place of prototypes.
const WELL_KNOWN_CONSTRUCTORS: [&str; 30] = [
    "Array", "ArrayBuffer", "BigInt", "BigInt64Array", "BigUint64Array", "Boolean", "DataView", "Date", "Error", "Float16Array",
    "Float32Array", "Float64Array", "Function", "Int16Array", "Int32Array", "Int8Array", "Map", "Number", "Object", "Promise",
    "RegExp", "Set", "String", "Symbol", "Uint16Array", "Uint32Array", "Uint8Array", "Uint8ClampedArray", "WeakMap", "WeakSet",
];
const WELL_KNOWN_KEY: &str = "\0builtin:comparisons/wellKnownConstructors";

/// Whether `left` and `right` are deeply equal in `mode`.
pub(crate) fn deep_equal(c: &mut Context<'_>, left: RootId, right: RootId, mode: Mode) -> Result<bool, RootedError> {
    Comparison { mode, in_progress: Vec::new() }.equal(c, left, right)
}

/// `util.isDeepStrictEqual(val1, val2)`.
pub(crate) fn is_deep_strict_equal(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let left = args.first().copied().unwrap_or(undefined);
    let right = args.get(1).copied().unwrap_or(undefined);
    let equal = deep_equal(c, left, right, Mode::Strict)?;
    Ok(c.boolean(equal))
}

/// One comparison: its mode and the objects being compared, which a cycle compares equal
/// (`memos`).
struct Comparison {
    mode: Mode,
    in_progress: Vec<RootId>,
}

fn type_of(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

/// `typeof value === 'object' && value !== null`.
fn is_object(c: &Context<'_>, value: RootId) -> bool {
    type_of(c, value) == ValueType::Object
}

fn is_nan(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).and_then(|value| value.as_number()).is_some_and(f64::is_nan)
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    type_of(c, value) == ValueType::Undefined
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn brand(c: &mut Context<'_>, value: RootId, brand: BuiltinBrand) -> Result<bool, RootedError> {
    c.has_builtin_brand_rooted(value, brand)
}

fn object_tag(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let tag = call_primordial(c, "objectToString", value, &[])?;
    c.to_string(tag)
}

fn call_primordial(c: &mut Context<'_>, name: &str, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = super::util_inspect_shared_vm::primordial(c, name)?;
    c.call_rooted(function, this, args)
}

fn collection_has(c: &mut Context<'_>, collection: RootId, value: RootId, method: &str) -> Result<bool, RootedError> {
    let found = call_primordial(c, method, collection, &[value])?;
    c.truthy_rooted(found)
}

fn has_own(c: &mut Context<'_>, object: RootId, key: RootId) -> Result<bool, RootedError> {
    Ok(c.own_property_fields_rooted(object, key)?.is_some())
}

fn has_enumerable(c: &mut Context<'_>, object: RootId, key: RootId) -> Result<bool, RootedError> {
    Ok(c.own_property_fields_rooted(object, key)?.is_some_and(|fields| fields.enumerable == Some(true)))
}

fn index_key(c: &mut Context<'_>, index: usize) -> RootId {
    c.string_rooted(&index.to_string())
}

/// `ObjectKeys`.
fn object_keys(c: &mut Context<'_>, object: RootId) -> Result<Vec<RootId>, RootedError> {
    c.enumerable_own_keys_rooted(object, false, true)
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

fn length(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    let length = get(c, value, name)?;
    c.to_number(length)
}

/// A Map's entries, or a Set's values each paired with itself, in insertion order.
fn entries(c: &mut Context<'_>, value: RootId) -> Result<Vec<(RootId, RootId)>, RootedError> {
    Ok(match c.builtin_data_rooted(value)? {
        Some(BuiltinData::Map(entries)) => entries,
        Some(BuiltinData::Set(values)) => values.into_iter().map(|value| (value, value)).collect(),
        _ => Vec::new(),
    })
}

/// The specific brand of an ArrayBuffer or view (`TypedArrayPrototypeGetSymbolToStringTag`)
/// and the bytes it covers.
fn binary(c: &mut Context<'_>, value: RootId) -> Result<(Option<BuiltinBrand>, Vec<u8>), RootedError> {
    Ok(match c.builtin_data_rooted(value)? {
        Some(BuiltinData::Bytes { brand, bytes }) => (Some(brand), bytes),
        _ => (None, Vec::new()),
    })
}

/// `isPartialUint8Array`: `expected`'s bytes occur in `actual` in order.
fn is_partial_bytes(actual: &[u8], expected: &[u8]) -> bool {
    let mut remaining = actual.iter();
    expected.iter().all(|byte| remaining.any(|candidate| candidate == byte))
}

/// `areSimilarFloatArrays`: loose element equality over the first byte-length indices.
fn similar_float_arrays(c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
    let byte_length = length(c, left, "byteLength")?;
    if byte_length != length(c, right, "byteLength")? {
        return Ok(false);
    }
    for index in 0..byte_length as usize {
        let key = index_key(c, index);
        let left_value = c.get_property_rooted(left, key)?;
        let right_value = c.get_property_rooted(right, key)?;
        if !c.strict_equal_rooted(left_value, right_value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn is_error(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    super::util_inspect_shared_vm::is_error(c, value)
}

/// `isURL`: a WHATWG URL by its own shape, not a legacy `url.parse` result.
fn is_url(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    let href = get(c, value, "href")?;
    let protocol = get(c, value, "protocol")?;
    if !c.truthy_rooted(href)? || !c.truthy_rooted(protocol)? {
        return Ok(false);
    }
    let auth = get(c, value, "auth")?;
    let path = get(c, value, "path")?;
    Ok(is_undefined(c, auth) && is_undefined(c, path))
}

/// `isBoxedPrimitive` values of the same class and equal contents.
fn equal_boxed(c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
    let (Some(BuiltinData::Primitive { brand: left_brand, value: left_value }), Some(BuiltinData::Primitive { brand: right_brand, value: right_value })) =
        (c.builtin_data_rooted(left)?, c.builtin_data_rooted(right)?)
    else {
        return Ok(false);
    };
    Ok(left_brand == right_brand && c.same_value_rooted(left_value, right_value)?)
}

/// The well-known constructors, captured once from the startup realm.
fn well_known_constructors(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(list) = c.host_mut().shared_state().borrow().builtin_cache.get(WELL_KNOWN_KEY).copied() {
        return Ok(list);
    }
    let mut constructors = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        WELL_KNOWN_CONSTRUCTORS.iter().map(|name| get(c, global, name)).collect::<Result<Vec<_>, _>>()
    })?;
    let buffer = crate::host::shared_vm::commonjs::builtin_require(c, "buffer")?;
    constructors.push(get(c, buffer, "Buffer")?);
    let list = c.array_rooted(&constructors)?;
    let list = c.retain(list)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(WELL_KNOWN_KEY.into(), list);
    Ok(list)
}

fn is_well_known_constructor(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if type_of(c, value) != ValueType::Function {
        return Ok(false);
    }
    let list = well_known_constructors(c)?;
    for constructor in c.array_like_values_rooted(list)? {
        if c.strict_equal_rooted(constructor, value)? {
            return Ok(true);
        }
    }
    Ok(false)
}

impl Comparison {
    /// `innerDeepEqual`.
    fn equal(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        if c.strict_equal_rooted(left, right)? {
            return Ok(self.mode == Mode::Loose || c.same_value_rooted(left, right)?);
        }
        if self.mode != Mode::Loose {
            if type_of(c, left) == ValueType::Number {
                return Ok(is_nan(c, left) && is_nan(c, right));
            }
            if !is_object(c, left) || !is_object(c, right) {
                return Ok(false);
            }
        } else {
            if !is_object(c, left) {
                return Ok(!is_object(c, right) && (c.equal_rooted(left, right)? || (is_nan(c, left) && is_nan(c, right))));
            }
            if !is_object(c, right) {
                return Ok(false);
            }
        }
        self.object_comparison_start(c, left, right)
    }

    /// `isEnumerableOrIdentical`.
    fn enumerable_or_identical(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, name: &str) -> Result<bool, RootedError> {
        let key = c.string_rooted(name);
        if has_enumerable(c, right, key)? {
            return Ok(true);
        }
        let right_value = c.get_property_rooted(right, key)?;
        if self.mode == Mode::Partial {
            let empty_message = name == "message" && c.string_code_units(right_value)?.is_some_and(|units| units.is_empty());
            if is_undefined(c, right_value) || empty_message {
                return Ok(true);
            }
        }
        let left_value = c.get_property_rooted(left, key)?;
        self.equal(c, left_value, right_value)
    }

    /// `getOwnNonIndexProperties(val2, filter)` and its count check against `left`.
    fn non_index_keys(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<Option<Vec<RootId>>, RootedError> {
        let keys = |c: &mut Context<'_>, value: RootId, mode: Mode| -> Result<Vec<RootId>, RootedError> {
            let mut keys = c.own_non_index_keys_rooted(value, true)?;
            if mode == Mode::Loose {
                let mut strings = Vec::with_capacity(keys.len());
                for key in keys {
                    if !c.is_symbol_rooted(key)? {
                        strings.push(key);
                    }
                }
                keys = strings;
            }
            Ok(keys)
        };
        let right_keys = keys(c, right, self.mode)?;
        if self.mode != Mode::Partial && right_keys.len() != keys(c, left, self.mode)?.len() {
            return Ok(None);
        }
        Ok(Some(right_keys))
    }

    /// `objectComparisonStart`.
    fn object_comparison_start(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        if self.mode == Mode::Strict {
            let constructor = get(c, left, "constructor")?;
            let constructor_key = c.string_rooted("constructor");
            if is_well_known_constructor(c, constructor)? || (!is_undefined(c, constructor) && !has_own(c, left, constructor_key)?) {
                let other = get(c, right, "constructor")?;
                if !c.strict_equal_rooted(constructor, other)? {
                    return Ok(false);
                }
            } else {
                let left_prototype = c.prototype_rooted(left)?;
                let right_prototype = c.prototype_rooted(right)?;
                if !c.strict_equal_rooted(left_prototype, right_prototype)? {
                    return Ok(false);
                }
            }
        }
        let tag = object_tag(c, left)?;
        if tag != object_tag(c, right)? {
            return Ok(false);
        }
        let partial = self.mode == Mode::Partial;
        // A size mismatch fails, except a larger `actual` in partial mode.
        let sizes_fit = |left: f64, right: f64| left == right || (partial && left > right);
        if c.is_array_rooted(left)? {
            if !c.is_array_rooted(right)? || !sizes_fit(length(c, left, "length")?, length(c, right, "length")?) {
                return Ok(false);
            }
            let Some(keys) = self.non_index_keys(c, left, right)? else { return Ok(false) };
            return self.key_check(c, left, right, Iteration::Array, Some(keys));
        }
        if tag == "[object Object]" {
            return self.key_check(c, left, right, Iteration::None, None);
        }
        if brand(c, left, BuiltinBrand::Date)? {
            if !brand(c, right, BuiltinBrand::Date)? {
                return Ok(false);
            }
            let (Some(BuiltinData::Primitive { value: left_time, .. }), Some(BuiltinData::Primitive { value: right_time, .. })) =
                (c.builtin_data_rooted(left)?, c.builtin_data_rooted(right)?)
            else {
                return Ok(false);
            };
            if !c.strict_equal_rooted(left_time, right_time)? {
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
        } else if brand(c, left, BuiltinBrand::ArrayBufferView)? {
            let (left_kind, left_bytes) = binary(c, left)?;
            let (right_kind, right_bytes) = binary(c, right)?;
            if left_kind != right_kind {
                return Ok(false);
            }
            let float = [BuiltinBrand::Float16Array, BuiltinBrand::Float32Array, BuiltinBrand::Float64Array];
            let similar = if partial && left_bytes.len() != right_bytes.len() {
                is_partial_bytes(&left_bytes, &right_bytes)
            } else if self.mode == Mode::Loose && left_kind.is_some_and(|kind| float.contains(&kind)) {
                similar_float_arrays(c, left, right)?
            } else {
                left_bytes == right_bytes
            };
            if !similar {
                return Ok(false);
            }
            let Some(keys) = self.non_index_keys(c, left, right)? else { return Ok(false) };
            return self.key_check(c, left, right, Iteration::None, Some(keys));
        } else if brand(c, left, BuiltinBrand::Set)? || brand(c, left, BuiltinBrand::Map)? {
            let (iteration, kind) = if brand(c, left, BuiltinBrand::Set)? { (Iteration::Set, BuiltinBrand::Set) } else { (Iteration::Map, BuiltinBrand::Map) };
            if !brand(c, right, kind)? || !sizes_fit(entries(c, left)?.len() as f64, entries(c, right)?.len() as f64) {
                return Ok(false);
            }
            return self.key_check(c, left, right, iteration, None);
        } else if brand(c, left, BuiltinBrand::AnyArrayBuffer)? {
            if !brand(c, right, BuiltinBrand::AnyArrayBuffer)? {
                return Ok(false);
            }
            let (_, left_bytes) = binary(c, left)?;
            let (_, right_bytes) = binary(c, right)?;
            let similar = if partial && left_bytes.len() != right_bytes.len() { is_partial_bytes(&left_bytes, &right_bytes) } else { left_bytes == right_bytes };
            if !similar {
                return Ok(false);
            }
        } else if is_error(c, left)? {
            if !is_error(c, right)? {
                return Ok(false);
            }
            for name in ["message", "name", "cause", "errors"] {
                if !self.enumerable_or_identical(c, left, right, name)? {
                    return Ok(false);
                }
            }
            let cause = c.string_rooted("cause");
            let right_cause = has_own(c, right, cause)?;
            if right_cause != has_own(c, left, cause)? && (!partial || right_cause) {
                return Ok(false);
            }
        } else if brand(c, left, BuiltinBrand::BoxedPrimitive)? {
            if !equal_boxed(c, left, right)? {
                return Ok(false);
            }
        } else if self.distinct_kind(c, right)? {
            return Ok(false);
        } else if is_url(c, left)? {
            if !is_url(c, right)? {
                return Ok(false);
            }
            let left_href = get(c, left, "href")?;
            let right_href = get(c, right, "href")?;
            if !c.strict_equal_rooted(left_href, right_href)? {
                return Ok(false);
            }
        } else if super::util_module_shared_vm::is_key_class(c, left, "KeyObject")? {
            if !super::util_module_shared_vm::is_key_class(c, right, "KeyObject")? {
                return Ok(false);
            }
            let equals = get(c, left, "equals")?;
            let same = c.call_rooted(equals, left, &[right])?;
            if !c.truthy_rooted(same)? {
                return Ok(false);
            }
        } else if super::util_module_shared_vm::is_key_class(c, left, "CryptoKey")? {
            if !super::util_module_shared_vm::is_key_class(c, right, "CryptoKey")? {
                return Ok(false);
            }
            let left_extractable = get(c, left, "extractable")?;
            let right_extractable = get(c, right, "extractable")?;
            if !c.strict_equal_rooted(left_extractable, right_extractable)? {
                return Ok(false);
            }
            for name in ["algorithm", "usages"] {
                let left_value = get(c, left, name)?;
                let right_value = get(c, right, name)?;
                if !self.equal(c, left_value, right_value)? {
                    return Ok(false);
                }
            }
            let crypto = crate::host::shared_vm::commonjs::builtin_require(c, "crypto")?;
            let key_object = get(c, crypto, "KeyObject")?;
            let from = get(c, key_object, "from")?;
            let left_key = c.call_rooted(from, key_object, &[left])?;
            let right_key = c.call_rooted(from, key_object, &[right])?;
            if !self.equal(c, left_key, right_key)? {
                return Ok(false);
            }
        } else if brand(c, left, BuiltinBrand::WeakMap)? || brand(c, left, BuiltinBrand::WeakSet)? {
            return Ok(false);
        }
        self.key_check(c, left, right, Iteration::None, None)
    }

    /// `right` is one of the kinds `left`, an ordinary object, is not.
    fn distinct_kind(&mut self, c: &mut Context<'_>, right: RootId) -> Result<bool, RootedError> {
        if c.is_array_rooted(right)? {
            return Ok(true);
        }
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
                return Ok(true);
            }
        }
        is_error(c, right)
    }

    /// `keyCheck`: the same enumerable own keys and symbols, then `objEquiv`.
    fn key_check(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, iteration: Iteration, keys: Option<Vec<RootId>>) -> Result<bool, RootedError> {
        let array_like = keys.is_some();
        let mut keys = match keys {
            Some(keys) => keys,
            None => object_keys(c, right)?,
        };
        if !array_like {
            match self.mode {
                Mode::Partial => {
                    for key in own_symbols(c, right)? {
                        if has_enumerable(c, right, key)? {
                            if !has_enumerable(c, left, key)? {
                                return Ok(false);
                            }
                            keys.push(key);
                        }
                    }
                }
                Mode::Loose | Mode::Strict => {
                    if keys.len() != object_keys(c, left)?.len() {
                        return Ok(false);
                    }
                    if self.mode == Mode::Strict && !self.symbols_match(c, left, right, &mut keys)? {
                        return Ok(false);
                    }
                }
            }
        }
        if keys.is_empty() {
            let empty = match iteration {
                Iteration::None => true,
                Iteration::Array => length(c, right, "length")? == 0.0,
                Iteration::Set | Iteration::Map => entries(c, right)?.is_empty(),
            };
            if empty {
                return Ok(true);
            }
        }
        let mut seen = [false; 2];
        for &value in &self.in_progress {
            seen[0] |= c.strict_equal_rooted(value, left)?;
            seen[1] |= c.strict_equal_rooted(value, right)?;
        }
        if seen == [true; 2] {
            return Ok(true);
        }
        self.in_progress.extend([left, right]);
        let equal = self.object_equivalent(c, left, right, &keys, iteration);
        self.in_progress.truncate(self.in_progress.len() - 2);
        equal
    }

    /// Strict mode's enumerable own symbols: the same on both sides, appended to `keys`.
    fn symbols_match(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, keys: &mut Vec<RootId>) -> Result<bool, RootedError> {
        let mut count = 0;
        for key in own_symbols(c, left)? {
            if has_enumerable(c, left, key)? {
                if !has_enumerable(c, right, key)? {
                    return Ok(false);
                }
                keys.push(key);
                count += 1;
            } else if has_enumerable(c, right, key)? {
                return Ok(false);
            }
        }
        let mut right_count = 0;
        for key in own_symbols(c, right)? {
            if has_enumerable(c, right, key)? {
                right_count += 1;
            }
        }
        Ok(right_count == count)
    }

    /// `objEquiv`.
    fn object_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, keys: &[RootId], iteration: Iteration) -> Result<bool, RootedError> {
        for &key in keys {
            let Some(fields) = c.own_property_fields_rooted(left, key)? else { return Ok(false) };
            if fields.enumerable != Some(true) {
                return Ok(false);
            }
            let left_value = match fields.value {
                Some(value) if !is_undefined(c, value) => value,
                _ => c.get_property_rooted(left, key)?,
            };
            let right_value = c.get_property_rooted(right, key)?;
            if !self.equal(c, left_value, right_value)? {
                return Ok(false);
            }
        }
        match iteration {
            Iteration::Array if self.mode == Mode::Partial => self.partial_array_equivalent(c, left, right),
            Iteration::Array => self.array_equivalent(c, left, right),
            Iteration::Set => self.set_equivalent(c, left, right),
            Iteration::Map => self.map_equivalent(c, left, right),
            Iteration::None => Ok(true),
        }
    }

    /// The element `index` of `array`, or `None` for a hole.
    fn element(c: &mut Context<'_>, array: RootId, index: usize) -> Result<Option<RootId>, RootedError> {
        let key = index_key(c, index);
        if !has_own(c, array, key)? {
            return Ok(None);
        }
        c.get_property_rooted(array, key).map(Some)
    }

    /// `objEquiv`'s array walk, holes matching holes.
    fn array_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        for index in 0..length(c, left, "length")? as usize {
            match (Self::element(c, left, index)?, Self::element(c, right, index)?) {
                (Some(left_value), Some(right_value)) => {
                    if !self.equal(c, left_value, right_value)? {
                        return Ok(false);
                    }
                }
                (None, None) => return self.sparse_array_equivalent(c, left, right, index),
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    /// `sparseArrayEquiv`: the remaining keys pairwise, from position `start`.
    fn sparse_array_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, start: usize) -> Result<bool, RootedError> {
        let left_keys = object_keys(c, left)?;
        let right_keys = object_keys(c, right)?;
        if left_keys.len() != right_keys.len() {
            return Ok(false);
        }
        for &key in right_keys.iter().skip(start) {
            if !has_own(c, left, key)? {
                return Ok(false);
            }
            let left_value = c.get_property_rooted(left, key)?;
            let right_value = c.get_property_rooted(right, key)?;
            if !self.equal(c, left_value, right_value)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `partialArrayEquiv`: `expected`'s elements occur in `actual` in order.
    fn partial_array_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        let left_length = length(c, left, "length")? as usize;
        let right_length = length(c, right, "length")? as usize;
        let mut position = 0;
        for index in 0..right_length {
            let Some(right_value) = Self::element(c, right, index)? else {
                return self.partial_sparse_array_equivalent(c, left, right, position, index);
            };
            loop {
                let Some(left_value) = Self::element(c, left, position)? else {
                    return self.partial_sparse_array_equivalent(c, left, right, position, index);
                };
                if self.equal(c, left_value, right_value)? {
                    break;
                }
                position += 1;
                if position + right_length > left_length + index {
                    return Ok(false);
                }
            }
            position += 1;
        }
        Ok(true)
    }

    /// `partialSparseArrayEquiv`: the remaining keys' values occur in order.
    fn partial_sparse_array_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, left_start: usize, right_start: usize) -> Result<bool, RootedError> {
        let left_keys: Vec<RootId> = object_keys(c, left)?.into_iter().skip(left_start).collect();
        let right_keys: Vec<RootId> = object_keys(c, right)?.into_iter().skip(right_start).collect();
        if left_keys.len() < right_keys.len() {
            return Ok(false);
        }
        let mut position = 0;
        for (index, &right_key) in right_keys.iter().enumerate() {
            let right_value = c.get_property_rooted(right, right_key)?;
            loop {
                let left_value = match left_keys.get(position) {
                    Some(&key) => c.get_property_rooted(left, key)?,
                    None => c.undefined(),
                };
                if self.equal(c, left_value, right_value)? {
                    break;
                }
                position += 1;
                if position + right_keys.len() > left_keys.len() + index {
                    return Ok(false);
                }
            }
            position += 1;
        }
        Ok(true)
    }

    /// `setEquiv`: `right`'s values missing from `left` must match distinct deep-equal ones.
    fn set_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        let mut unmatched = Vec::new();
        for (value, _) in entries(c, right)? {
            if collection_has(c, left, value, "setHas")? {
                continue;
            }
            if !is_object(c, value) && (self.mode != Mode::Loose || !self.set_might_have_loose_primitive(c, left, right, value)?) {
                return Ok(false);
            }
            unmatched.push(value);
        }
        if unmatched.is_empty() {
            return Ok(true);
        }
        let left_entries = entries(c, left)?;
        let mut candidates = Vec::new();
        for (value, _) in left_entries {
            // In strict modes a primitive of `left` either is in `right` or is extra.
            if collection_has(c, right, value, "setHas")? || (self.mode != Mode::Loose && !is_object(c, value)) {
                continue;
            }
            candidates.push(value);
        }
        self.match_distinct(c, &unmatched, &candidates, |this, c, expected, actual| this.equal(c, actual, expected))
    }

    /// `mapEquiv`.
    fn map_equivalent(&mut self, c: &mut Context<'_>, left: RootId, right: RootId) -> Result<bool, RootedError> {
        let mut unmatched = Vec::new();
        for (key, item) in entries(c, right)? {
            if is_object(c, key) {
                unmatched.push((key, item));
                continue;
            }
            if collection_has(c, left, key, "mapHas")? {
                let left_item = call_primordial(c, "mapGet", left, &[key])?;
                if self.equal(c, left_item, item)? {
                    continue;
                }
            }
            if self.mode != Mode::Loose || !self.map_might_have_loose_primitive(c, left, right, key, item)? {
                return Ok(false);
            }
            unmatched.push((key, item));
        }
        if unmatched.is_empty() {
            return Ok(true);
        }
        let mut candidates = Vec::new();
        for (key, item) in entries(c, left)? {
            if !is_object(c, key) {
                if self.mode != Mode::Loose {
                    continue;
                }
                if collection_has(c, right, key, "mapHas")? {
                    let right_item = call_primordial(c, "mapGet", right, &[key])?;
                    if self.equal(c, item, right_item)? {
                        continue;
                    }
                }
            }
            candidates.push((key, item));
        }
        self.match_distinct(c, &unmatched, &candidates, |this, c, (expected_key, expected_item), (actual_key, actual_item)| {
            Ok(this.equal(c, actual_key, expected_key)? && this.equal(c, actual_item, expected_item)?)
        })
    }

    /// Each of `expected` matches a distinct one of `actual`; outside partial mode every one
    /// of `actual` must be used, so the counts must agree.
    fn match_distinct<T: Copy>(
        &mut self,
        c: &mut Context<'_>,
        expected: &[T],
        actual: &[T],
        mut matches: impl FnMut(&mut Self, &mut Context<'_>, T, T) -> Result<bool, RootedError>,
    ) -> Result<bool, RootedError> {
        if expected.len() > actual.len() || (self.mode != Mode::Partial && expected.len() != actual.len()) {
            return Ok(false);
        }
        let mut used = vec![false; actual.len()];
        'expected: for &wanted in expected {
            for (index, &candidate) in actual.iter().enumerate() {
                if !used[index] && matches(self, c, wanted, candidate)? {
                    used[index] = true;
                    continue 'expected;
                }
            }
            return Ok(false);
        }
        Ok(true)
    }

    /// `findLooseMatchingPrimitives`: `Some(answer)` when no loose match can exist (or one
    /// certainly might), `None` when `primitive`'s loose partner is `alternative`.
    fn loose_partner(c: &mut Context<'_>, primitive: RootId) -> Result<Result<RootId, bool>, RootedError> {
        Ok(match type_of(c, primitive) {
            ValueType::Undefined => Ok(c.null()),
            ValueType::Null => Ok(c.undefined()),
            ValueType::Symbol => Err(false),
            ValueType::String => Err(!c.to_number(primitive)?.is_nan()),
            ValueType::Number => Err(!is_nan(c, primitive)),
            _ => Err(true),
        })
    }

    /// `setMightHaveLoosePrim`.
    fn set_might_have_loose_primitive(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, primitive: RootId) -> Result<bool, RootedError> {
        match Self::loose_partner(c, primitive)? {
            Err(answer) => Ok(answer),
            Ok(partner) => Ok(!collection_has(c, right, partner, "setHas")? && collection_has(c, left, partner, "setHas")?),
        }
    }

    /// `mapMightHaveLoosePrim`.
    fn map_might_have_loose_primitive(&mut self, c: &mut Context<'_>, left: RootId, right: RootId, primitive: RootId, item: RootId) -> Result<bool, RootedError> {
        let partner = match Self::loose_partner(c, primitive)? {
            Err(answer) => return Ok(answer),
            Ok(partner) => partner,
        };
        if !collection_has(c, left, partner, "mapHas")? {
            return Ok(false);
        }
        let left_item = call_primordial(c, "mapGet", left, &[partner])?;
        Ok(self.equal(c, left_item, item)? && !collection_has(c, right, partner, "mapHas")?)
    }
}

/// `util.diff`'s operations: an item only in `actual`, in both, or only in `expected`.
const INSERTED: f64 = 1.0;
const UNCHANGED: f64 = 0.0;
const DELETED: f64 = -1.0;

/// `validateInput`: a string's code units, or an array of strings, as diff items.
fn diff_items(c: &mut Context<'_>, value: RootId, name: &str) -> Result<Vec<Vec<u16>>, RootedError> {
    if let Some(units) = c.string_code_units(value)? {
        return Ok(units.into_iter().map(|unit| vec![unit]).collect());
    }
    if !c.is_array_rooted(value)? {
        return Err(super::util_shared_vm::argument_type_error(c, value, name, "string")?);
    }
    let mut items = Vec::new();
    for (index, item) in c.array_like_values_rooted(value)?.into_iter().enumerate() {
        match c.string_code_units(item)? {
            Some(units) => items.push(units),
            None => return Err(super::util_shared_vm::argument_type_error(c, item, &format!("{name}[{index}]"), "string")?),
        }
    }
    Ok(items)
}

/// Node's `myersDiff` and `backtrack` (internal/assert/myers_diff.js), as (operation, item).
fn myers_diff<'a>(actual: &'a [Vec<u16>], expected: &'a [Vec<u16>]) -> Vec<(f64, &'a [u16])> {
    let max = actual.len() + expected.len();
    let offset = |diagonal: isize| (diagonal + max as isize) as usize;
    let mut v = vec![0usize; 2 * max + 1];
    let mut trace = Vec::new();
    'search: for level in 0..=max as isize {
        trace.push(v.clone());
        let mut diagonal = -level;
        while diagonal <= level {
            let index = offset(diagonal);
            let mut x = if diagonal == -level || (diagonal != level && v[index - 1] < v[index + 1]) { v[index + 1] } else { v[index - 1] + 1 };
            let mut y = (x as isize - diagonal) as usize;
            while x < actual.len() && y < expected.len() && actual[x] == expected[y] {
                x += 1;
                y += 1;
            }
            v[index] = x;
            if x >= actual.len() && y >= expected.len() {
                break 'search;
            }
            diagonal += 2;
        }
    }
    let (mut x, mut y) = (actual.len(), expected.len());
    let mut result = Vec::new();
    for (level, v) in trace.iter().enumerate().rev() {
        let level = level as isize;
        let diagonal = x as isize - y as isize;
        let index = offset(diagonal);
        let previous = if diagonal == -level || (diagonal != level && v[index - 1] < v[index + 1]) { diagonal + 1 } else { diagonal - 1 };
        let previous_x = v[offset(previous)];
        let previous_y = previous_x as isize - previous;
        while x > previous_x && y as isize > previous_y {
            result.push((UNCHANGED, actual[x - 1].as_slice()));
            x -= 1;
            y -= 1;
        }
        if level > 0 {
            if x > previous_x {
                result.push((INSERTED, actual[x - 1].as_slice()));
                x -= 1;
            } else {
                result.push((DELETED, expected[y - 1].as_slice()));
                y -= 1;
            }
        }
    }
    result.reverse();
    result
}

/// `util.diff(actual, expected)`.
pub(crate) fn diff(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let actual = args.first().copied().unwrap_or(undefined);
    let expected = args.get(1).copied().unwrap_or(undefined);
    let actual_items = diff_items(c, actual, "actual")?;
    let expected_items = diff_items(c, expected, "expected")?;
    if c.strict_equal_rooted(actual, expected)? || (actual_items.is_empty() && expected_items.is_empty()) {
        return c.array_rooted(&[]);
    }
    let mut entries = Vec::new();
    for (operation, item) in myers_diff(&actual_items, &expected_items) {
        let operation = c.number(operation);
        let item = c.string_units_rooted(item);
        entries.push(c.array_rooted(&[operation, item])?);
    }
    c.array_rooted(&entries)
}
