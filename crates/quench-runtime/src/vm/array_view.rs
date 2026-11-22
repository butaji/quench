//! The one dense view of an array's elements, and the searches over it.
//!
//! `array_read_view` proves once, per call, that every element index of an array is its own
//! data property held in the element vector, so the built-ins that only read elements walk the
//! borrowed slice instead of the per-element [[Get]]. A hole in the slice is the one place the
//! view does not answer by itself: the prototype chain decides it (`inherited_index_absent`).
//! The walk over the elements is the staged body `staged::array_search`; this file supplies the
//! VM's part of it (the holes, the content comparison and the generic resume).

use super::*;
use crate::staged::array_search::{self, Needle};
pub(super) use crate::staged::array_search::{Match, Search};

impl Vm {
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
            return if range.is_empty() { Search::Missing } else { array_search::resume_at(&range, reverse) };
        };
        self.search_elements(elements, array, range, reverse, search, mode)
    }

    /// `search_dense` over `elements`, the backing `array_search_elements` gave for `array`.
    fn search_elements(
        &self,
        elements: &crate::heap::ArrayElements,
        array: Value,
        range: std::ops::Range<usize>,
        reverse: bool,
        search: Value,
        mode: Match,
    ) -> Search {
        // A string or BigInt `search` is compared by content (`content_matches`).
        let text = self.heap.string(search);
        let needle = Needle::new(search, mode, text.is_some() || matches!(self.heap.get(search), Some(Cell::BigInt(_))));
        let content = self.content_matches(search, text);
        let settle = |index, hole| self.settle_search_stop(array, index, hole, search, mode);
        array_search::search(elements, range, reverse, &needle, content, settle)
    }

    /// The content comparison of `search`, whose string `text` is given: a string equals a value
    /// with the same units (by identity first, then length and units), a BigInt one of the same
    /// value, and anything else strictly equal.
    fn content_matches<'a>(&'a self, search: Value, text: Option<&'a crate::vm::wtf16::JsString>)
        -> impl Fn(Value) -> bool + 'a {
        move |value| match text {
            Some(text) => value == search || self.heap.string(value).is_some_and(|other| other == text),
            None => self.strict_equal(value, search),
        }
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
    /// `fromIndex` that the view decides alone: `Ok` is the answer. `Err(None)` when the view
    /// declined before walking; `Err(Some(index))` when the walk stopped at a hole a prototype
    /// may fill, having covered every index before `index` from `fromIndex`. The generic
    /// algorithm resumes at that index rather than searching the prefix again.
    pub fn dense_search_resume(&self, native: Native, array: Value, args: &[Value]) -> Result<Value, Option<usize>> {
        // One proof of the backing serves the length and the search.
        let Some(elements) = self.array_search_elements(array) else {
            return Err(None);
        };
        let length = elements.len();
        let start = match args.get(1) {
            None => 0,
            Some(from) => {
                let Some(from) = from.as_int() else {
                    return Err(None);
                };
                crate::units::relative_index(f64::from(from), length)
            }
        };
        let search = args.first().copied().unwrap_or(Value::UNDEFINED);
        let includes = native == Native::ArrayIncludes;
        let mode = if includes { Match::SameValueZero } else { Match::Strict };
        Ok(match self.search_elements(elements, array, start..length, false, search, mode) {
            Search::Found(_) if includes => Value::TRUE,
            Search::Found(index) => Value::number(index as f64),
            Search::Missing if includes => Value::FALSE,
            Search::Missing => Value::number(-1.0),
            Search::Resume(index) => return Err(Some(index)),
        })
    }
}
