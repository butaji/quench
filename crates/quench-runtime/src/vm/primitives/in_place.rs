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
                    parts.push(self.heap.alloc_unit_run(Units::Ascii(&text[start..end])));
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
    /// `replace` or `replaceAll` of a flat ASCII receiver at a non-empty flat ASCII search
    /// string, by a flat ASCII replacement with no `$` (so no substitution pattern): no
    /// conversion runs guest code, so the result's bytes are written once, the unmatched runs
    /// copied around each replacement; no match answers the receiver itself. `None` leaves
    /// any other call to `call_primitive_native`.
    pub(super) fn replace_in_place(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if !matches!(native, Native::StringReplace | Native::StringReplaceAll) {
            return None;
        }
        let [search, replacement, ..] = args else {
            return None;
        };
        let ascii = |value: Value| match self.heap.string(value)?.existing_flat()?.view() {
            Units::Ascii(bytes) => Some(bytes),
            Units::Wide(_) => None,
        };
        let (text, needle, with) = (ascii(receiver)?, ascii(*search)?, ascii(*replacement)?);
        if needle.is_empty() || with.contains(&b'$') {
            return None;
        }
        let mut found = memchr::memmem::find_iter(text, needle);
        let Some(first) = found.next() else {
            return Some(receiver);
        };
        let mut bytes = smallvec::SmallVec::<[u8; 64]>::new();
        bytes.extend_from_slice(&text[..first]);
        bytes.extend_from_slice(with);
        let mut cursor = first + needle.len();
        if native == Native::StringReplaceAll {
            // Occurrences do not overlap: each search starts past the previous match.
            while let Some(at) = memchr::memmem::find(&text[cursor..], needle) {
                bytes.extend_from_slice(&text[cursor..cursor + at]);
                bytes.extend_from_slice(with);
                cursor += at + needle.len();
            }
        }
        bytes.extend_from_slice(&text[cursor..]);
        let result = crate::vm::wtf16::JsString::from_ascii_bytes(&bytes);
        Some(self.heap.alloc_string(result))
    }
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
