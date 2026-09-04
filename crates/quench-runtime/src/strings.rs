use crate::value::Value;

/// Return the stable FNV-1a hash of UTF-16 code units.
///
/// This is the canonical computation used by the owner-local `StringUnits`
/// cache and by direct hashing of well-formed UTF-8 strings.
pub(crate) fn hash_units(units: &[u16]) -> u64 {
    units
        .iter()
        .fold(0xcbf29ce484222325, |hash, unit| hash_unit(hash, *unit))
}

#[inline]
fn hash_unit(hash: u64, unit: u16) -> u64 {
    (hash ^ u64::from(unit)).wrapping_mul(0x100000001b3)
}

/// Hash a UTF-8 string using the same UTF-16-unit algorithm as `hash_units`.
pub(crate) fn hash_str(value: &str) -> u64 {
    value.encode_utf16().fold(0xcbf29ce484222325, hash_unit)
}

/// Hash a canonical runtime string without materializing a representation.
///
/// `StringUnits` owns its immutable code units and a lazily initialized hash
/// cache. Clones share both through the same `Rc`, so the cache lifecycle is
/// exactly the canonical value lifecycle; well-formed `String` values remain
/// derived from their owned UTF-8 source.
#[inline]
pub(crate) fn hash_value(value: &Value) -> Option<u64> {
    match value {
        Value::String(value) => Some(hash_str(value)),
        Value::StringUnits(value) => Some(value.cached_hash(hash_units)),
        _ => None,
    }
}

/// Strings have one canonical representation today: `String` (UTF-8) or the
/// UTF-16-unit container produced by `from_units`. Both are owned flat buffers.
/// A rope would need runtime-owned nodes, flattening rules, and lifecycle/
/// invalid-state handling; none exists, so concatenation must remain flat.
///
/// Flat concatenation policy: `concat` appends converted UTF-16 units directly
/// into one newly-owned `Vec<u16>`. The receiver and each argument remain the
/// semantic source of truth until conversion; no rope nodes, slices, or cached
/// flattened copies are retained. The vector is born in the concat call, is
/// transferred to `Value::String`/`Value::StringUnits` by `from_units`, and is
/// dropped on conversion failure. A result is invalid if its checked UTF-16
/// byte size exceeds this limit; the operation returns a RangeError before
/// appending the offending argument.
pub(crate) const MAX_STRING_BYTES: usize = MAX_STRING_UNITS * std::mem::size_of::<u16>();
/// V8 exposes this UTF-16-unit limit through `buffer.constants`.
const MAX_STRING_UNITS: usize = 536_870_888;

#[inline]
fn string_byte_len_fits_limit(bytes: usize) -> bool {
    bytes <= MAX_STRING_BYTES
}

#[inline]
fn units_fit_limit(units: usize) -> bool {
    units
        .checked_mul(std::mem::size_of::<u16>())
        .is_some_and(string_byte_len_fits_limit)
}

#[inline]
fn units_add_fits_limit(left: usize, right: usize) -> Option<usize> {
    left.checked_add(right)
        .filter(|total| units_fit_limit(*total))
}

#[inline]

pub(crate) fn string_bytes_fit_limit(value: &str) -> bool {
    string_byte_len_fits_limit(value.len())
}

pub(crate) fn replace_discard_string(
    input: &str,
    regexp: &Value,
    replacement: &Value,
) -> Result<(), crate::execute::VmError> {
    crate::regexp::execute_builtin(
        crate::ops::Builtin::RegExpSymbolReplace,
        Some(regexp),
        &[Value::String(input.into()), replacement.clone()],
    )
    .ok_or(crate::execute::VmError::NotCallable)?
    .map(|_| ())
}

#[cfg(test)]
mod hash_tests {
    use super::{
        encoding_of, for_each_unit, hash_str, hash_units, is_short_string, is_short_units,
        short_string_layout, source_encoding, string_byte_len_fits_limit, string_bytes_fit_limit,
        units_add_fits_limit, units_fit_limit, ShortStringLayout, StringEncoding,
        StringSourceEncoding, Value, MAX_STRING_BYTES, SHORT_STRING_MAX_UNITS,
    };
    use crate::value::StringUnitsData;

