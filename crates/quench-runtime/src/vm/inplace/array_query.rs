//! In-place fast paths for `Array.prototype.at`, `lastIndexOf`, `reverse`, `toReversed` and
//! `with`: each completes the call with no guest code and no raised error, or answers `None`
//! so the general native runs.
use super::super::*;
use super::super::array_view::{Match, Search};

/// The `indexOf`-style result for "not found".
const NOT_FOUND: i32 = -1;

/// Inline capacity of the staging buffer a copied element list is built in.
const INLINE_VALUES: usize = 16;

impl Vm {
    /// `Array.prototype.at(index)` on a dense array with an int32 `index`. A negative index counts
    /// from the end. A present element is returned; a hole, or an index past either end, is `None`
    /// except past the end of an array of `Array.prototype` whose chain holds no index, which reads
    /// `undefined`.
    pub(in crate::vm) fn plain_array_at(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let index = args.first()?.as_int()?;
        let length = self.array_search_elements(receiver)?.len();
        let Some(position) = at_position(index, length) else {
            return self.undefined_past_end(receiver);
        };
        let value = self.array_search_elements(receiver)?.get(position)?;
        (!value.is_deleted()).then_some(value)
    }

    /// The `undefined` a read past the end of `receiver` gives when `receiver` is an array of this
    /// realm's `Array.prototype` and no prototype on that chain holds an index; `None` otherwise.
    fn undefined_past_end(&self, receiver: Value) -> Option<Value> {
        let of_array_proto = self.heap.array(receiver)?.proto() == self.array_proto;
        (of_array_proto && self.chain_holds_no_index(self.array_proto)).then_some(Value::UNDEFINED)
    }

    /// `Array.prototype.lastIndexOf(searchElement, fromIndex)` over a dense array, searched with
    /// strict equality as `indexOf` is. The start is the last index when `fromIndex` is absent,
    /// else an int32 as the spec clamps it. A hole the search reaches is left to the general
    /// algorithm (`None`).
    pub(in crate::vm) fn plain_array_last_index_of(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        let length = self.array_search_elements(receiver)?.len();
        let Ok(start) = usize::try_from(last_index_start(args.get(1).copied(), length)?) else {
            return Some(Value::integer(NOT_FOUND));
        };
        match self.search_dense(receiver, 0..start + 1, true, search, Match::Strict) {
            Search::Found(index) => Some(Value::length(index)),
            Search::Missing => Some(Value::integer(NOT_FOUND)),
            Search::Resume(_) => None,
        }
    }

    /// `Array.prototype.reverse()` in place: an array whose elements are ordinary and all present
    /// (`holds_ordinary_elements`, no hole). Every HasProperty, Get and Set reads or writes an own
    /// data element and observes nothing, so the backing is reversed and the receiver returned.
    /// Moving values within one array stores no new reference.
    pub(in crate::vm) fn plain_array_reverse(&mut self, receiver: Value, _args: &[Value]) -> Option<Value> {
        if !self.heap.array(receiver)?.holds_ordinary_elements() {
            return None;
        }
        if self.array_search_elements(receiver)?.iter().any(|value| value.is_deleted()) {
            return None;
        }
        let array = self.heap.array_store_mut(receiver, Value::UNDEFINED)?;
        super::super::index::mutable_array_elements(array.elements_mut()).reverse();
        Some(receiver)
    }

    /// `Array.prototype.toReversed()` of a dense array with no hole: a new array of its elements in
    /// reverse order, copied from the backing as `array_of` copies a slice.
    pub(in crate::vm) fn plain_array_to_reversed(&mut self, receiver: Value, _args: &[Value]) -> Option<Value> {
        let elements = self.array_search_elements(receiver)?;
        if elements.iter().any(|value| value.is_deleted()) {
            return None;
        }
        let mut values = smallvec::SmallVec::<[Value; INLINE_VALUES]>::new();
        values.extend(elements.iter().rev());
        Some(self.array_of(&values))
    }

    /// `Array.prototype.with(index, value)` of a dense array with no hole and an int32 `index` in
    /// range: a new array of the elements with `value` at the relative index. An index past either
    /// end, which throws, is `None` for the general native.
    pub(in crate::vm) fn plain_array_with(&mut self, receiver: Value, args: &[Value]) -> Option<Value> {
        let index = args.first()?.as_int()?;
        let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let elements = self.array_search_elements(receiver)?;
        let position = at_position(index, elements.len())?;
        if elements.iter().any(|element| element.is_deleted()) {
            return None;
        }
        let mut values = smallvec::SmallVec::<[Value; INLINE_VALUES]>::new();
        values.extend(elements.iter());
        values[position] = value;
        Some(self.array_of(&values))
    }
}

/// The element index `at(index)` reads in an array of `length` for an int32 `index`: a negative
/// index counts from the end; `None` past either end.
#[inline(always)]
fn at_position(index: i32, length: usize) -> Option<usize> {
    let relative = if index < 0 { length as i64 + i64::from(index) } else { i64::from(index) };
    usize::try_from(relative).ok().filter(|position| *position < length)
}

/// The index a `lastIndexOf` search starts from in an array of `length`: the last index when
/// `fromIndex` is absent, else the int32 `fromIndex` clamped as the spec clamps it (a negative one
/// counts from the end). Negative when nothing is searched; `None` for any other `fromIndex`,
/// whose ToIntegerOrInfinity could run guest code.
#[inline(always)]
fn last_index_start(from: Option<Value>, length: usize) -> Option<isize> {
    let last = length as isize - 1;
    let Some(from) = from else {
        return Some(last);
    };
    let from = from.as_int()? as isize;
    Some(if from >= 0 { from.min(last) } else { length as isize + from })
}
