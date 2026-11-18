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
    /// The element backing of an array whose indices are all own data properties of it: no
    /// indexed descriptors, sparse storage or arguments mapping. A slot holding the hole
    /// marker is a hole. Reading it runs no guest code; a search reads it in place.
    pub(super) fn array_search_elements(&self, this: Value) -> Option<&crate::heap::ArrayElements> {
        let object = self.heap.array(this)?;
        (!object.has_indexed_descriptors() && !object.is_arguments_object() && self.heap.sparse_length(this).is_none())
            .then(|| object.elements())
    }

    /// `array_search_elements` as one slice of values, decoded into a new vector where the
    /// backing is narrow or double.
    pub(super) fn array_read_view(&self, this: Value) -> Option<std::borrow::Cow<'_, [Value]>> {
        self.array_search_elements(this).map(crate::heap::ArrayElements::values_cow)
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
        // The backing itself, decoded element by element where it is narrow or double: a
        // search reads each element once and copies nothing (`array_read_view` would decode an
        // encoded backing into a new vector first).
        let Some(elements) = self.array_search_elements(array) else {
            return if range.is_empty() { Search::Missing } else { resume(&range) };
        };
        // Indices past the vector are the generic algorithm's.
        if range.end > elements.len() && !range.is_empty() {
            return resume(&range);
        }
        // A narrow backing searched for a value whose equal values are its own word alone (an
        // int32, or a constant or object reference): its words compare as they are, a hole by
        // the hole marker's word. A narrow backing holds no double, so no other word is equal.
        let narrow = elements.narrow_words().zip(self.word_identity_search(search).and_then(Value::to_narrow));
        let hole = Value::DELETED.to_narrow();
        while !range.is_empty() {
            let found = match narrow {
                Some((words, key)) => {
                    let stop = |word: &u32| *word == key || Some(*word) == hole;
                    let mut words = words[range.clone()].iter();
                    if reverse { words.rposition(stop) } else { words.position(stop) }
                }
                None => match elements.wide_values() {
                    // An int32 search of a wide backing: the same word, the hole's, or a double
                    // word of the same value, tested as words in one pass.
                    Some(values) if let Some(int) = search.as_int() && int != 0 => {
                        let number = f64::from(int);
                        let stop = |value: &Value| *value == search || value.is_deleted() || value.is_double() && value.as_number() == Some(number);
                        let mut values = values[range.clone()].iter();
                        if reverse { values.rposition(stop) } else { values.position(stop) }
                    }
                    Some(values) => self.position_of(values[range.clone()].iter().copied(), reverse, search, mode),
                    None => self.position_of(elements.range(range.clone()), reverse, search, mode),
                },
            };
            let Some(offset) = found else {
                break;
            };
            let index = range.start + offset;
            let hole = elements.get(index).is_some_and(Value::is_deleted);
            if let Some(settled) = self.settle_stop(array, index, hole, search, mode) {
                return settled;
            }
            range = if reverse { range.start..index } else { index + 1..range.end };
        }
        Search::Missing
    }

    /// `search` when every value equal to it, under strict equality and SameValueZero alike,
    /// is its own word: an int32, or anything but a Number, a string or a BigInt.
    fn word_identity_search(&self, search: Value) -> Option<Value> {
        let plain = if search.as_number().is_some() {
            search.as_int().is_some()
        } else {
            self.heap.string(search).is_none() && !matches!(self.heap.get(search), Some(Cell::BigInt(_)))
        };
        plain.then_some(search)
    }

    /// What the walk does at `index`, where it stopped on a match or a hole: a match is found;
    /// a hole a prototype may hold is the generic algorithm's; any other hole is `undefined`
    /// for `includes` and absent for the rest. `None` goes on past the hole.
    fn settle_stop(&self, array: Value, index: usize, hole: bool, search: Value, mode: Match) -> Option<Search> {
        if !hole {
            return Some(Search::Found(index));
        }
        if !self.inherited_index_absent(array, index) {
            return Some(Search::Resume(index));
        }
        (mode == Match::SameValueZero && search.is_undefined()).then_some(Search::Found(index))
    }

    /// `Array.prototype.indexOf` or `includes` (`native`) of an array from an integer or absent
    /// `fromIndex` that the view decides alone; `None` leaves any other call to the generic
    /// algorithm.
    pub(super) fn dense_search_call(&self, native: Native, array: Value, args: &[Value]) -> Option<Value> {
        let length = self.array_search_elements(array)?.len();
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
    fn position_of<I>(&self, values: I, reverse: bool, search: Value, mode: Match) -> Option<usize>
    where
        I: DoubleEndedIterator<Item = Value> + ExactSizeIterator,
    {
        fn find<I>(mut values: I, reverse: bool, stop: impl Fn(Value) -> bool) -> Option<usize>
        where
            I: DoubleEndedIterator<Item = Value> + ExactSizeIterator,
        {
            let stop = |value: Value| stop(value) || value.is_deleted();
            if reverse { values.rposition(stop) } else { values.position(stop) }
        }
        if let Some(number) = search.as_number() {
            return if number.is_nan() {
                // IsStrictlyEqual never holds for NaN; SameValueZero holds for it.
                find(values, reverse, |value| mode == Match::SameValueZero && value.as_number().is_some_and(f64::is_nan))
            } else if search.as_int().is_some_and(|int| int != 0) {
                // An int32 search other than 0: the same word is the same Number, and any other
                // equal Number is a double word of its value (a raw double), never an int32 one.
                find(values, reverse, |value| value == search || value.is_double() && value.as_number() == Some(number))
            } else {
                find(values, reverse, |value| value.as_number() == Some(number))
            };
        }
        if let Some(text) = self.heap.string(search) {
            return find(values, reverse, |value| value == search || self.heap.string(value).is_some_and(|other| other == text));
        }
        if matches!(self.heap.get(search), Some(Cell::BigInt(_))) {
            return find(values, reverse, |value| self.strict_equal(value, search));
        }
        find(values, reverse, |value| value == search)
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
        let mut stop = view.len().min(length);
        // One allocation for typical pieces (short strings and numbers) instead of growing
        // from empty.
        let mut text = Vec::with_capacity(stop.saturating_mul(separator.len() + 8).min(1 << 16));
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
        if self.heap.is_string(value) {
            // Text read in place, a small string's included (no view made); a rope is flattened.
            let existing = self.heap.flat_text(value);
            let units = match &existing {
                Some(text) => text.units(),
                None => match self.heap.string(value) {
                    Some(string) => string.flat().view(),
                    None => return false,
                },
            };
            return match units {
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