    // The immutable UTF-16 owner lazily computes its hash once and shares the
    // cached result across clones without changing semantic string contents.
    #[test]
    fn utf16_hash_cache_is_lazy_and_shared() {
        let data = StringUnitsData::new(vec![0xD800, 0x0061]);
        let calls = std::cell::Cell::new(0);
        let first = data.cached_hash(|units| {
            calls.set(calls.get() + 1);
            hash_units(units)
        });
        let second = data.cached_hash(|_| {
            calls.set(calls.get() + 1);
            0
        });
        assert_eq!(first, hash_units(&[0xD800, 0x0061]));
        assert_eq!(second, first);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn canonical_hash_uses_owned_value_and_shared_cache_lifecycle() {
        let value = super::from_units(vec![0xD800, 0x0061]);
        let expected = hash_units(&[0xD800, 0x0061]);
        assert_eq!(super::hash_value(&value), Some(expected));
        let clone = value.clone();
        drop(value);
        assert_eq!(super::hash_value(&clone), Some(expected));
    }

    #[test]
    fn canonical_hash_rejects_non_strings() {
        assert_eq!(super::hash_value(&Value::Null), None);
    }

    #[test]
    fn utf16_hash_is_stable_across_repeated_derivation() {
        let units = [0, 0xFFFF, 0xD800, 0x61];
        assert_eq!(hash_units(&units), hash_units(&units));
    }
    // Well-formed UTF-8 strings derive directly from their canonical buffer;
    // lone-surrogate strings use the owner-local cache above.
    #[test]
    fn utf8_hash_matches_utf16_hash_without_buffer() {
        let value = "héllo";
        let units: Vec<u16> = value.encode_utf16().collect();
        assert_eq!(hash_str(value), hash_units(&units));
    }
    #[test]
    fn short_string_boundary_is_explicit() {
        assert!(is_short_string("a".repeat(22).as_str()));
        assert!(!is_short_string("a".repeat(23).as_str()));
    }
    #[test]
    fn short_string_budget_counts_utf16_units_not_utf8_bytes() {
        let value = "😀".repeat(11);
        assert_eq!(value.encode_utf16().count(), SHORT_STRING_MAX_UNITS);
        assert!(is_short_string(&value));
        assert!(!is_short_string(&(value + "😀")));
    }

    #[test]
    fn compact_layout_is_derived_from_canonical_owner() {
        let utf8 = Value::String("short".into());
        let utf16 = super::from_units(vec![0xD800]);
        assert_eq!(source_encoding(&utf8), Some(StringSourceEncoding::Utf8));
        assert_eq!(source_encoding(&utf16), Some(StringSourceEncoding::Utf16));
        assert_eq!(short_string_layout(&utf8), Some(ShortStringLayout::Utf8));
        assert_eq!(short_string_layout(&utf16), Some(ShortStringLayout::Utf16));
        assert_eq!(short_string_layout(&Value::Null), None);
    }
    #[test]
    fn utf16_capacity_uses_code_units() {
        assert!(is_short_units(&[0xD83D, 0xDE00]));
        assert!(!is_short_units(&[0; 23]));
    }
    #[test]
    fn encoding_classification_preserves_latin1_boundary() {
        assert_eq!(encoding_of(&[0x41, 0xFF]), StringEncoding::Latin1);
        assert_eq!(encoding_of(&[0x100]), StringEncoding::Utf16);
    }
    #[test]
    fn unit_visitation_avoids_materialization() {
        let mut units = Vec::new();
        assert!(for_each_unit(&Value::String("hé".into()), |unit| units.push(unit)));
        assert_eq!(units, vec![b'h' as u16, 0xE9]);
    }
    #[test]
    fn string_memory_limit_is_explicit() {
        assert!(string_bytes_fit_limit("small"));
    }
    #[test]
    fn string_memory_limit_accepts_exact_boundary() {
        assert!(string_byte_len_fits_limit(MAX_STRING_BYTES));
    }

    #[test]
    fn flat_concat_limit_is_checked_in_utf16_bytes() {
        let exact_units = MAX_STRING_BYTES / std::mem::size_of::<u16>();
        assert_eq!(units_add_fits_limit(exact_units - 1, 1), Some(exact_units));
        assert_eq!(units_add_fits_limit(exact_units, 1), None);
        assert!(units_fit_limit(exact_units));
        assert!(!units_fit_limit(exact_units + 1));
    }
}

/// Maximum number of UTF-16 code units eligible for the compact short form.
///
/// This is a policy boundary, not a second string representation: callers
/// classify the existing owned value and must continue to use that value as
/// the semantic source of truth.
pub(crate) const SHORT_STRING_MAX_UNITS: usize = 22;

#[inline]
pub(crate) fn is_latin1(units: &[u16]) -> bool {
    units.iter().all(|unit| *unit <= 0xff)
}

#[inline]
pub(crate) fn is_short_string(value: &str) -> bool {
    value.encode_utf16().count() <= SHORT_STRING_MAX_UNITS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringEncoding {
    Latin1,
    Utf16,
}

/// Canonical source encoding owned by a runtime string.
///
/// `Utf8` values are valid Rust strings and therefore cannot contain lone
/// surrogates. `Utf16` values retain exact JavaScript code units, including
/// lone surrogates, in the immutable `StringUnits` allocation. This is a
/// description of the owning value, not a second buffer or a conversion
/// cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringSourceEncoding {
    Utf8,
    Utf16,
}

#[inline]
pub(crate) fn source_encoding(value: &Value) -> Option<StringSourceEncoding> {
    match value {
        Value::String(_) => Some(StringSourceEncoding::Utf8),
        Value::StringUnits(_) => Some(StringSourceEncoding::Utf16),
        _ => None,
    }
}

/// Storage family derived from the canonical value; no semantic bytes retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortStringLayout {
    Utf8,
    Utf16,
}

#[inline]
pub(crate) fn short_string_layout(value: &Value) -> Option<ShortStringLayout> {
    match value {
        Value::String(text) if text.encode_utf16().count() <= SHORT_STRING_MAX_UNITS => {
            Some(ShortStringLayout::Utf8)
        }
        Value::StringUnits(units) if is_short_units(units) => Some(ShortStringLayout::Utf16),
        _ => None,
    }
}

