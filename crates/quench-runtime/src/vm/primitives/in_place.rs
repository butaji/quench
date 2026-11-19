//! String methods answered from a flat receiver's units in place, without copying it.

use super::*;
use crate::vm::wtf16::Units;
use crate::vm::wtf16::find_ascii;
use crate::vm::wtf16::split::SplitParts;

impl<H: Host> Vm<H> {
    /// `indexOf`, `lastIndexOf`, `includes`, `startsWith` or `endsWith` of a flat string receiver
    /// searching for a flat string, from a position left out or passed as an integer: every
    /// conversion the method makes is then the value itself, so the search reads both strings'
    /// units in place, as `call_primitive_native` would after copying them. `None` leaves any
    /// other call to it.
    pub(super) fn string_search_in_place(&self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if !matches!(
            native,
            Native::StringIndexOf
                | Native::StringLastIndexOf
                | Native::StringIncludes
                | Native::StringStartsWith
                | Native::StringEndsWith
        ) {
            return None;
        }
        let [search, rest @ ..] = args else {
            return None;
        };
        let text = self.heap.flat_text(receiver)?;
        let needle = self.heap.flat_text(*search)?;
        let (text, needle) = (text.units(), needle.units());
        let number = search_argument(native, rest.first(), text.len())?;
        Some(search_result(native, text, needle, clamped_position(native, number, text.len())))
    }

    /// `slice`, `substring` or `substr` of a flat string receiver with integer or left-out
    /// bounds: every conversion is then the value itself, so the result is cut from the units
    /// in place rather than from a copy of the receiver. `None` leaves any other call to
    /// `call_primitive_native`.
    pub(super) fn substring_in_place(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if !matches!(native, Native::StringSlice | Native::StringSubstring | Native::StringSubstr) {
            return None;
        }
        let flat = self.heap.string(receiver)?.existing_flat()?;
        let length = flat.view().len() as i64;
        let range = in_place_range(native, length, args)?;
        let text = flat.substring(range);
        Some(self.heap.alloc_string(text))
    }
}

impl<H: Host> Vm<H> {
    /// `split` of a flat string receiver at a flat string separator, with the limit left
    /// out or an int32: no conversion runs guest code and nothing collects while the parts are
    /// made, so the receiver is cut in place without the general path's roots and copies. The
    /// parts are gathered once and the array is made from them in one step. `None` leaves any
    /// other call to `call_primitive_native`.
    pub(super) fn split_in_place(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if native != Native::StringSplit {
            return None;
        }
        let separator = args.first()?;
        let limit = split_limit(args.get(1))?;
        let input = self.heap.string(receiver)?;
        input.existing_flat()?;
        let input = input.clone();
        let separator = self.heap.string(*separator)?;
        separator.existing_flat()?;
        let separator = separator.clone();
        let mut parts = SplitParts::<Value>::new();
        match (input.units(), separator.units()) {
            // An ASCII text cut at one byte: a plain scan, each piece's bytes made a string
            // (a one-byte piece is that byte's string) without a units view per piece.
            (Units::Ascii(text), Units::Ascii(&[byte])) => self.push_byte_pieces(&mut parts, text, byte, limit),
            // An ASCII text cut into its units: each byte's string.
            (Units::Ascii(text), Units::Ascii(&[])) => self.push_unit_pieces(&mut parts, text, limit),
            _ => input.split_runs(separator.units(), limit, |run| parts.push(self.heap.alloc_unit_run(run))),
        }
        Some(self.heap.alloc_literal_array_from_parts(self.array_proto, &[parts.as_slice()]))
    }

    /// The pieces of the ASCII `text` cut at the byte `byte`, at most `limit` of them, each
    /// made into `parts` as its own string. The cuts are counted first so `parts` is sized
    /// once, not grown as it fills.
    fn push_byte_pieces(&mut self, parts: &mut SplitParts<Value>, text: &[u8], byte: u8, limit: usize) {
        let cuts = if text.len() <= SHORT_SPLIT_TEXT_BYTES {
            text.iter().filter(|&&unit| unit == byte).count()
        } else {
            memchr::memchr_iter(byte, text).count()
        };
        parts.reserve((cuts + 1).min(limit));
        if text.len() <= SHORT_SPLIT_TEXT_BYTES {
            let offsets = text.iter().enumerate().filter_map(|(offset, &unit)| (unit == byte).then_some(offset));
            push_pieces(&mut self.heap, parts, text, offsets, limit);
        } else {
            push_pieces(&mut self.heap, parts, text, memchr::memchr_iter(byte, text), limit);
        }
    }

