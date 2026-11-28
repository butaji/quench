//! Map, Set and Symbol, reached through the heap rather than by matching `Cell`
//! variants at each site. Every caller asks the heap for the record it means, so the
//! storage behind these accessors can move without touching a single call site. Each of
//! them is a record of its own family (`Heap::collection_records`, `Heap::symbol_records`).

use super::super::{CollectionRecord, CollectionStorage, Entries, Heap, Object, SymbolData, WasmGcRecord};
use crate::value::Value;

impl Heap {
    /// A Map's or Set's record (`CollectionRecord`).
    #[inline(always)]
    pub(crate) fn collection(&self, value: Value) -> Option<&CollectionRecord> {
        let record_id = value.direct_collection_index()?;
        // SAFETY: a direct-collection Value names an initialized collection record.
        Some(unsafe { self.collection_records.get_live(record_id) })
    }

    /// `collection`, mutably, past the write barrier.
    #[inline(always)]
    pub(crate) fn collection_mut(&mut self, value: Value) -> Option<&mut CollectionRecord> {
        let record_id = value.direct_collection_index()?;
        // SAFETY: direct-collection Values name live records; `&mut self` is exclusive.
        Some(unsafe { self.collection_records.get_live_mut(record_id) })
    }

    /// The entries of a Map or Set, mutably, past the write barrier.
    #[inline(always)]
    pub(crate) fn collection_storage_mut(&mut self, value: Value) -> Option<&mut CollectionStorage> {
        self.collection_mut(value).map(|collection| &mut collection.entries)
    }

    /// Whether `value` is a Map.
    #[inline(always)]
    pub fn is_map(&self, value: Value) -> bool {
        matches!(self.collection(value), Some(CollectionRecord { entries: CollectionStorage::Map(_), .. }))
    }

    /// Whether `value` is a Set.
    #[inline(always)]
    pub fn is_set(&self, value: Value) -> bool {
        matches!(self.collection(value), Some(CollectionRecord { entries: CollectionStorage::Set(_), .. }))
    }

    /// Whether `value` is a Symbol.
    #[inline(always)]
    pub fn is_symbol(&self, value: Value) -> bool {
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
    pub fn set_entries(&self, value: Value) -> Option<&Entries<Value>> {
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

    /// The Wasm GC object `value` names, read without the write barrier: reads take no barrier.
    #[inline(always)]
    pub(crate) fn wasm_gc(&self, value: Value) -> Option<&WasmGcRecord> {
        let record_id = value.direct_wasm_gc_index()?;
        self.wasm_gc_records.is_live(record_id).then(|| unsafe { self.wasm_gc_records.get_live(record_id) })
    }

    /// The Wasm GC object `value` names, mutably, past the write barrier: for a write into its payload.
    #[inline(always)]
    pub(crate) fn wasm_gc_mut(&mut self, value: Value) -> Option<&mut WasmGcRecord> {
        let record_id = value.direct_wasm_gc_index()?;
        if !self.wasm_gc_records.is_live(record_id) {
            return None;
        }
        // SAFETY: a live direct Wasm GC Value names an initialized record; `&mut self` is exclusive.
        Some(unsafe { self.wasm_gc_records.get_live_mut(record_id) })
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

    /// A new Map or Set record, admitted and charged as an object record is, with its entries'
    /// storage charged as `alloc` charges a collection's. The collection space holds
    /// `COLLECTION_RECORD_CAPACITY` (2^28) records, so a full space is a panic.
    fn alloc_collection(&mut self, record: CollectionRecord) -> Value {
        let payload = record.entries.payload_bytes();
        #[cfg(feature = "profile-aggregate")]
        let (kind, variant) = match record.entries {
            CollectionStorage::Map(_) => (super::super::CellKind::Map, "map"),
            CollectionStorage::Set(_) => (super::super::CellKind::Set, "set"),
        };
        #[cfg(feature = "profile-aggregate")]
        let bytes = record.object.allocated_extra_bytes() + payload;
        self.admit_object_contents(&record.object);
        self.mark_prototype(record.object.proto());
        self.allocated += 1;
        let Ok(record_id) = self.collection_records.try_insert(record) else {
            panic!("collection records exhausted: the collection space holds 2^28 records");
        };
        let value = Value::direct_collection(record_id);
        #[cfg(feature = "profile-aggregate")]
        {
            self.profile_record_allocation(kind, variant, bytes);
            self.charge_allocation(payload, 0);
        }
        #[cfg(not(feature = "profile-aggregate"))]
        self.charge_payload_growth(payload);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
    }

    /// A new Wasm GC object, as a record whose head names its payload. The payload is external
    /// bytes, charged as a description is (`alloc_symbol`) and uncharged when the record is swept.
    #[inline]
    pub(crate) fn alloc_wasm_gc(&mut self, record: WasmGcRecord) -> Value {
        let payload = record.allocated_bytes();
        self.charge_allocation(0, payload);
        let Ok(record_id) = self.wasm_gc_records.try_insert(record) else {
            panic!("Wasm GC records exhausted: the space holds 2^28 records");
        };
        let value = Value::direct_wasm_gc(record_id);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
    }

    /// A new Symbol, as a symbol record. Its description is external bytes, charged as a
    /// cell's are (`charge_allocation`), and uncharged when the record is swept.
    #[inline]
    pub fn alloc_symbol(&mut self, symbol: SymbolData) -> Value {
        let description = symbol.description.as_ref().map_or(0, |description| description.len());
        self.charge_allocation(0, description);
        let Ok(record_id) = self.symbol_records.try_insert(symbol) else {
            panic!("symbol records exhausted: the symbol space holds 2^27 records");
        };
        let value = Value::direct_symbol(record_id);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
    }
}