#[inline]
pub(crate) fn encoding_of(units: &[u16]) -> StringEncoding {
    if is_latin1(units) {
        StringEncoding::Latin1
    } else {
        StringEncoding::Utf16
    }
}
/// Classify an owned runtime string without creating a second semantic value.
///
/// `Value::String` remains UTF-8-owned and `Value::StringUnits` remains the
/// raw UTF-16 source of truth; this reports only the derived compact storage
/// family. Latin-1 is valid exactly when every UTF-16 code unit is at most
/// `0xff`, including empty strings.
#[inline]
pub(crate) fn encoding_of_value(value: &Value) -> Option<StringEncoding> {
    match value {
        Value::String(text) => Some(if text.encode_utf16().all(|unit| unit <= 0xff) {
            StringEncoding::Latin1
        } else {
            StringEncoding::Utf16
        }),
        Value::StringUnits(units) => Some(encoding_of(units)),
        _ => None,
    }
}

/// Number of bytes needed by the derived compact representation.
///
/// This is an accounting helper only: bytes are not retained alongside the
/// canonical `Value`, so classification cannot make semantic copies stale.
#[inline]
pub(crate) fn compact_storage_bytes(units: &[u16]) -> usize {
    match encoding_of(units) {
        StringEncoding::Latin1 => units.len(),
        StringEncoding::Utf16 => units.len().saturating_mul(std::mem::size_of::<u16>()),
    }
}

#[inline]
pub(crate) fn latin1_to_units(bytes: &[u8]) -> Vec<u16> {
    bytes.iter().map(|&byte| u16::from(byte)).collect()
}

#[inline]
pub(crate) fn units_to_latin1(units: &[u16]) -> Option<Vec<u8>> {
    if !is_latin1(units) {
        return None;
    }
    Some(units.iter().map(|&unit| unit as u8).collect())
}
/// Construct the canonical runtime string from Latin-1 code units.
///
/// The byte slice is an input view only: it is widened once into the
/// canonical UTF-16/UTF-8 representation and is never retained as a second
/// semantic buffer. Every byte is valid Latin-1, including an empty slice.
#[inline]
pub(crate) fn from_latin1(bytes: &[u8]) -> Value {
    from_units(latin1_to_units(bytes))
}

#[inline]
pub(crate) fn is_short_units(units: &[u16]) -> bool {
    units.len() <= SHORT_STRING_MAX_UNITS
}

/// Convert raw UTF-16 units into the canonical runtime value.
///
/// `Value::String` is the sole well-formed UTF-16 source. Invalid UTF-16
/// (lone surrogates) cannot be represented by Rust `String`, so the exact
/// units remain owned by `Value::StringUnits`. Encoding classifications and
/// compact layouts must be derived from these values, never stored as a
/// competing semantic buffer.
pub(crate) fn from_units(units: Vec<u16>) -> Value {
    // Large UTF-16 slices are already in the representation consumed by
    // indexed string operations. Retain that flat storage instead of
    // transcoding to UTF-8 and rebuilding the units vector on every
    // `charCodeAt` call. This is a generic representation choice for large
    // indexed values; observable string semantics remain unchanged.
    if units.len() > 1024 {
        return Value::StringUnits(std::rc::Rc::new(
            crate::value::StringUnitsData::new(units),
        ));
    }
    match String::from_utf16(&units) {
        Ok(value) => Value::String(value),
        Err(_) => Value::StringUnits(std::rc::Rc::new(crate::value::StringUnitsData::new(units))),
    }
}

include!("strings_static.rs");

/// Expand the canonical source into UTF-16 code units at an API boundary.
///
/// `Value::String` remains the sole source for well-formed text and is encoded
/// only for this operation. `Value::StringUnits` already owns exact units, so
/// expansion is just a clone of that immutable source. The returned vector is
/// temporary boundary state: it is never retained by the runtime or attached
/// to the value. Lone surrogates therefore survive exactly, while a valid
/// UTF-8 string gets the JavaScript UTF-16 representation (including pairs).
pub(crate) fn expand_utf16(value: &Value) -> Option<Vec<u16>> {
    match value {
        Value::String(value) => Some(value.encode_utf16().collect()),
        Value::StringUnits(units) => Some((**units).to_vec()),
        _ => None,
    }
}

/// Compatibility name for callers that need a materialized UTF-16 boundary.
#[inline]
pub(crate) fn units_of(value: &Value) -> Option<Vec<u16>> {
    expand_utf16(value)
}
/// Borrowed view of canonical string storage.
///
/// This is the allocation boundary for string algorithms: callers that only
/// inspect code units must use this view rather than materializing a `Vec`.
/// The view never owns or caches data, and cannot outlive the `Value` it
/// borrows. `StringUnits` remains reference-counted so cloning a `Value`
/// shares its immutable backing allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringView<'a> {
    Utf8(&'a str),
    Utf16(&'a [u16]),
}

#[inline]
pub(crate) fn view_of(value: &Value) -> Option<StringView<'_>> {
    match value {
        Value::String(value) => Some(StringView::Utf8(value)),
        Value::StringUnits(units) => Some(StringView::Utf16(units)),
        _ => None,
    }
}

#[inline]
pub(crate) fn view_len_units(view: StringView<'_>) -> usize {
    match view {
        StringView::Utf8(value) => value.encode_utf16().count(),
        StringView::Utf16(units) => units.len(),
    }
}

/// Materialize the canonical string at host/serialization boundaries.
/// Lone surrogates use the replacement semantics of host string APIs.
#[inline]
pub(crate) fn materialize(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::StringUnits(units) => Some(String::from_utf16_lossy(units)),
        _ => None,
    }
}

