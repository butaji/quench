//! Map, Set and Symbol, reached through the heap rather than by matching `Cell`
//! variants at each site. Every caller asks the heap for the record it means, so the
//! storage behind these accessors can move without touching a single call site. A Symbol
//! is already a record (`Heap::symbol_records`); a Map or Set is still a legacy arena cell.

use super::super::{Cell, CollectionRecord, CollectionStorage, Entries, Heap, Object, SymbolData};
use crate::value::Value;

impl Heap {
    /// A Map's or Set's record, in its direct record or in a slot (`CollectionRecord`).
    #[inline(always)]
    pub(crate) fn collection(&self, value: Value) -> Option<&CollectionRecord> {
        if let Some(record_id) = value.direct_collection_index() {
            // SAFETY: a direct-collection Value names an initialized collection record.
            return Some(unsafe { self.collection_records.get_live(record_id) });
        }
        match self.get(value)? {
            Cell::Collection(collection) => Some(collection),
            _ => None,
        }
    }

    /// `collection`, mutably, past the write barrier.
    #[inline(always)]
    pub(crate) fn collection_mut(&mut self, value: Value) -> Option<&mut CollectionRecord> {
        if let Some(record_id) = value.direct_collection_index() {
            // SAFETY: direct-collection Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.collection_records.get_live_mut(record_id, &self.properties) });
        }
        match self.get_mut(value)? {
            Cell::Collection(collection) => Some(collection),
            _ => None,
        }
    }

    /// The entries of a Map or Set, mutably, past the write barrier.
    #[inline(always)]
    pub(crate) fn collection_storage_mut(&mut self, value: Value) -> Option<&mut CollectionStorage> {
        self.collection_mut(value).map(|collection| &mut collection.entries)
    }

    /// Whether `value` is a Map.
    #[inline(always)]
    pub(crate) fn is_map(&self, value: Value) -> bool {
        matches!(self.collection(value), Some(CollectionRecord { entries: CollectionStorage::Map(_), .. }))
    }

    /// Whether `value` is a Set.
    #[inline(always)]
    pub(crate) fn is_set(&self, value: Value) -> bool {
        matches!(self.collection(value), Some(CollectionRecord { entries: CollectionStorage::Set(_), .. }))
    }

    /// Whether `value` is a Symbol.
    #[inline(always)]
    pub(crate) fn is_symbol(&self, value: Value) -> bool {
        value.direct_symbol_index().is_some()
    }

    /// A Map's entries.
    #[inline(always)]
    pub(crate) fn map_entries(&self, value: Value) -> Option<&Entries<(Value, Value)>> {
        match &self.collection(value)?.entries {
            CollectionStorage::Map(entries) => Some(entries),
            CollectionStorage::Set(_) => None,
        }
    }

    /// `map_entries`, mutably, past the write barrier.
    #[inline(always)]
    pub(crate) fn map_entries_mut(&mut self, value: Value) -> Option<&mut Entries<(Value, Value)>> {
        match self.collection_storage_mut(value)? {
            CollectionStorage::Map(entries) => Some(entries),
            CollectionStorage::Set(_) => None,
        }
    }

    /// A Set's entries.
    #[inline(always)]
    pub(crate) fn set_entries(&self, value: Value) -> Option<&Entries<Value>> {
        match &self.collection(value)?.entries {
            CollectionStorage::Set(entries) => Some(entries),
            CollectionStorage::Map(_) => None,
        }
    }

    /// `set_entries`, mutably, past the write barrier.
    #[inline(always)]
    pub(crate) fn set_entries_mut(&mut self, value: Value) -> Option<&mut Entries<Value>> {
        match self.collection_storage_mut(value)? {
            CollectionStorage::Set(entries) => Some(entries),
            CollectionStorage::Map(_) => None,
        }
    }

    /// Changes the entries of the Map or Set `collection` through `change`, charging what
    /// they grew toward the next collection as `alloc` charges a new collection's.
    pub(crate) fn grow_collection_payload<R>(
        &mut self,
        collection: Value,
        change: impl FnOnce(&mut CollectionStorage) -> R,
    ) -> Option<R> {
        let storage = self.collection_storage_mut(collection)?;
        let before = storage.payload_bytes();
        let result = change(&mut *storage);
        let grown = storage.payload_bytes().saturating_sub(before);
        self.charge_payload_growth(grown);
        Some(result)
    }

    /// A Symbol's data.
    #[inline(always)]
    pub(crate) fn symbol_data(&self, value: Value) -> Option<&SymbolData> {
        let record_id = value.direct_symbol_index()?;
        self.symbol_records.is_live(record_id).then(|| unsafe { self.symbol_records.get_live(record_id) })
    }

    /// A new Map with `object` as its ordinary object part and `entries`.
    #[inline]
    pub(crate) fn alloc_map(&mut self, object: Object, entries: Entries<(Value, Value)>) -> Value {
        self.alloc_collection(CollectionRecord { object, entries: CollectionStorage::Map(entries) })
    }

    /// A new Set with `object` as its ordinary object part and `entries`.
    #[inline]
    pub(crate) fn alloc_set(&mut self, object: Object, entries: Entries<Value>) -> Value {
        self.alloc_collection(CollectionRecord { object, entries: CollectionStorage::Set(entries) })
    }

    /// A new Map or Set record, charged as `alloc` charges its entries. `alloc_object_record`
    /// charges no payload, so the entries' storage is charged here (`alloc` does it itself
    /// when allocations are profiled).
    fn alloc_collection(&mut self, record: CollectionRecord) -> Value {
        let payload = record.entries.payload_bytes();
        let value = self.alloc_object_record(
            record,
            |record| &record.object,
            |heap, record| heap.collection_records.try_insert(record),
            Value::direct_collection,
            |record| Cell::Collection(Box::new(record)),
        );
        if !cfg!(feature = "profile-aggregate") {
            self.charge_payload_growth(payload);
        }
        value
    }

    /// A new Symbol, as a symbol record. Its description is external bytes, charged as a
    /// cell's are (`charge_allocation`), and uncharged when the record is swept.
    #[inline]
    pub(crate) fn alloc_symbol(&mut self, symbol: SymbolData) -> Value {
        let description = symbol.description.as_ref().map_or(0, |description| description.len());
        self.charge_allocation(0, description);
        let Ok(record_id) = self.symbol_records.try_insert(symbol) else {
            panic!("symbol records exhausted");
        };
        let value = Value::direct_symbol(record_id);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
    }
}
