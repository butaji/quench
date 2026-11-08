//! String methods answered from a flat receiver's units in place, without copying it.

use super::*;

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
        if !matches!(native, Native::StringSlice | Native::StringSubstring) {
            return None;
        }
        let flat = self.heap.string(receiver)?.existing_flat()?;
        let length = flat.view().len() as i64;
        let bound = |index: usize, default: i64| match args.get(index) {
            None => Some(default),
            Some(value) if value.is_undefined() => Some(default),
            Some(value) => value.as_int().map(i64::from),
        };
        let range = substring_range(native, length, bound(0, 0)?, bound(1, length)?);
        let text = flat.substring(range);
        Some(self.heap.alloc_string(text))
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