/// Whether `units` form a well-formed UTF-16 sequence (no lone surrogates).
pub(crate) fn units_well_formed(units: &[u16]) -> bool {
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if is_high_surrogate(unit) {
            if !units
                .get(index + 1)
                .is_some_and(|next| is_low_surrogate(*next))
            {
                return false;
            }
            index += 2;
        } else if is_low_surrogate(unit) {
            return false;
        } else {
            index += 1;
        }
    }
    true
}
/// Visit UTF-16 code units without materializing a temporary vector.
pub(crate) fn for_each_unit(value: &Value, mut visit: impl FnMut(u16)) -> bool {
    match value {
        Value::String(value) => {
            value.encode_utf16().for_each(&mut visit);
            true
        }
        Value::StringUnits(units) => {
            units.iter().copied().for_each(&mut visit);
            true
        }
        _ => false,
    }
}

/// The string value of the code point at UTF-16 code-unit `index`, preserving
/// a lone surrogate as a one-unit `StringUnits`.
pub(crate) fn char_at_units(units: &[u16], index: usize) -> Option<Value> {
    let unit = *units.get(index)?;
    Some(from_units(vec![unit]))
}

/// The code point beginning at `index` within `units`, folding a valid
/// surrogate pair and otherwise yielding the lone code unit.
fn code_point(units: &[u16], index: usize) -> u32 {
    let unit = units[index];
    if index + 1 < units.len() && is_high_surrogate(unit) && is_low_surrogate(units[index + 1]) {
        0x1_0000 + (((unit - 0xD800) as u32) << 10) + (units[index + 1] - 0xDC00) as u32
    } else {
        unit as u32
    }
}

fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..0xDC00).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..0xE000).contains(&unit)
}

fn is_surrogate(code: u32) -> bool {
    (0xD800..0xE000).contains(&code)
}

pub(crate) fn property_method(key: &str) -> Option<crate::ops::Builtin> {
    if key == "Symbol.iterator" {
        return Some(crate::ops::Builtin::StringIterator);
    }
    match key {
        "anchor" => Some(crate::ops::Builtin::StringAnchor),
        "big" => Some(crate::ops::Builtin::StringBig),
        "blink" => Some(crate::ops::Builtin::StringBlink),
        "bold" => Some(crate::ops::Builtin::StringBold),
        "fixed" => Some(crate::ops::Builtin::StringFixed),
        "fontcolor" => Some(crate::ops::Builtin::StringFontcolor),
        "fontsize" => Some(crate::ops::Builtin::StringFontsize),
        "italics" => Some(crate::ops::Builtin::StringItalics),
        "link" => Some(crate::ops::Builtin::StringLink),
        "strike" => Some(crate::ops::Builtin::StringStrike),
        "small" => Some(crate::ops::Builtin::StringSmall),
        "includes" => Some(crate::ops::Builtin::StringIncludes),
        "isWellFormed" => Some(crate::ops::Builtin::StringIsWellFormed),
        "toWellFormed" => Some(crate::ops::Builtin::StringToWellFormed),
        "startsWith" => Some(crate::ops::Builtin::StringStartsWith),
        "endsWith" => Some(crate::ops::Builtin::StringEndsWith),
        "at" => Some(crate::ops::Builtin::StringAt),
        "repeat" => Some(crate::ops::Builtin::StringRepeat),
        "trim" => Some(crate::ops::Builtin::StringTrim),
        "toLowerCase" => Some(crate::ops::Builtin::StringToLowerCase),
        "toUpperCase" => Some(crate::ops::Builtin::StringToUpperCase),
        "normalize" => Some(crate::ops::Builtin::StringNormalize),
        "charAt" => Some(crate::ops::Builtin::StringCharAt),
        "charCodeAt" => Some(crate::ops::Builtin::StringCharCodeAt),
        "indexOf" => Some(crate::ops::Builtin::StringIndexOf),
        "lastIndexOf" => Some(crate::ops::Builtin::StringLastIndexOf),
        "slice" => Some(crate::ops::Builtin::StringSlice),
        "substring" => Some(crate::ops::Builtin::StringSubstring),
        "substr" => Some(crate::ops::Builtin::StringSubstr),
        "sub" => Some(crate::ops::Builtin::StringSub),
        "sup" => Some(crate::ops::Builtin::StringSup),
        "concat" => Some(crate::ops::Builtin::StringConcat),
        "split" => Some(crate::ops::Builtin::StringSplit),
        "padStart" => Some(crate::ops::Builtin::StringPadStart),
        "padEnd" => Some(crate::ops::Builtin::StringPadEnd),
        "trimStart" => Some(crate::ops::Builtin::StringTrimStart),
        "trimLeft" => Some(crate::ops::Builtin::StringTrimStart),
        "trimEnd" => Some(crate::ops::Builtin::StringTrimEnd),
        "trimRight" => Some(crate::ops::Builtin::StringTrimEnd),
        "codePointAt" => Some(crate::ops::Builtin::StringCodePointAt),
        "toString" => Some(crate::ops::Builtin::StringToString),
        "valueOf" => Some(crate::ops::Builtin::StringValueOf),
        "replace" => Some(crate::ops::Builtin::StringReplace),
        "replaceAll" => Some(crate::ops::Builtin::StringReplaceAll),
        "search" => Some(crate::ops::Builtin::StringSearch),
        "localeCompare" => Some(crate::ops::Builtin::StringLocaleCompare),
        "match" => Some(crate::ops::Builtin::StringMatch),
        "matchAll" => Some(crate::ops::Builtin::StringMatchAll),
        "toLocaleLowerCase" => Some(crate::ops::Builtin::StringToLocaleLowerCase),
        "toLocaleUpperCase" => Some(crate::ops::Builtin::StringToLocaleUpperCase),
        _ => None,
    }
}

