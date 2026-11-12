//! The one dense view of an array's elements, and the searches over it.
//!
//! `array_read_view` proves once, per call, that every element index of an array is its own
//! data property held in the element vector, so the built-ins that only read elements walk the
//! borrowed slice instead of the per-element [[Get]]. A hole in the slice is the one place the
//! view does not answer by itself: the prototype chain decides it (`inherited_index_absent`).

use super::*;

/// How `Vm::search_dense` compares an element with the searched value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Match {
    /// `indexOf` and `lastIndexOf`: IsStrictlyEqual.
    Strict,
    /// `includes`: SameValueZero, and a hole no prototype holds reads as `undefined`.
    SameValueZero,
}

/// The outcome of `Vm::search_dense` over a range.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Search {
    Found(usize),
    Missing,
    /// The view cannot decide from this index on (no view, or a hole a prototype may hold):
    /// the generic algorithm continues here.
    Resume(usize),
}

impl<H: Host> Vm<H> {
    /// The element vector of an array whose indices are all own data properties of it: no
    /// indexed descriptors, sparse storage or arguments mapping. A slot holding the hole
    /// marker is a hole. Reading through the view runs no guest code.
    pub(super) fn array_read_view(&self, this: Value) -> Option<&[Value]> {
        let object = self.heap.array(this)?;
        (!object.has_indexed_descriptors() && !object.is_arguments_object() && self.heap.sparse_length(this).is_none())
            .then(|| &**object.elements())
    }

    /// Searches `range` of `array` for `search`, from its end when `reverse`. Nothing in the
    /// walk runs guest code: elements compare by strict equality or SameValueZero.
    pub(super) fn search_dense(
        &self,
        array: Value,
        mut range: std::ops::Range<usize>,
        reverse: bool,
        search: Value,
        mode: Match,
    ) -> Search {
        let resume = |range: &std::ops::Range<usize>| Search::Resume(if reverse { range.end - 1 } else { range.start });
        let Some(view) = self.array_read_view(array) else {
            return if range.is_empty() { Search::Missing } else { resume(&range) };
        };
        // Indices past the vector are the generic algorithm's.
        if range.end > view.len() && !range.is_empty() {
            return resume(&range);
        }
        while !range.is_empty() {
            let slice = &view[range.clone()];
            let Some(offset) = self.position_of(slice, reverse, search, mode) else {
                break;
            };
            let index = range.start + offset;
            if !slice[offset].is_deleted() {
                return Search::Found(index);
            }
            if !self.inherited_index_absent(array, index) {
                return Search::Resume(index);
            }
            if mode == Match::SameValueZero && search.is_undefined() {
                return Search::Found(index);
            }
            range = if reverse { range.start..index } else { index + 1..range.end };
        }
        Search::Missing
    }

    /// `Array.prototype.indexOf` or `includes` (`native`) of an array from an integer or absent
    /// `fromIndex` that the view decides alone; `None` leaves any other call to the generic
    /// algorithm.
    pub(super) fn dense_search_call(&self, native: Native, array: Value, args: &[Value]) -> Option<Value> {
        let length = self.array_read_view(array)?.len();
        let start = match args.get(1) {
            None => 0,
            Some(from) => crate::units::relative_index(f64::from(from.as_int()?), length),
        };
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        let includes = native == Native::ArrayIncludes;
        let mode = if includes { Match::SameValueZero } else { Match::Strict };
        Some(match self.search_dense(array, start..length, false, search, mode) {
            Search::Found(_) if includes => Value::TRUE,
            Search::Found(index) => Value::number(index as f64),
            Search::Missing if includes => Value::FALSE,
            Search::Missing => Value::number(-1.0),
            Search::Resume(_) => return None,
        })
    }

    /// The offset in `slice` of the first (last when `reverse`) element equal to `search` under
    /// `mode` or holding the hole marker. The searched value's kind picks the comparison once.
    fn position_of(&self, slice: &[Value], reverse: bool, search: Value, mode: Match) -> Option<usize> {
        fn find(slice: &[Value], reverse: bool, stop: impl Fn(&Value) -> bool) -> Option<usize> {
            let stop = |value: &Value| stop(value) || value.is_deleted();
            if reverse { slice.iter().rposition(stop) } else { slice.iter().position(stop) }
        }
        if let Some(number) = search.as_number() {
            return if number.is_nan() {
                // IsStrictlyEqual never holds for NaN; SameValueZero holds for it.
                find(slice, reverse, |value| mode == Match::SameValueZero && value.as_number().is_some_and(f64::is_nan))
            } else {
                find(slice, reverse, |value| value.as_number() == Some(number))
            };
        }
        if let Some(text) = self.heap.string(search) {
            return find(slice, reverse, |value| *value == search || self.heap.string(*value).is_some_and(|other| other == text));
        }
        if matches!(self.heap.get(search), Some(Cell::BigInt(_))) {
            return find(slice, reverse, |value| self.strict_equal(*value, search));
        }
        find(slice, reverse, |value| *value == search)
    }

    /// Appends to `output` the join pieces of the elements of `array` below `length`, with
    /// separators, for as long as the view decides each piece without guest code and as ASCII
    /// text: a String, a Number, a Boolean, `undefined` or `null`, or a hole no prototype holds.
    /// The pieces are written into one buffer and appended as one string. Returns the first
    /// index it did not append, where the generic algorithm continues.
    pub(super) fn join_dense_prefix(
        &self,
        array: Value,
        length: usize,
        separator: &JsString,
        output: &mut super::wtf16::JsStringBuilder,
    ) -> usize {
        use super::wtf16::Units;
        let (Some(view), Units::Ascii(separator)) = (self.array_read_view(array), separator.flat().view()) else {
            return 0;
        };
        let mut text = Vec::new();
        let mut stop = view.len().min(length);
        for (index, value) in view.iter().take(length).enumerate() {
            let mark = text.len();
            if index != 0 {
                text.extend_from_slice(separator);
            }
            if !self.extend_join_piece(array, index, *value, &mut text) {
                text.truncate(mark);
                stop = index;
                break;
            }
        }
        if !text.is_empty() {
            output.append(&JsString::from_ascii_parts(text.len(), std::iter::once(&text[..])));
        }
        stop
    }

    /// Writes the join piece of `value`, the element at `index` of `array`, onto `text` when
    /// it is ASCII and no guest code decides it; `false` leaves the element to the generic path.
    fn extend_join_piece(&self, array: Value, index: usize, value: Value, text: &mut Vec<u8>) -> bool {
        use super::wtf16::Units;
        if value.is_deleted() {
            return self.inherited_index_absent(array, index);
        }
        if value.is_null() || value.is_undefined() {
            return true;
        }
        if let Some(string) = self.heap.string(value) {
            return match string.flat().view() {
                Units::Ascii(bytes) => {
                    text.extend_from_slice(bytes);
                    true
                }
                Units::Wide(_) => false,
            };
        }
        match value.as_bool() {
            Some(flag) => text.extend_from_slice(if flag { b"true" } else { b"false" }),
            None => match value.as_number() {
                Some(number) => crate::number_to_string::write(number, text),
                None => return false,
            },
        }
        true
    }
}
