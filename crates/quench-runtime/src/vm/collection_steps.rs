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
        match (kind, self.heap.get(source)?) {
            (IteratorKind::MapKeys, Cell::Map { entries, .. }) => entries.next_live(position).map(|(slot, (key, _))| ((key, None), slot + 1)),
            (IteratorKind::MapValues, Cell::Map { entries, .. }) => entries.next_live(position).map(|(slot, (_, value))| ((value, None), slot + 1)),
            (IteratorKind::MapEntries, Cell::Map { entries, .. }) => entries.next_live(position).map(|(slot, (key, value))| ((key, Some(value)), slot + 1)),
            (IteratorKind::SetValues, Cell::Set { entries, .. }) => entries.next_live(position).map(|(slot, value)| ((value, None), slot + 1)),
            (IteratorKind::SetEntries, Cell::Set { entries, .. }) => entries.next_live(position).map(|(slot, value)| ((value, Some(value)), slot + 1)),
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

    /// The value of `item`: an entry pair is a new array of its two values.
    pub(super) fn iterator_item_value(&mut self, (value, second): IteratorItem) -> Value {
        let Some(second) = second else {
            return value;
        };
        self.heap.alloc(Cell::Array { object: crate::heap::ArrayObject::new(Self::empty_object(self.array_proto), vec![value, second]) })
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