include!("strings_execute.rs");

pub(crate) fn repeat(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = string_receiver(receiver)?;
    let count_value = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    if count_value.is_infinite() {
        return Err(crate::value::error::throw_range_error(
            "String.prototype.repeat: count must be finite",
        ));
    }
    if count_value < 0.0 {
        return Err(crate::value::error::throw_range_error(
            "String.prototype.repeat: count must be non-negative",
        ));
    }
    let count = count_value as usize;
    let source = receiver
        .and_then(units_of)
        .unwrap_or_else(|| value.encode_utf16().collect());
    let total_units = source
        .len()
        .checked_mul(count)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid string length"))?;
    if total_units > MAX_STRING_UNITS {
        return Err(crate::value::error::throw_range_error(
            "Invalid string length",
        ));
    }
    if !units_fit_limit(total_units) {
        return Err(crate::value::error::throw_range_error(
            "Invalid string length",
        ));
    }
    // Keep large indexed values in the UTF-16 storage already assembled above:
    // repeated `charCodeAt` calls can then address one unit without rebuilding
    // the entire encoding on every call. Small ordinary strings retain their
    // compact UTF-8 representation.
    if total_units > 1024 {
        let mut result = Vec::with_capacity(total_units);
        for _ in 0..count {
            result.extend_from_slice(&source);
        }
        return Ok(Value::StringUnits(std::rc::Rc::new(
            crate::value::StringUnitsData::new(result),
        )));
    }
    if source.len() == value.encode_utf16().count() {
        return Ok(Value::String(value.repeat(count)));
    }
    let mut result = Vec::with_capacity(total_units);

    for _ in 0..count {
        result.extend_from_slice(&source);
    }
    Ok(from_units(result))
}

pub(crate) fn to_lower_case(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String(string_receiver(receiver)?.to_lowercase()))
}

pub(crate) fn to_upper_case(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String(string_receiver(receiver)?.to_uppercase()))
}

pub(crate) fn char_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let index = arguments
        .first()
        .map_or(Ok(0.0), crate::conversion::to_number)?;
    let index = index.trunc();
    if index < 0.0 {
        return Ok(Value::String(String::new()));
    }
    let index = index as usize;
    let Some(first) = units.get(index).copied() else {
        return Ok(Value::String(String::new()));
    };
    let value = if is_high_surrogate(first) {
        if let Some(second) = units
            .get(index + 1)
            .copied()
            .filter(|unit| is_low_surrogate(*unit))
        {
            from_units(vec![first, second])
        } else {
            from_units(vec![first])
        }
    } else {
        from_units(vec![first])
    };
    Ok(value)
}

pub(crate) fn char_code_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let index = arguments
        .first()
        .map_or(Ok(0.0), crate::conversion::to_number)?;
    let index = index.trunc();
    if index < 0.0 {
        return Ok(Value::Number(f64::NAN));
    }
    let index = index as usize;
    // Keep the indexed operation on the canonical owner. In particular, do
    // not route `StringUnits` through the general receiver adapter: that
    // adapter is intentionally allocation-free for most callers but still
    // performs boxed-string/coercion checks that are unnecessary here.
    let unit = match receiver {
        Some(Value::StringUnits(units)) => units.get(index).copied(),
        Some(Value::String(value)) => utf16_code_unit(value, index),
        _ => None,
    };
    Ok(unit.map_or(Value::Number(f64::NAN), |unit| Value::Number(unit as f64)))
}

/// Numeric-index fast path used by the registered method executor. Returning
/// `None` keeps boxed/non-string receivers on the ordinary coercion path.
#[inline(always)]
pub(crate) fn char_code_at_number(receiver: &Value, index: f64) -> Option<f64> {
    if !matches!(receiver, Value::String(_) | Value::StringUnits(_)) {
        return None;
    }
    if index.is_nan() || index < 0.0 {
        return Some(f64::NAN);
    }
    let index = index.trunc() as usize;
    let unit = match receiver {
        Value::StringUnits(units) => units.get(index).copied(),
        Value::String(value) => utf16_code_unit(value, index),
        _ => unreachable!(),
    };
    Some(unit.map_or(f64::NAN, f64::from))
}

pub(crate) fn slice(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let length = units.len() as isize;
    let start = string_index(arguments.first(), length)?;
    let end = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
        .map_or(Ok(length), |value| string_index(Some(value), length))?;
    let range = if start < end {
        start as usize..end as usize
    } else {
        0..0
    };
    Ok(from_units(units[range].to_vec()))
}

pub(crate) fn substring(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let length = units.len() as isize;
    let start = substring_index(arguments.first(), length)?;
    let end = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined))
        .map_or(Ok(length), |value| substring_index(Some(value), length))?;
    let range = start.min(end) as usize..end.max(start) as usize;
    Ok(from_units(units[range].to_vec()))
}

