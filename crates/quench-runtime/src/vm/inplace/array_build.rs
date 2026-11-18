//! In-place fast paths for `Array.prototype.fill`, `Array.from`, `Array.of` and
//! `Array.prototype.join`: each completes the call with no guest code and no raised error, or
//! answers `None` so the general native runs.
use super::super::*;

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
    /// The text is gathered into one buffer and copied into one new string.
    pub(in crate::vm) fn plain_array_join(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let mut text = Vec::new();
        let separator: &[u8] = match args.first().copied() {
            Some(value) if !value.is_undefined() => {
                self.push_flat_ascii(value, &mut text)?;
                &text[..]
            }
            _ => &b","[..],
        };
        let bytes = self.join_dense_ascii(receiver, separator)?;
        Some(self.heap.alloc_ascii_run(&bytes))
    }

    /// The `join` text of the dense array `receiver` with `separator` between elements; `None`
    /// for a hole, a wide string, an object, or a result past the longest string.
    fn join_dense_ascii(&self, receiver: Value, separator: &[u8]) -> Option<Vec<u8>> {
        let elements = self.array_search_elements(receiver)?;
        let mut out = Vec::new();
        for (index, value) in elements.range(0..elements.len()).enumerate() {
            if index > 0 {
                out.extend_from_slice(separator);
            }
            if value.is_deleted() {
                return None;
            }
            if let Some(int) = value.as_int() {
                out.extend_from_slice(itoa::Buffer::new().format(int).as_bytes());
            } else if let Some(number) = value.as_number() {
                crate::number_to_string::write(number, &mut out);
            } else if !(value.is_undefined() || value.is_null()) {
                self.push_flat_ascii(value, &mut out)?;
            }
            if out.len() > super::super::wtf16::MAX_STRING_UNITS {
                return None;
            }
        }
        Some(out)
    }

    /// Appends the text of the flat ASCII string `value` to `out`, read from its record in place;
    /// `None` for a wide string or a value that is not a flat string.
    fn push_flat_ascii(&self, value: Value, out: &mut Vec<u8>) -> Option<()> {
        match self.heap.flat_text(value)? {
            crate::heap::FlatText::Record(super::super::wtf16::Units::Ascii(bytes)) => out.extend_from_slice(bytes),
            crate::heap::FlatText::Record(super::super::wtf16::Units::Wide(_)) => return None,
            crate::heap::FlatText::Small(text) => out.extend_from_slice(text.as_bytes()),
        }
        Some(())
    }
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
