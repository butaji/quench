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
        // An ASCII unit is its own code point, whose shared string answers it with no string
        // built (`Vm::ascii_byte_string`).
        if let super::wtf16::Units::Ascii(bytes) = units {
            let byte = bytes[position];
            return Some(((self.ascii_byte_string(byte), None), position + 1));
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
        let pair = [value, second];
        self.heap.alloc_literal_array(self.array_proto, 2, |index| pair[index])
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
        self.heap.store_iterator_position(iterator, position + 1);
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
        let (kind, source, position) = self.live_iterator_position(iterator)?;
        let (item, after) = self.collection_iterator_item(kind, source, position)?;
        self.heap.store_iterator_position(iterator, after);
        Some(self.iterator_item_value(item))
    }

    /// `step_collection_value` where an iterator with nothing left is done, as the native's
    /// step leaves it: `Some(None)`. `None` for an iterator that was already done. One read
    /// of the record serves the kind, the source and the position.
    pub(in crate::vm) fn step_collection(&mut self, iterator: Value) -> Option<Option<Value>> {
        let (kind, source, position) = self.live_iterator_position(iterator)?;
        if matches!(kind, IteratorKind::ArrayKeys | IteratorKind::ArrayEntries) {
            return self.step_array_entry(iterator);
        }
        if let Some((item, after)) = self.collection_iterator_item(kind, source, position) {
            self.heap.store_iterator_position(iterator, after);
            return Some(Some(self.iterator_item_value(item)));
        }
        // Nothing is left: the iterator is done, as the native's step leaves it.
        if let Some(IteratorRecord { done, .. }) = self.heap.iterator_mut(iterator) {
            *done = true;
        }
        Some(None)
    }

    /// The kind, source and position of `iterator` while it is not done; `None` for a done
    /// iterator or any other value.
    #[inline(always)]
    fn live_iterator_position(&self, iterator: Value) -> Option<(IteratorKind, Value, usize)> {
        match self.heap.iterator(iterator)? {
            IteratorRecord { source, kind, index, done: false, ext, .. } => Some((*kind, *source, ext.position(*index))),
            _ => None,
        }
    }
}