    /// The first `limit` units of the ASCII `text`, each its own one-unit string, into `parts`.
    fn push_unit_pieces(&mut self, parts: &mut SplitParts<Value>, text: &[u8], limit: usize) {
        let count = text.len().min(limit);
        parts.reserve(count);
        parts.extend(text[..count].iter().map(|unit| self.heap.alloc_unit_run(Units::Ascii(std::slice::from_ref(unit)))));
    }
}

/// The text length up to which a split scans for its one-byte separator unit by unit, where
/// `memchr`'s setup costs more than the scan.
const SHORT_SPLIT_TEXT_BYTES: usize = 32;

/// The limit a `split` takes: left out or undefined is every piece (ToUint32 of `u32::MAX`), an
/// int32 is its ToUint32. `None` for any other value, which the general path converts.
fn split_limit(limit: Option<&Value>) -> Option<usize> {
    match limit {
        Some(value) if !value.is_undefined() => value.as_int().map(|int| int as u32 as usize),
        _ => Some(u32::MAX as usize),
    }
}

/// Makes each piece of the ASCII `text` between the one-byte separator's `offsets` as a string
/// into `parts`, taking at most `limit` pieces: the last piece runs to the end of the text.
fn push_pieces(heap: &mut crate::heap::Heap, parts: &mut SplitParts<Value>, text: &[u8], offsets: impl Iterator<Item = usize>, limit: usize) {
    let mut start = 0;
    for end in offsets.chain(std::iter::once(text.len())).take(limit) {
        parts.push(heap.alloc_ascii_run(&text[start..end]));
        start = end + 1;
    }
}

impl<H: Host> Vm<H> {
    /// The one body of `replace` and `replaceAll` at a string pattern: a flat string receiver,
    /// pattern and replacement (the replacement's `$` patterns are GetSubstitution with no
    /// captures, which `replacement_substitution` makes). No conversion here runs guest code, so
    /// the result is written once, the unmatched runs copied around each replacement. A
    /// replacement function, a non-string pattern or a rope leaves the call to the general path
    /// (`string_replace_native`), which answers `None` here.
    pub(super) fn replace_string_pattern(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        if !matches!(native, Native::StringReplace | Native::StringReplaceAll) {
            return None;
        }
        let [search, replacement, ..] = args else {
            return None;
        };
        let replace_all = native == Native::StringReplaceAll;
        if let Some(replaced) = self.ascii_replace(receiver, *search, *replacement, replace_all) {
            return Some(Ok(replaced));
        }
        let input = self.flat_string(receiver)?;
        let search = self.flat_string(*search)?;
        let template = self.flat_string(*replacement)?;
        Some(self.replace_units(p, receiver, &input, &search, &template, replace_all))
    }

    /// `replace` or `replaceAll` of a flat ASCII receiver at a flat ASCII pattern, with a flat
    /// ASCII replacement that holds no `$`: the matches are found in place, which gives the
    /// result's length, and the result is then written once from the operands read in place.
    /// The receiver itself when nothing matches. `None` leaves the call to the general path.
    fn ascii_replace(&mut self, receiver: Value, search: Value, replacement: Value, replace_all: bool) -> Option<Value> {
        let (offsets, length) = {
            let text = self.heap.flat_text(receiver)?;
            let needle = self.heap.flat_text(search)?;
            let with = self.heap.flat_text(replacement)?;
            let (Units::Ascii(text), Units::Ascii(needle), Units::Ascii(with)) = (text.units(), needle.units(), with.units())
            else {
                return None;
            };
            if needle.is_empty() || with.contains(&b'$') {
                return None;
            }
            let offsets = ascii_match_offsets(text, needle, replace_all);
            if offsets.is_empty() {
                return Some(receiver);
            }
            let length = replaced_len(text.len(), needle.len(), with.len(), offsets.len())?;
            (offsets, length)
        };
        self.heap.alloc_ascii_derived([receiver, search, replacement], length, |out, [text, needle, with]| {
            write_replaced(out, text, needle.len(), with, &offsets);
        })
    }

    /// A string value's flat units, read in place: a rope is left to the general path, which
    /// flattens it.
    fn flat_string(&self, value: Value) -> Option<JsString> {
        let string = self.heap.string(value)?;
        string.existing_flat()?;
        Some(string.clone())
    }


    /// The replacement text of one match at `index`: `template` itself, or its substitution
    /// (`$$`, `$&`, `` $` ``, `$'`) where it holds a `$`, appended to `output`.
    #[allow(clippy::too_many_arguments)]
    fn append_replacement(
        &mut self,
        p: &ResidualProgram,
        output: &mut crate::vm::wtf16::JsStringBuilder,
        substitutes: bool,
        template: &JsString,
        input: &JsString,
        index: usize,
        search: &JsString,
    ) -> Result<(), JsError> {
        if substitutes {
            let text = self.replacement_substitution(p, template, input, index, search, &[], Value::UNDEFINED)?;
            output.append(&text);
        } else {
            output.append(template);
        }
        Ok(())
    }

