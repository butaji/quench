//! AddEntriesFromIterable of `new Map(iterable)` and `new Set(iterable)` where no step can
//! run guest code, so the iterable is read from its store (`IterationSource`).

use super::collections::{MAP_ENTRY_KEY_INDEX, MAP_ENTRY_VALUE_INDEX};
use super::iterators::IterationSource;
use super::*;

/// A Map's or Set's table, copied for another collection of the same kind.
enum TableCopy {
    Set(crate::heap::Entries<Value>),
    Map(crate::heap::Entries<(Value, Value)>),
}

impl<H: Host> Vm<H> {
    /// AddEntriesFromIterable when no step can run guest code: the adder is the intrinsic
    /// `Set.prototype.add` or `Map.prototype.set` and the iterable iterates intact
    /// (`intact_iteration_kind`). `false` (nothing added) leaves every other case to the
    /// protocol.
    pub(super) fn add_intact_entries(&mut self, native: Native, collection: Value, adder: Value, iterable: Value) -> bool {
        let adds = match native {
            Native::Set => self.is_current_realm_native(adder, Native::SetAdd),
            Native::Map => self.is_current_realm_native(adder, Native::MapSet),
            _ => false,
        };
        let Some(kind) = adds.then(|| self.intact_iteration_kind(iterable)).flatten() else {
            return false;
        };
        match (native, kind) {
            (Native::Set, IterationSource::Set) | (Native::Map, IterationSource::Map) if self.copy_collection_table(collection, iterable) => true,
            (Native::Map, IterationSource::Map) => self.add_map_pairs(collection, iterable),
            (Native::Map, _) => self.add_entry_arrays(collection, kind, iterable),
            _ => {
                let Some(values) = self.iteration_source_values(kind, iterable) else { return false };
                for value in values {
                    self.set_add(collection, value);
                }
                true
            }
        }
    }

    /// The entries of the intact Map `source` added to the Map `collection`, with no entry
    /// array made.
    fn add_map_pairs(&mut self, collection: Value, source: Value) -> bool {
        let Some(pairs) = self.intact_map_pairs(source) else { return false };
        for pair in &pairs {
            self.map_set(collection, pair);
        }
        true
    }

    /// The entries of the intact `iterable` added to the Map `collection`: every entry is an
    /// array whose indices 0 and 1 are present own data elements, so each Get reads an element.
    fn add_entry_arrays(&mut self, collection: Value, kind: IterationSource, iterable: Value) -> bool {
        let Some(values) = self.iteration_source_values(kind, iterable) else { return false };
        let Some(pairs) = values
            .iter()
            .map(|&entry| {
                let pair = self.present_dense_elements(entry, MAP_ENTRY_KEY_INDEX..MAP_ENTRY_VALUE_INDEX + 1)?;
                Some([pair[MAP_ENTRY_KEY_INDEX], pair[MAP_ENTRY_VALUE_INDEX]])
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        for pair in pairs {
            self.map_set(collection, &pair);
        }
        true
    }

    /// The table of `source` copied whole into the empty `collection` of the same kind (a Map
    /// into a Map, a Set into a Set): its keys are already distinct and canonical, so no entry
    /// is looked up or hashed again. `false` leaves `collection` as it was.
    fn copy_collection_table(&mut self, collection: Value, source: Value) -> bool {
        let copy = match (self.heap.get(source), self.heap.get(collection)) {
            (Some(Cell::Set { entries, .. }), Some(Cell::Set { entries: target, .. })) if target.len() == 0 => {
                TableCopy::Set(entries.duplicate().into())
            }
            (Some(Cell::Map { entries, .. }), Some(Cell::Map { entries: target, .. })) if target.len() == 0 => {
                TableCopy::Map(entries.duplicate().into())
            }
            _ => return false,
        };
        self.heap.grow_cell_payload(collection, |cell| match (cell, copy) {
            (Cell::Set { entries, .. }, TableCopy::Set(copy)) => *entries = copy,
            (Cell::Map { entries, .. }, TableCopy::Map(copy)) => *entries = copy,
            _ => unreachable!("the copy is of the collection's kind"),
        });
        true
    }
}