pub(crate) fn substr(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let text = string_receiver(receiver)?;
    let units: Vec<u16> = text.encode_utf16().collect();
    let size = units.len() as f64;
    let start = substr_start(to_integer_or_infinity(arguments.first())?, size);
    let length = substr_length(arguments.get(1))?;
    let result_len = length.max(0.0).min(size - start);
    if result_len <= 0.0 {
        return Ok(Value::String(String::new()));
    }
    let begin = start as usize;
    let end = (start + result_len) as usize;
    Ok(from_units(units[begin..end].to_vec()))
}

fn to_integer_or_infinity(value: Option<&Value>) -> Result<f64, crate::execute::VmError> {
    let number = value.map_or(Ok(0.0), crate::conversion::to_number)?;
    if number.is_nan() {
        return Ok(0.0);
    }
    Ok(if number.is_infinite() {
        number
    } else {
        number.trunc()
    })
}

fn substr_start(int_start: f64, size: f64) -> f64 {
    if int_start == f64::NEG_INFINITY {
        return 0.0;
    }
    if int_start < 0.0 {
        (size + int_start).max(0.0)
    } else {
        int_start.min(size)
    }
}

fn substr_length(value: Option<&Value>) -> Result<f64, crate::execute::VmError> {
    match value {
        None | Some(Value::Undefined) => Ok(f64::INFINITY),
        Some(value) => to_integer_or_infinity(Some(value)),
    }
}

fn string_index(value: Option<&Value>, length: isize) -> Result<isize, crate::execute::VmError> {
    let number = value
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let number = if number.is_nan() { 0.0 } else { number.trunc() } as isize;
    if number < 0 {
        Ok((length + number).max(0))
    } else {
        Ok(number.min(length))
    }
}

fn substring_index(value: Option<&Value>, length: isize) -> Result<isize, crate::execute::VmError> {
    let number = value
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let number = if number.is_nan() { 0.0 } else { number.trunc() } as isize;
    Ok(number.clamp(0, length))
}

/// Flatten a concatenation into one owned UTF-16 buffer.
///
/// The buffer is the sole transient assembly representation: inputs remain
/// authoritative, and no rope nodes or cached flattened copies escape this
/// function. String values are appended from their canonical view so lone
/// surrogates remain exact; non-strings are converted once at the JavaScript
/// coercion boundary.
fn flatten_concat(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Vec<u16>, crate::execute::VmError> {
    let mut units = match receiver {
        Some(value) if view_of(value).is_some() => {
            let length = view_len_units(view_of(value).expect("string view"));
            if !units_fit_limit(length) {
                return Err(crate::value::error::throw_range_error(
                    "String.prototype.concat: result is too large",
                ));
            }
            let mut output = Vec::with_capacity(length);
            for_each_unit(value, |unit| output.push(unit));
            output
        }
        _ => {
            let value = string_receiver(receiver)?;
            let length = value.encode_utf16().count();
            if !units_fit_limit(length) {
                return Err(crate::value::error::throw_range_error(
                    "String.prototype.concat: result is too large",
                ));
            }
            value.encode_utf16().collect()
        }
    };
    for argument in arguments {
        let converted = crate::conversion::to_string(argument)?;
        let additional = converted.encode_utf16().count();
        let total = units_add_fits_limit(units.len(), additional).ok_or_else(|| {
            crate::value::error::throw_range_error("String.prototype.concat: result is too large")
        })?;
        units.reserve(total.saturating_sub(units.len()));
        units.extend(converted.encode_utf16());
    }
    Ok(units)
}

pub(crate) fn concat(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    Ok(from_units(flatten_concat(receiver, arguments)?))
}

pub(crate) fn locale_compare(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let left = string_receiver(receiver)?;
    let right = arguments
        .first()
        .map_or(Ok(String::from("undefined")), crate::conversion::to_string)?;
    let normalized_left =
        unicode_normalization::UnicodeNormalization::nfd(left.chars()).collect::<String>();
    let normalized_right =
        unicode_normalization::UnicodeNormalization::nfd(right.chars()).collect::<String>();
    if normalized_left == normalized_right {
        return Ok(Value::Number(0.0));
    }
    Ok(Value::Number(crate::intl::collator::compare(
        &left,
        &right,
        &crate::intl::default_locale(),
        false,
        "variant",
    )))
}

pub(crate) fn pad_start(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    pad(receiver, arguments, true)
}

pub(crate) fn pad_end(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    pad(receiver, arguments, false)
}

fn pad(
    receiver: Option<&Value>,
    arguments: &[Value],
    start: bool,
) -> Result<Value, crate::execute::VmError> {
    let value = string_receiver(receiver)?;
    let target = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let target = if target.is_nan() || target <= 0.0 {
        0
    } else if target.is_infinite() {
        return Err(crate::value::error::throw_range_error(
            "Invalid string length",
        ));
    } else {
        target.trunc() as usize
    };
    let value_units = receiver
        .and_then(units_of)
        .unwrap_or_else(|| value.encode_utf16().collect());
    let value_len = value_units.len();
    let result_units = value_len.max(target);
    if !units_fit_limit(result_units) {
        return Err(crate::value::error::throw_range_error(
            "String.prototype.padStart/padEnd: result is too large",
        ));
    }
    let fill = match arguments.get(1) {
        None | Some(Value::Undefined) => " ".to_string(),
        Some(value) => crate::conversion::to_string(value)?,
    };
    let count = target.saturating_sub(value_len);
    let padding_units: Vec<u16> = fill.encode_utf16().cycle().take(count).collect();
    if count == 0 {
        return Ok(Value::String(value));
    }
    let mut result_units = Vec::with_capacity(result_units);
    if start {
        result_units.extend(padding_units);
        result_units.extend(value_units.iter().copied());
    } else {
        result_units.extend(value_units.iter().copied());
        result_units.extend(padding_units);
    }
    Ok(from_units(result_units))
}
pub(crate) fn code_point_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let units = receiver_units(receiver)?;
    let mut position = match arguments.first() {
        None => 0.0,
        Some(value) => {
            let number = crate::conversion::to_number(value)?;
            number.trunc()
        }
    };
    if position.is_nan() {
        position = 0.0;
    }
    if position < 0.0 || position >= units.len() as f64 {
        return Ok(Value::Undefined);
    }
    let code = code_point(&units, position as usize);
    Ok(Value::Number(code as f64))
}
pub(crate) fn to_string_value(receiver: Option<&Value>) -> Value {
    receiver
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()))
}