    /// `replace_string_pattern` of units of either width. Each match takes the replacement
    /// template, or its GetSubstitution when the template has a `$`; empty matches occur at
    /// each code-unit boundary, so a replace-all search resumes one unit on.
    pub(in crate::vm) fn replace_units(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &JsString,
        search: &JsString,
        template: &JsString,
        replace_all: bool,
    ) -> Result<Value, JsError> {
        let Some(first) = input.find_units(search.units(), 0) else {
            return Ok(receiver);
        };
        let units = template.units();
        let substitutes = (0..units.len()).any(|index| units.at(index) == u16::from(b'$'));
        let advance = search.units().len().max(1);
        let mut output = crate::vm::wtf16::JsStringBuilder::default();
        let mut cursor = 0;
        let mut position = Some(first);
        while let Some(index) = position {
            output.append_slice(input, cursor..index);
            self.append_replacement(p, &mut output, substitutes, template, input, index, search)?;
            cursor = index + search.units().len();
            let next = index + advance;
            position = if replace_all && next <= input.units().len() {
                input.find_units(search.units(), next)
            } else {
                None
            };
        }
        output.append_slice(input, cursor..input.units().len());
        let output = self.string_build_result(p, output.finish())?;
        Ok(self.heap.alloc_string(output))
    }
}

/// The inline capacity of a replace's match offsets: a typical search's matches stay off the heap.
const REPLACE_INLINE_MATCHES: usize = 8;
/// The byte offsets of the matches of a replace, in order.
type MatchOffsets = smallvec::SmallVec<[usize; REPLACE_INLINE_MATCHES]>;

/// The byte offsets of the occurrences of the non-empty `needle` in `text`, in order: only the
/// first when not `replace_all`. Occurrences do not overlap, so each search starts past the
/// previous match.
fn ascii_match_offsets(text: &[u8], needle: &[u8], replace_all: bool) -> MatchOffsets {
    let mut offsets = MatchOffsets::new();
    let mut cursor = 0;
    while let Some(at) = crate::vm::wtf16::find_ascii(&text[cursor..], needle) {
        offsets.push(cursor + at);
        cursor += at + needle.len();
        if !replace_all {
            break;
        }
    }
    offsets
}

/// The length of a replace's result: `text_len` less `count` matches of `needle_len` bytes,
/// plus `count` replacements of `with_len`; `None` past the longest string.
fn replaced_len(text_len: usize, needle_len: usize, with_len: usize, count: usize) -> Option<usize> {
    let kept = text_len.checked_sub(count.checked_mul(needle_len)?)?;
    let length = kept.checked_add(count.checked_mul(with_len)?)?;
    (length <= crate::vm::wtf16::MAX_STRING_UNITS).then_some(length)
}

/// Writes `text` into `out` with each match at `offsets` (of `needle_len` bytes) replaced by
/// `with`; `out` is exactly the length `replaced_len` gives.
fn write_replaced(out: &mut [u8], text: &[u8], needle_len: usize, with: &[u8], offsets: &[usize]) {
    let mut at = 0;
    let mut cursor = 0;
    for &index in offsets {
        put(out, &mut at, &text[cursor..index]);
        put(out, &mut at, with);
        cursor = index + needle_len;
    }
    put(out, &mut at, &text[cursor..]);
}

/// Writes `bytes` into `out` at `*at`, then advances `*at` past them.
fn put(out: &mut [u8], at: &mut usize, bytes: &[u8]) {
    out[*at..*at + bytes.len()].copy_from_slice(bytes);
    *at += bytes.len();
}

/// The units range `slice` (negative bounds count from the end) or `substring` (bounds in
/// either order) takes from a string of `length` units, given its integer bounds.
pub(super) fn substring_range(native: Native, length: i64, start: i64, end: i64) -> std::ops::Range<usize> {
    let (start, end) = if native == Native::StringSlice {
        let from_end = |bound: i64| if bound < 0 { (length + bound).max(0) } else { bound.min(length) };
        (from_end(start), from_end(end))
    } else {
        let (start, end) = (start.clamp(0, length), end.clamp(0, length));
        (start.min(end), start.max(end))
    };
    start as usize..end.max(start) as usize
}

/// The count `substr` takes when it is left out: every unit from the start. `i64::MAX` reaches
/// the end from any start, so `substr_range` needs no length for it.
pub(super) const SUBSTR_REST: i64 = i64::MAX;

