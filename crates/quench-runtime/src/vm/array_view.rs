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

/// Where the generic algorithm resumes a search of `range` the view cannot decide: its last
/// index when `reverse`, else its first.
fn resume_at(range: &std::ops::Range<usize>, reverse: bool) -> Search {
    Search::Resume(if reverse { range.end - 1 } else { range.start })
}

impl<H: Host> Vm<H> {
    /// The element backing of an array whose indices are all own data properties of it: no
    /// indexed descriptors, sparse storage or arguments mapping. A slot holding the hole
    /// marker is a hole. Reading it runs no guest code; a search reads it in place.
    pub(super) fn array_search_elements(&self, this: Value) -> Option<&crate::heap::ArrayElements> {
        let object = self.heap.array(this)?;
        // Only an array marked as having held sparse elements (`Object::mark_sparse_elements`,
        // which every side-table insert sets first) can have a side-table entry: an unmarked
        // one is decided by its header, without the side table's hash probe.
        let sparse = || object.has_sparse_elements() && self.heap.sparse_length(this).is_some();
        (!object.has_indexed_descriptors() && !object.is_arguments_object() && !sparse())
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
        range: std::ops::Range<usize>,
        reverse: bool,
        search: Value,
        mode: Match,
    ) -> Search {
        // The backing itself, decoded element by element where it is narrow or double: a
        // search reads each element once and copies nothing (`array_read_view` would decode an
        // encoded backing into a new vector first).
        let Some(elements) = self.array_search_elements(array) else {
            return if range.is_empty() { Search::Missing } else { resume_at(&range, reverse) };
        };
        self.search_elements(elements, array, range, reverse, search, mode)
    }

    /// `search_dense` over `elements`, the backing `array_search_elements` gave for `array`.
    fn search_elements(
        &self,
        elements: &crate::heap::ArrayElements,
        array: Value,
        mut range: std::ops::Range<usize>,
        reverse: bool,
        search: Value,
        mode: Match,
    ) -> Search {
        // Indices past the vector are the generic algorithm's.
        if range.end > elements.len() && !range.is_empty() {
            return resume_at(&range, reverse);
        }
        let needle = self.classify(search, mode);
        while !range.is_empty() {
            let Some(offset) = self.position_in(elements, range.clone(), reverse, &needle) else {
                break;
            };
            let index = range.start + offset;
            if let Some(settled) = self.settle_search_stop(array, index, slice[offset].is_deleted(), search, mode) {
                return settled;
            }
            range = if reverse { range.start..index } else { index + 1..range.end };
        }
        Search::Missing
    }

    /// Classifies `search` once for a search over the elements.
    fn classify(&self, search: Value, mode: Match) -> Needle {
        let kind = match search.as_number() {
            Some(number) => number_kind(number, mode),
            None if self.heap.string(search).is_some() || matches!(self.heap.get(search), Some(Cell::BigInt(_))) => {
                Kind::Content
            }
            None => Kind::One(search.to_bits()),
        };
        Needle { value: search, kind, narrow: narrow_key(search) }
    }

    /// The offset in `range` of the first (last when `reverse`) element that `needle` matches
    /// or that is a hole, read from the backing's own form.
    fn position_in(
        &self,
        elements: &crate::heap::ArrayElements,
        range: std::ops::Range<usize>,
        reverse: bool,
        needle: &Needle,
    ) -> Option<usize> {
        if let Some(words) = elements.narrow_words().filter(|_| !matches!(needle.kind, Kind::Content)) {
            return find_narrow(&words[range], reverse, needle.narrow);
        }
        match elements.wide_values() {
            Some(values) => self.position_of(values[range].iter().copied(), reverse, needle),
            None => self.position_of(elements.range(range), reverse, needle),
        }
    }

    /// `position_in` over values, comparing by the needle's kind.
    fn position_of<I>(&self, values: I, reverse: bool, needle: &Needle) -> Option<usize>
    where
        I: DoubleEndedIterator<Item = Value> + ExactSizeIterator,
    {
        match needle.kind {
            Kind::One(word) => first_stop(values, reverse, |value| value.to_bits() == word),
            Kind::Two(words) => first_stop(values, reverse, |value| words.contains(&value.to_bits())),
            Kind::Three(words) => first_stop(values, reverse, |value| words.contains(&value.to_bits())),
            Kind::Never => first_stop(values, reverse, |_| false),
            Kind::Nan => first_stop(values, reverse, |value| {
                value.is_double() && f64::from_bits(value.to_bits()).is_nan()
            }),
            Kind::Content => self.position_content(values, reverse, needle.value),
        }
    }

    /// The first (last when `reverse`) value that is `search` itself, or a string with the same
    /// units (compared by identity first, then length and units) or a BigInt of the same value.
    fn position_content<I>(&self, values: I, reverse: bool, search: Value) -> Option<usize>
    where
        I: DoubleEndedIterator<Item = Value> + ExactSizeIterator,
    {
        let text = self.heap.string(search);
        first_stop(values, reverse, |value| match text {
            Some(text) => value == search || self.heap.string(value).is_some_and(|other| other == text),
            None => self.strict_equal(value, search),
        })
    }

    /// What the walk does at `index`, where it stopped on a match or a hole: a match is found;
    /// a hole a prototype may hold is the generic algorithm's; any other hole is `undefined`
    /// for `includes` and absent for the rest. `None` goes on past the hole.
    fn settle_search_stop(&self, array: Value, index: usize, hole: bool, search: Value, mode: Match) -> Option<Search> {
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
        // One proof of the backing serves the length and the search.
        let elements = self.array_search_elements(array)?;
        let length = elements.len();
        let start = match args.get(1) {
            None => 0,
            Some(from) => crate::units::relative_index(f64::from(from.as_int()?), length),
        };
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        let includes = native == Native::ArrayIncludes;
        let mode = if includes { Match::SameValueZero } else { Match::Strict };
        Some(match self.search_elements(elements, array, start..length, false, search, mode) {
            Search::Found(_) if includes => Value::TRUE,
            Search::Found(index) => Value::number(index as f64),
            Search::Missing if includes => Value::FALSE,
            Search::Missing => Value::number(-1.0),
            Search::Resume(_) => return None,
        })
    }
}