include!("strings_tail.rs");

include!("strings_search.rs");

#[cfg(test)]
mod tests {
    use super::{
        char_at_units, concat, encoding_of, from_latin1, from_units, hash_units, is_latin1,
        latin1_to_units, materialize, short_string_layout, source_encoding, units_of,
        units_to_latin1, units_well_formed, view_len_units, view_of, ShortStringLayout,
        StringEncoding, StringSourceEncoding, StringView, MAX_STRING_BYTES,
    };

    use crate::{construct::construct_value, ops::Builtin, value::Value};
    #[test]
    fn materialize_is_the_explicit_utf16_boundary() {
        let units = from_units(vec![0x41, 0xd800, 0x42]);
        assert_eq!(materialize(&units).as_deref(), Some("A�B"));
        assert_eq!(
            materialize(&Value::String("😀".into())).as_deref(),
            Some("😀")
        );
        assert_eq!(materialize(&Value::Number(1.0)), None);
    }

    #[test]
    fn latin1_round_trip_is_delayed() {
        let units = [65, 0xff];
        let bytes = units_to_latin1(&units).expect("compact");
        assert_eq!(latin1_to_units(&bytes), units);
        assert!(!is_latin1(&[0x100]));
        assert!(units_to_latin1(&[0x100]).is_none());
    }

    #[test]
    fn encoding_classification_does_not_change_utf16_units() {
        let latin1 = [0x41, 0xff];
        let utf16 = [0x41, 0x100];
        assert_eq!(encoding_of(&latin1), StringEncoding::Latin1);
        assert_eq!(encoding_of(&utf16), StringEncoding::Utf16);
        assert!(units_well_formed(&latin1));
        assert!(units_well_formed(&utf16));
    }

    #[test]
    fn invalid_surrogates_remain_raw_and_round_trip() {
        let units = vec![0x0061, 0xd800, 0x0062, 0xdc00];
        assert!(!units_well_formed(&units));
        let value = from_units(units.clone());
        assert!(matches!(value, Value::StringUnits(_)));
        assert_eq!(units_of(&value), Some(units));
    }

    #[test]
    fn char_at_preserves_lone_surrogate_boundaries() {
        let units = [0xd800, 0x0061, 0xdc00];
        let high = char_at_units(&units, 0).expect("high surrogate");
        let plain = char_at_units(&units, 1).expect("ascii");
        let low = char_at_units(&units, 2).expect("low surrogate");
        assert!(matches!(high, Value::StringUnits(_)));
        assert_eq!(plain, Value::String("a".to_string()));
        assert!(matches!(low, Value::StringUnits(_)));
    }

    #[test]
    fn char_at_expands_utf16_only_at_code_unit_boundary() {
        let value = Value::String("A😀Z".to_string());
        assert_eq!(
            super::char_at(Some(&value), &[Value::Number(0.0)]).unwrap(),
            Value::String("A".to_string())
        );
        assert_eq!(
            super::char_at(Some(&value), &[Value::Number(1.0)]).unwrap(),
            Value::String("😀".to_string())
        );
        assert_eq!(
            super::char_at(Some(&value), &[Value::Number(2.0)]).unwrap(),
            Value::StringUnits(std::rc::Rc::new(crate::value::StringUnitsData::new(vec![
                0xde00
            ])))
        );
        assert_eq!(
            super::char_at(Some(&value), &[Value::Number(3.0)]).unwrap(),
            Value::String("Z".to_string())
        );
        assert_eq!(
            super::char_at(Some(&value), &[Value::Number(4.0)]).unwrap(),
            Value::String(String::new())
        );
    }

    #[test]
    fn char_code_at_reports_utf16_units_without_code_point_folding() {
        let value = Value::String("A😀Z".to_string());
        for (index, expected) in [(0.0, 0x41), (1.0, 0xd83d), (2.0, 0xde00), (3.0, 0x5a)] {
            assert_eq!(
                super::char_code_at(Some(&value), &[Value::Number(index)]).unwrap(),
                Value::Number(expected as f64)
            );
        }
        assert!(matches!(
            super::char_code_at(Some(&value), &[Value::Number(4.0)]).unwrap(),
            Value::Number(value) if value.is_nan()
        ));
    }

    #[test]
    fn repeat_rejects_results_over_string_memory_limit() {
        let result = super::repeat(
            Some(&Value::String("x".to_string())),
            &[Value::Number((super::MAX_STRING_BYTES as f64) + 1.0)],
        );
        assert!(result.is_err());
    }

