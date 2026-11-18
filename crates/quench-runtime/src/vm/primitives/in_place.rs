//! String methods answered from a flat receiver's units in place, without copying it.

use super::*;
use crate::vm::wtf16::Units;

impl<H: Host> Vm<H> {
    /// `indexOf`, `includes` or `startsWith` of a flat string receiver searching for a flat
    /// string, from a position left out or passed as an integer: every conversion the
    /// method makes is then the value itself, so the search reads the units in place, as
    /// `call_primitive_native` would after copying them. `None` leaves any other call to it.
    pub(super) fn string_search_in_place(&self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if !matches!(native, Native::StringIndexOf | Native::StringIncludes | Native::StringStartsWith) {
            return None;
        }
        let [search, rest @ ..] = args else {
            return None;
        };
        let text = self.heap.string(receiver)?.existing_flat()?.view();
        let needle = self.heap.string(*search)?.existing_flat()?.view();
        let length = text.len();
        // A position below zero reads as zero and one past the end as the end, for all three.
        let position = match rest.first() {
            None => 0,
            Some(position) if position.is_undefined() => 0,
            Some(position) => usize::try_from(position.as_int()?).unwrap_or(0).min(length),
        };
        Some(match native {
            Native::StringIndexOf => Value::number(text.find(needle, position).map_or(-1.0, |index| index as f64)),
            Native::StringIncludes => Value::boolean(text.find(needle, position).is_some()),
            _ => Value::boolean(text.slice(position..length).starts_with(needle)),
        })
    }

    /// `slice` or `substring` of a flat string receiver with integer or left-out bounds:
    /// every conversion is then the value itself, so the result is cut from the units in
    /// place rather than from a copy of the receiver. `None` leaves any other call to
    /// `call_primitive_native`.
    pub(super) fn substring_in_place(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if !matches!(native, Native::StringSlice | Native::StringSubstring | Native::StringSubstr) {
            return None;
        }
        let flat = self.heap.string(receiver)?.existing_flat()?;
        let length = flat.view().len() as i64;
        let bound = |index: usize, default: i64| match args.get(index) {
            None => Some(default),
            Some(value) if value.is_undefined() => Some(default),
            Some(value) => value.as_int().map(i64::from),
        };
        let range = if native == Native::StringSubstr {
            // `substr(start, length)`: a start below zero counts from the end; the count
            // defaults to the rest.
            let start = bound(0, 0)?;
            let start = if start < 0 { (length + start).max(0) } else { start.min(length) };
            let end = start.saturating_add(bound(1, length - start)?.max(0)).min(length);
            start as usize..end as usize
        } else {
            substring_range(native, length, bound(0, 0)?, bound(1, length)?)
        };
        let text = flat.substring(range);
        Some(self.heap.alloc_string(text))
    }
}

impl<H: Host> Vm<H> {
    /// `split` of a flat string receiver at a flat string separator, with the limit left
    /// out: no conversion runs guest code and nothing collects while the parts are made, so
    /// the receiver is cut in place without the general path's roots and copies. `None`
    /// leaves any other call to `call_primitive_native`.
    pub(super) fn split_in_place(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if native != Native::StringSplit {
            return None;
        }
        let (Some(separator), None) = (args.first(), args.get(1).filter(|limit| !limit.is_undefined())) else {
            return None;
        };
        let input = self.heap.string(receiver)?;
        input.existing_flat()?;
        let input = input.clone();
        let separator = self.heap.string(*separator)?;
        separator.existing_flat()?;
        let separator = separator.clone();
        let mut parts = crate::vm::wtf16::split::SplitParts::<Value>::new();
        match (input.units(), separator.units()) {
            // An ASCII text cut at one byte: a plain scan, each piece's bytes made a string
            // (a one-byte piece is that byte's string) without a units view per piece.
            (Units::Ascii(text), Units::Ascii(&[byte])) => {
                let mut start = 0;
                let mut cut = |end: usize, parts: &mut crate::vm::wtf16::split::SplitParts<Value>| {
                    parts.push(self.heap.alloc_ascii_run(&text[start..end]));
                    start = end + 1;
                };
                if text.len() <= 32 {
                    for (offset, &unit) in text.iter().enumerate() {
                        if unit == byte {
                            cut(offset, &mut parts);
                        }
                    }
                } else {
                    for offset in memchr::memchr_iter(byte, text) {
                        cut(offset, &mut parts);
                    }
                }
                cut(text.len(), &mut parts);
            }
            // An ASCII text cut into its units: each byte's string.
            (Units::Ascii(text), Units::Ascii(&[])) => {
                parts.extend(text.iter().map(|unit| self.heap.alloc_unit_run(Units::Ascii(std::slice::from_ref(unit)))));
            }
            _ => input.split_runs(separator.units(), u32::MAX as usize, |run| parts.push(self.heap.alloc_unit_run(run))),
        }
        let values = crate::heap::ArrayElements::from_slice(&parts);
        Some(self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(self.array_proto), values),
        }))
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
