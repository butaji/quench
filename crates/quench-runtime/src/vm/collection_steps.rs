//! Steps of String, Map and Set iterators: what one yields, shared by the natives' `next`
//! and the lane's step in place.
use super::*;

/// What a step yields: a value, or the two values of an entry pair.
pub(super) type IteratorItem = (Value, Option<Value>);

/// The `next` native of an iterator of `kind`, when it is one of these iterators.
pub(super) fn next_native(kind: IteratorKind) -> Option<Native> {
    match kind {
        IteratorKind::String => Some(Native::StringIteratorNext),
        IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries | IteratorKind::SetValues | IteratorKind::SetEntries => {
            Some(Native::IteratorNext)
        }
        _ => None,
    }
}

impl<H: Host> Vm<H> {
    /// The item an iterator of `kind` over `source` yields from `position`, and the position
    /// after it: a string's next code point, or a collection's next live entry, whose
    /// deleted slots the position skips. `None` once nothing is left, or for another kind.
    pub(super) fn collection_iterator_item(&mut self, kind: IteratorKind, source: Value, position: usize) -> Option<(IteratorItem, usize)> {
        if kind == IteratorKind::String {
            return self.string_iterator_item(source, position);
        }
        match kind {
            IteratorKind::MapKeys => self.heap.map_entries(source)?.next_live(position).map(|(slot, (key, _))| ((key, None), slot + 1)),
            IteratorKind::MapValues => self.heap.map_entries(source)?.next_live(position).map(|(slot, (_, value))| ((value, None), slot + 1)),
            IteratorKind::MapEntries => self.heap.map_entries(source)?.next_live(position).map(|(slot, (key, value))| ((key, Some(value)), slot + 1)),
            IteratorKind::SetValues => self.heap.set_entries(source)?.next_live(position).map(|(slot, value)| ((value, None), slot + 1)),
            IteratorKind::SetEntries => self.heap.set_entries(source)?.next_live(position).map(|(slot, value)| ((value, Some(value)), slot + 1)),
            _ => None,
        }
    }

    /// A string iterator's item: its position is the code unit its next code point starts
    /// at, so each step reads one code point.
    fn string_iterator_item(&mut self, source: Value, position: usize) -> Option<(IteratorItem, usize)> {
        let text = self.heap.string(source)?;
        let units = text.units();
        if position >= units.len() {
            return None;
        }
        let pair = units.get(position + 1).is_some_and(|low| crate::unicode::decode_surrogate_pair(units.at(position), low).is_some());
        let after = position + 1 + usize::from(pair);
        let run = super::wtf16::JsString::from_unit_run(units.slice(position..after));
        Some(((self.heap.alloc_string(run), None), after))
    }

    /// The value of `item`: an entry pair is a new array of its two values, or, during a step
    /// whose value only an array pattern reads (`share_entry_pair`), the one shared pair
    /// rewritten (`shared_entry_pair_of`).
    pub(super) fn iterator_item_value(&mut self, (value, second): IteratorItem) -> Value {
        let Some(second) = second else {
            return value;
        };
        if self.share_entry_pair
            && let Some(pair) = self.shared_entry_pair_of(value, second)
        {
            return pair;
        }
        let pair = [value, second];
        self.heap.alloc_literal_array(self.array_proto, 2, |index| pair[index])
    }