    #[test]
    fn long_string_hash_is_stable_and_cached() {
        let units = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(hash_units(&units), hash_units(&units));
    }
    #[test]
    fn short_layout_is_derived_without_semantic_duplicate() {
        assert_eq!(
            short_string_layout(&Value::String("😀".repeat(11))),
            Some(ShortStringLayout::Utf8)
        );
        assert_eq!(short_string_layout(&Value::String("😀".repeat(12))), None);
        assert_eq!(
            short_string_layout(&Value::StringUnits(std::rc::Rc::new(
                crate::value::StringUnitsData::new(vec![0xd800; 22])
            ))),
            Some(ShortStringLayout::Utf16)
        );
        assert_eq!(
            short_string_layout(&Value::StringUnits(std::rc::Rc::new(
                crate::value::StringUnitsData::new(vec![0xd800; 23])
            ))),
            None
        );
    }

    #[test]
    fn latin1_classification_and_memory_boundary_are_exact() {
        let ascii = Value::String("A\u{ff}".to_string());
        let wide = Value::String("\u{100}".to_string());
        assert_eq!(
            super::encoding_of_value(&ascii),
            Some(StringEncoding::Latin1)
        );
        assert_eq!(super::encoding_of_value(&wide), Some(StringEncoding::Utf16));
        assert_eq!(super::compact_storage_bytes(&[0; 4]), 4);
        assert_eq!(super::compact_storage_bytes(&[0x100; 4]), 8);
        assert_eq!(super::encoding_of_value(&Value::Number(1.0)), None);
    }

    #[test]
    fn latin1_conversion_preserves_empty_and_0xff_boundaries() {
        for bytes in [vec![], vec![0], vec![0xff], vec![0, 0xff, 1]] {
            let units = super::latin1_to_units(&bytes);
            assert_eq!(super::units_to_latin1(&units), Some(bytes));
        }
        assert_eq!(super::units_to_latin1(&[0xff, 0x100]), None);
    }
    #[test]
    fn latin1_source_constructs_canonical_string_and_preserves_units() {
        let value = from_latin1(&[0x41, 0xff]);
        assert_eq!(value, Value::String("A\u{ff}".to_owned()));
        assert_eq!(units_of(&value), Some(vec![0x41, 0xff]));

        let empty = from_latin1(&[]);
        assert_eq!(empty, Value::String(String::new()));
        assert_eq!(units_of(&empty), Some(Vec::new()));
    }

    #[test]
    fn borrowed_view_shares_canonical_storage_without_materializing_units() {
        let value = Value::StringUnits(std::rc::Rc::new(crate::value::StringUnitsData::new(vec![
            0xd800, 0x0061,
        ])));
        let clone = value.clone();
        assert_eq!(
            std::rc::Rc::strong_count(match &value {
                Value::StringUnits(units) => units,
                _ => unreachable!(),
            }),
            2
        );
        assert_eq!(view_of(&value), Some(StringView::Utf16(&[0xd800, 0x0061])));
        assert_eq!(
            view_of(&Value::String("😀".into())),
            Some(StringView::Utf8("😀"))
        );
        assert_eq!(view_len_units(view_of(&value).unwrap()), 2);
        drop(clone);
        assert_eq!(
            std::rc::Rc::strong_count(match &value {
                Value::StringUnits(units) => units,
                _ => unreachable!(),
            }),
            1
        );
    }
    #[test]
    fn concat_flattens_canonical_views_without_replacing_surrogates() {
        let receiver = from_units(vec![0x41, 0xd800]);
        let result = concat(
            Some(&receiver),
            &[Value::String("😀".to_string()), Value::Number(7.0)],
        )
        .expect("concat");
        assert_eq!(
            units_of(&result),
            Some(vec![0x41, 0xd800, 0xd83d, 0xde00, 0x37])
        );
    }

    #[test]
    fn concat_empty_arguments_transfers_one_flat_source() {
        let receiver = Value::String("stable".to_string());
        let result = concat(Some(&receiver), &[]).expect("concat");
        assert_eq!(result, receiver);
    }
    #[test]
    fn source_encoding_is_the_canonical_storage_owner() {
        let utf8 = Value::String("hello".into());
        let lone_surrogate = from_units(vec![0xd800]);
        assert_eq!(source_encoding(&utf8), Some(StringSourceEncoding::Utf8));
        assert_eq!(
            source_encoding(&lone_surrogate),
            Some(StringSourceEncoding::Utf16)
        );
        assert_eq!(source_encoding(&Value::Number(1.0)), None);
    }

    #[test]
    fn concat_rejects_an_oversized_canonical_receiver_before_allocation() {
        let oversized = from_units(vec![0; MAX_STRING_BYTES / 2 + 1]);
        let result = concat(Some(&oversized), &[]);
        assert!(result.is_err());
    }

    #[test]
    fn discard_replace_executes_intrinsic_regexp_protocol() {
        let regexp = construct_value(
            &Value::Builtin(Builtin::RegExp),
            &[
                Value::String("^\\s*|\\s*$".into()),
                Value::String("g".into()),
            ],
        )
        .expect("regexp construction");
        super::replace_discard_string("  value  ", &regexp, &Value::String(String::new()))
            .expect("discarded replace");
    }
}
