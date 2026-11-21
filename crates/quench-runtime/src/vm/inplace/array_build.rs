//! In-place fast paths for `Array.prototype.fill`, `Array.from`, `Array.of` and
//! `Array.prototype.join`: each completes the call with no guest code and no raised error, or
//! answers `None` so the general native runs.
use super::super::*;
use super::ascii_text::{ascii_bytes, owned_flat_ascii};
use crate::vm::wtf16::{JsString, MAX_STRING_UNITS, Units};
use std::mem::MaybeUninit;

/// The ASCII byte that separates `join`ed elements when none is given.
const DEFAULT_JOIN_SEPARATOR: &[u8] = b",";

impl Vm {
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

    /// `Array.prototype.join` of a dense array whose elements are flat ASCII strings, numbers,
    /// booleans, `undefined`, `null` or holes no prototype holds, with an absent or flat ASCII
    /// separator. The result is one string when the prefix covers every element; otherwise `None`,
    /// and the general native continues from the first index the prefix did not cover.
    pub(in crate::vm) fn plain_array_join(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let separator = match args.first().copied() {
            Some(value) if !value.is_undefined() => owned_flat_ascii(&self.heap, value)?,
            _ => JsString::short_ascii(DEFAULT_JOIN_SEPARATOR),
        };
        let length = self.array_search_elements(receiver)?.len();
        let (text, stop) = self.plain_join_prefix(receiver, length, &separator);
        (stop == length).then(|| self.heap.alloc_string(text))
    }

    /// The join text of the elements of `array` below `length`, with `separator` between them, for
    /// as long as each element is decided without guest code and as ASCII text: a String, a Number,
    /// a Boolean, `undefined`, `null`, or a hole no prototype holds. Returns that text and the first
    /// index it does not cover, where the general algorithm continues (`length` when it covers all).
    /// The elements are read from the heap, which is not written while the text is made.
    pub(in crate::vm) fn plain_join_prefix(&self, array: Value, length: usize, separator: &JsString) -> (JsString, usize) {
        let (Some(elements), Some(separator)) = (self.array_search_elements(array), ascii_bytes(separator)) else {
            return (JsString::short_ascii(b""), 0);
        };
        let covered = length.min(elements.len());
        let (stop, text_len) = self.join_prefix_extent(array, elements, covered, separator.len());
        let text = JsString::from_ascii_filled(text_len, |out| {
            self.write_join_prefix(array, elements, stop, separator, out);
        });
        (text, stop)
    }

    /// The byte length of the join text of the first `covered` elements of `array` with
    /// `separator_len` bytes between them, and the index where that text stops: the first element
    /// not decided as text, or whose text would pass `MAX_STRING_UNITS` (the general algorithm then
    /// raises the error).
    fn join_prefix_extent(&self, array: Value, elements: &crate::heap::ArrayElements, covered: usize, separator_len: usize) -> (usize, usize) {
        let mut length = 0;
        for (index, value) in elements.range(0..covered).enumerate() {
            let separator = if index > 0 { separator_len } else { 0 };
            let Some(piece) = self.join_piece(array, index, value, |bytes| bytes.len()) else {
                return (index, length);
            };
            if length + separator + piece > MAX_STRING_UNITS {
                return (index, length);
            }
            length += separator + piece;
        }
        (covered, length)
    }

    /// Writes the join text of the first `stop` elements of `array` with `separator` between them
    /// into `out`, which `join_prefix_extent` sized over the same elements. Every byte of `out` is
    /// written, as the final assertion checks.
    fn write_join_prefix(&self, array: Value, elements: &crate::heap::ArrayElements, stop: usize, separator: &[u8], out: &mut [MaybeUninit<u8>]) {
        let mut at = 0;
        for (index, value) in elements.range(0..stop).enumerate() {
            if index > 0 {
                at = put(out, at, separator);
            }
            let next = self.join_piece(array, index, value, |bytes| put(out, at, bytes));
            at = next.unwrap_or(at);
        }
        assert_eq!(at, out.len(), "the elements fill the length `join_prefix_extent` counted");
    }

    /// Applies `f` to the bytes of the join piece of element `index` of `array` (`value`, its slot):
    /// a hole no prototype holds is empty. `None` for an element whose text needs guest code or is
    /// not ASCII.
    fn join_piece<R>(&self, array: Value, index: usize, value: Value, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        if value.is_deleted() {
            return self.inherited_index_absent(array, index).then(|| f(&[]));
        }
        if value.is_undefined() || value.is_null() {
            return Some(f(&[]));
        }
        if let Some(int) = value.as_int() {
            return Some(f(itoa::Buffer::new().format(int).as_bytes()));
        }
        if let Some(number) = value.as_number() {
            let mut text = crate::number_to_string::Short::new();
            crate::number_to_string::write(number, &mut text);
            return Some(f(&text));
        }
        if let Some(flag) = value.as_bool() {
            let word: &[u8] = if flag { b"true" } else { b"false" };
            return Some(f(word));
        }
        self.with_join_text(value, f)
    }

    /// Applies `f` to the bytes of the flat ASCII string `value`, read in place (a rope is flattened
    /// as the general join reads it); `None` for a wide string or a value that is not a string.
    fn with_join_text<R>(&self, value: Value, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let existing = self.heap.flat_text(value);
        let units = match &existing {
            Some(text) => text.units(),
            None => self.heap.string(value)?.flat().view(),
        };
        match units {
            Units::Ascii(bytes) => Some(f(bytes)),
            Units::Latin1(_) | Units::Wide(_) => None,
        }
    }
}

/// Copies `bytes` into `out` at `at`, returning the offset past them; a single byte is one store.
#[inline(always)]
fn put(out: &mut [MaybeUninit<u8>], at: usize, bytes: &[u8]) -> usize {
    if let [byte] = bytes {
        out[at].write(*byte);
        return at + 1;
    }
    let target = &mut out[at..at + bytes.len()];
    // SAFETY: `target` has exactly `bytes.len()` slots, which `bytes` cannot overlap (it is a
    // borrow of the heap or of a formatting buffer, never of `out`); `u8` and `MaybeUninit<u8>`
    // share a layout, so the copy initializes the slots.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.as_mut_ptr().cast::<u8>(), bytes.len()) };
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
