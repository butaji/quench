//! In-place fast paths for `Array.prototype.fill`, `Array.from`, `Array.of` and
//! `Array.prototype.join`: each completes the call with no guest code and no raised error, or
//! answers `None` so the general native runs.
use super::super::*;
use super::ascii_text::{ascii_bytes, owned_flat_ascii};
use crate::vm::wtf16::{JsString, MAX_STRING_UNITS, Units};

/// The ASCII byte that separates `join`ed elements when none is given.
const DEFAULT_JOIN_SEPARATOR: &[u8] = b",";

impl<H: Host> Vm<H> {
    /// `Array.prototype.fill` in place: a dense array of this realm where every Set is the
    /// plain element store (`fill_plain_array_elements`), so holes, as in `new Array(n)`, are
    /// defined in place too. The range's bounds are relative indices that are absent,
    /// undefined or int32, clamped as `fill` clamps them; the receiver is returned.
    pub(in crate::vm) fn plain_array_fill(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let length = self.dense_array_length(receiver)?;
        let object = self.heap.array(receiver)?;
        if object.proto() != self.array_proto || object.is_frozen() {
            return None;
        }
        let mut operands = args.iter().copied();
        let value = operands.next().unwrap_or(Value::UNDEFINED);
        let start = fill_bound(operands.next(), 0, length)?;
        let end = fill_bound(operands.next(), length, length)?.max(start);
        self.fill_plain_array_elements(receiver, start..end, value).then_some(receiver)
    }

    /// `Array.from(items)` with no mapping function, where `this` is this realm's `Array` and
    /// `items` is a dense array whose iteration is the builtin one (`array_iteration_intact`).
    /// Its iterator yields the elements in order, so the result is one copy of them.
    pub(in crate::vm) fn plain_array_from(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let [items] = args else {
            return None;
        };
        if !self.realm_array_constructor(receiver) || !self.array_iteration_intact(*items) {
            return None;
        }
        let length = self.own_array_length(*items)?;
        self.array_of_dense_range(*items, 0..length)
    }

    /// `Array.of(...items)` where `this` is this realm's `Array`: a new array of `items`.
    pub(in crate::vm) fn plain_array_of(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        if !self.realm_array_constructor(receiver) {
            return None;
        }
        Some(self.array_of(args))
    }

    /// `Array.prototype.join` of a dense array whose elements are present and are flat ASCII
    /// strings, numbers, `undefined` or `null` (empty), with an absent or flat ASCII separator.
    /// The result is one string: its length is counted over the elements, then its bytes are
    /// written into it.
    pub(in crate::vm) fn plain_array_join(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let separator = match args.first().copied() {
            Some(value) if !value.is_undefined() => owned_flat_ascii(&self.heap, value)?,
            _ => JsString::short_ascii(DEFAULT_JOIN_SEPARATOR),
        };
        let string = self.join_dense_ascii(receiver, &separator)?;
        Some(self.heap.alloc_string(string))
    }

    /// The `join` text of the dense array `receiver` with `separator` between elements, as one
    /// string; `None` for a hole, a wide string, an object, or a result past the longest string.
    /// The elements are read from the heap, which is not written while the text is made.
    fn join_dense_ascii(&self, receiver: Value, separator: &JsString) -> Option<JsString> {
        let elements = self.array_search_elements(receiver)?;
        let separator = ascii_bytes(separator)?;
        let length = self.join_length(elements, separator.len())?;
        Some(JsString::from_ascii_zeroed_with(length, |out| self.write_join(elements, separator, out)))
    }

    /// The byte length of the `join` text of `elements` with `separator_len` bytes between them;
    /// `None` for a hole, a non-ASCII element, or a length past the longest string.
    fn join_length(&self, elements: &crate::heap::ArrayElements, separator_len: usize) -> Option<usize> {
        let mut length = 0;
        for (index, value) in elements.range(0..elements.len()).enumerate() {
            if value.is_deleted() {
                return None;
            }
            if index > 0 {
                length += separator_len;
            }
            length += self.join_piece_len(value)?;
            if length > MAX_STRING_UNITS {
                return None;
            }
        }
        Some(length)
    }

    /// The byte length of `value` as a `join` element: a number's text, a flat ASCII string's
    /// bytes, nothing for `undefined` or `null`; `None` for any other value.
    fn join_piece_len(&self, value: Value) -> Option<usize> {
        if let Some(int) = value.as_int() {
            return Some(itoa::Buffer::new().format(int).len());
        }
        if let Some(number) = value.as_number() {
            let mut text = crate::number_to_string::Short::new();
            crate::number_to_string::write(number, &mut text);
            return Some(text.len());
        }
        if value.is_undefined() || value.is_null() {
            return Some(0);
        }
        match self.heap.flat_text(value)?.units() {
            Units::Ascii(bytes) => Some(bytes.len()),
            Units::Wide(_) => None,
        }
    }

    /// Writes the `join` text of `elements` with `separator` between them into `out`, which
    /// `join_length` sized over the same elements.
    fn write_join(&self, elements: &crate::heap::ArrayElements, separator: &[u8], out: &mut [u8]) {
        let mut at = 0;
        for (index, value) in elements.range(0..elements.len()).enumerate() {
            if index > 0 {
                at = put(out, at, separator);
            }
            at = self.write_join_piece(value, out, at);
        }
        assert_eq!(at, out.len(), "the elements fill the length `join_length` counted");
    }

    /// Writes the `join` text of one element at `at` in `out`, returning the offset past it.
    fn write_join_piece(&self, value: Value, out: &mut [u8], at: usize) -> usize {
        if let Some(int) = value.as_int() {
            return put(out, at, itoa::Buffer::new().format(int).as_bytes());
        }
        if let Some(number) = value.as_number() {
            let mut text = crate::number_to_string::Short::new();
            crate::number_to_string::write(number, &mut text);
            return put(out, at, &text);
        }
        let Some(text) = self.heap.flat_text(value) else {
            return at;
        };
        match text.units() {
            Units::Ascii(bytes) => put(out, at, bytes),
            Units::Wide(_) => at,
        }
    }
}

/// Copies `bytes` into `out` at `at`, returning the offset past them.
fn put(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    out[at..at + bytes.len()].copy_from_slice(bytes);
    at + bytes.len()
}

/// A `fill` bound: `default` for an absent or undefined argument, an int32 relative to `length`
/// (negative counts from the end, clamped to `[0, length]`); `None` for any other value, whose
/// ToIntegerOrInfinity could run guest code.
#[inline(always)]
fn fill_bound(value: Option<Value>, default: usize, length: usize) -> Option<usize> {
    let Some(value) = value.filter(|value| !value.is_undefined()) else {
        return Some(default);
    };
    let index = value.as_int()?;
    Some(if index < 0 {
        length.saturating_sub(index.unsigned_abs() as usize)
    } else {
        (index as usize).min(length)
    })
}