/// The units range `substr(start, count)` takes from a string of `length` units, given its
/// integer arguments: a start below zero counts from the end, and the count is clamped to the
/// units from the start to the end.
pub(super) fn substr_range(length: i64, start: i64, count: i64) -> std::ops::Range<usize> {
    let start = if start < 0 { (length + start).max(0) } else { start.min(length) };
    let end = start.saturating_add(count.max(0)).min(length);
    start as usize..end as usize
}

/// The units range a `slice`, `substring` or `substr` cuts from a flat string of `length`
/// units for its integer arguments, where a left-out or undefined argument takes the method's
/// default. `None` for any other argument, which the general path converts.
fn in_place_range(native: Native, length: i64, args: &[Value]) -> Option<std::ops::Range<usize>> {
    let bound = |index: usize, default: i64| match args.get(index) {
        Some(value) if !value.is_undefined() => value.as_int().map(i64::from),
        _ => Some(default),
    };
    if native == Native::StringSubstr {
        return Some(substr_range(length, bound(0, 0)?, bound(1, SUBSTR_REST)?));
    }
    Some(substring_range(native, length, bound(0, 0)?, bound(1, length)?))
}

/// The position a search method takes from a left-out or undefined argument, as a number: the
/// end of the text for `endsWith` and `lastIndexOf`, the start for the others.
pub(super) fn search_default(native: Native, length: usize) -> f64 {
    if matches!(native, Native::StringEndsWith | Native::StringLastIndexOf) {
        length as f64
    } else {
        0.0
    }
}

/// A search's position argument as the number the in-place body searches from: an integer
/// argument as its value, a left-out or undefined one as `search_default`. `None` for any other
/// value, which the general path converts.
fn search_argument(native: Native, argument: Option<&Value>, length: usize) -> Option<f64> {
    match argument {
        Some(value) if !value.is_undefined() => value.as_int().map(f64::from),
        _ => Some(search_default(native, length)),
    }
}

/// The position a search method searches from, given its position argument as a number and the
/// text's `length`: ToIntegerOrInfinity clamped to `0..=length`. NaN is the start, except for
/// `lastIndexOf`, where it is the end, as +Infinity is.
pub(super) fn clamped_position(native: Native, number: f64, length: usize) -> usize {
    if number.is_nan() {
        return if native == Native::StringLastIndexOf { length } else { 0 };
    }
    number.trunc().max(0.0).min(length as f64) as usize
}

/// The result of a string search method at `position` (already clamped by `clamped_position`)
/// of `needle` in `text`: the index it finds, or the boolean it answers.
pub(super) fn search_result(native: Native, text: Units<'_>, needle: Units<'_>, position: usize) -> Value {
    if let (Units::Ascii(text), Units::Ascii(needle)) = (text, needle) {
        return ascii_search_result(native, text, needle, position);
    }
    match native {
        Native::StringIndexOf => index_value(text.find(needle, position)),
        Native::StringLastIndexOf => index_value(text.rfind(needle, position)),
        Native::StringIncludes => Value::boolean(text.find(needle, position).is_some()),
        Native::StringStartsWith => Value::boolean(text.slice(position..text.len()).starts_with(needle)),
        _ => Value::boolean(
            position
                .checked_sub(needle.len())
                .is_some_and(|start| text.slice(start..position) == needle),
        ),
    }
}

/// `search_result` of ASCII text and needle, on the bytes: `find_ascii` finds a one-byte
/// needle with `memchr`, and the affixes compare slices. `position` is within the text.
fn ascii_search_result(native: Native, text: &[u8], needle: &[u8], position: usize) -> Value {
    match native {
        Native::StringIndexOf => index_value(find_ascii(&text[position..], needle).map(|at| position + at)),
        Native::StringLastIndexOf => index_value(ascii_rfind(text, needle, position)),
        Native::StringIncludes => Value::boolean(find_ascii(&text[position..], needle).is_some()),
        Native::StringStartsWith => Value::boolean(text[position..].starts_with(needle)),
        _ => Value::boolean(position.checked_sub(needle.len()).is_some_and(|start| text[start..position] == *needle)),
    }
}

/// The last offset at or before `position` where the ASCII `needle` occurs in `text`, as
/// `lastIndexOf` searches: `position` is first clamped to the last offset a match can start at.
/// A one-byte needle is found by `memrchr`, a longer one by `memmem`'s reverse search.
fn ascii_rfind(text: &[u8], needle: &[u8], position: usize) -> Option<usize> {
    let last = text.len().checked_sub(needle.len())?;
    let from = position.min(last);
    match needle {
        [] => Some(from),
        [byte] => memchr::memrchr(*byte, &text[..=from]),
        _ => memchr::memmem::rfind(&text[..from + needle.len()], needle),
    }
}

/// A search index as a number value: the index found, or -1 for none.
fn index_value(found: Option<usize>) -> Value {
    Value::number(found.map_or(-1.0, |index| index as f64))
}