    /// `shared_entry_pair` holding `key` and `value`, for a for-of step whose value goes only to
    /// an array pattern (`RequireIteratorResult` marked by the compiler,
    /// `FunctionCompiler::for_iterable`). Nothing else ever holds the pair: the pattern reads it
    /// by index, or, if that test fails, iterates it with the intrinsic array iterator, which
    /// this checks is still the one a pattern would get, so no code can keep or see it, and the
    /// next such step may rewrite it. `None` where iteration of the pair is not intact.
    fn shared_entry_pair_of(&mut self, key: Value, value: Value) -> Option<Value> {
        let pair = self.shared_entry_pair;
        if pair.is_undefined() {
            let values = [key, value];
            let pair = self.heap.alloc_literal_array(self.array_proto, 2, |index| values[index]);
            if !self.array_iteration_intact(pair) {
                return None;
            }
            self.shared_entry_pair = pair;
            return Some(pair);
        }
        // The pair keeps the shape, prototype and length it was made with, as nothing else
        // holds it; only the iteration fuses or the realm can have changed since.
        if !(self.heap.array(pair).is_some_and(|array| array.proto() == self.array_proto)
            && self.fuse_intact(crate::code_dependencies::IntactFuse::ArrayIterator)
            && self.fuse_intact(crate::code_dependencies::IntactFuse::ArrayIteratorNext))
        {
            return None;
        }
        for (index, element) in [key, value].into_iter().enumerate() {
            // The store passes the write barrier for each value it writes.
            let array = self.heap.array_store_mut(pair, element)?;
            array.elements_mut().make_mut().set(index, element);
        }
        Some(pair)
    }

    /// One %ArrayIteratorPrototype%.next step of a keys or entries iterator over a plain
    /// array where it runs no guest code, as `step_present_array_value` makes a values step:
    /// the index, or `[index, element]` of a present data element (`Some(Some(_))`), or the
    /// iterator done past the end (`Some(None)`). `None` for a hole, an exotic or sparse
    /// array, or a done iterator, whose step the native makes.
    pub(in crate::vm) fn step_array_entry(&mut self, iterator: Value) -> Option<Option<Value>> {
        let IteratorRecord { source, kind, index, done: false, ext, .. } = self.heap.iterator(iterator)? else {
            return None;
        };
        let (source, kind, position) = (*source, *kind, ext.position(*index));
        let array = self.heap.array(source)?;
        if array.has_indexed_descriptors() || array.is_arguments_object() || self.heap.sparse_length(source).is_some() {
            return None;
        }
        let Some(element) = array.elements().get(position) else {
            if let Some(IteratorRecord { done, source, .. }) = self.heap.iterator_mut(iterator) {
                *done = true;
                *source = Value::UNDEFINED;
            }
            return Some(None);
        };
        if element.is_deleted() {
            return None;
        }
        if let Some(IteratorRecord { index, ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.store_position(index, position + 1);
        }
        let key = Value::number(position as f64);
        Some(Some(match kind {
            IteratorKind::ArrayKeys => key,
            _ => self.iterator_item_value((key, Some(element))),
        }))
    }

    /// One `next` step of a String, Map or Set iterator that has an item, where the step
    /// runs no guest code: the item's value, the iterator left past it. `None` for an
    /// iterator that is done or has nothing left, whose step the native makes.
    pub(in crate::vm) fn step_collection_value(&mut self, iterator: Value) -> Option<Value> {
        let IteratorRecord { source, kind, index, done: false, ext, .. } = self.heap.iterator(iterator)? else {
            return None;
        };
        let (source, kind, position) = (*source, *kind, ext.position(*index));
        let (item, after) = self.collection_iterator_item(kind, source, position)?;
        if let Some(IteratorRecord { index, ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.store_position(index, after);
        }
        Some(self.iterator_item_value(item))
    }

    /// `step_collection_value` where an iterator with nothing left is done, as the native's
    /// step leaves it: `Some(None)`. `None` for an iterator that was already done.
    pub(in crate::vm) fn step_collection(&mut self, iterator: Value) -> Option<Option<Value>> {
        if self.heap.iterator(iterator).is_some_and(|record| matches!(record.kind, IteratorKind::ArrayKeys | IteratorKind::ArrayEntries)) {
            return self.step_array_entry(iterator);
        }
        if let Some(value) = self.step_collection_value(iterator) {
            return Some(Some(value));
        }
        match self.heap.iterator_mut(iterator)? {
            IteratorRecord { done, .. } if !*done => {
                *done = true;
                Some(None)
            }
            _ => None,
        }
    }
}
