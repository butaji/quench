//! Map, Set and Symbol, reached through the heap rather than by matching `Cell`
//! variants at each site. Every caller asks the heap for the record it means, so the
//! storage behind these accessors can move without touching a single call site. A Symbol
//! is already a record (`Heap::symbol_records`); a Map or Set is still a legacy arena cell.

use super::super::{Cell, Entries, Heap, Object, SymbolData};
use crate::value::Value;

impl Heap {
    /// Whether `value` is a Map.
    #[inline(always)]
    pub(crate) fn is_map(&self, value: Value) -> bool {
        matches!(self.get(value), Some(Cell::Map { .. }))
    }

    /// Whether `value` is a Set.
    #[inline(always)]
    pub(crate) fn is_set(&self, value: Value) -> bool {
        matches!(self.get(value), Some(Cell::Set { .. }))
    }

    /// Whether `value` is a Symbol.
    #[inline(always)]
    pub(crate) fn is_symbol(&self, value: Value) -> bool {
        value.direct_symbol_index().is_some()
    }

    /// A Map's entries.
    #[inline(always)]
    pub(crate) fn map_entries(&self, value: Value) -> Option<&Entries<(Value, Value)>> {
        match self.get(value)? {
            Cell::Map { entries, .. } => Some(entries),
            _ => None,
        }
    }

    /// `map_entries`, mutably, past the write barrier.
    #[inline(always)]
    pub(crate) fn map_entries_mut(&mut self, value: Value) -> Option<&mut Entries<(Value, Value)>> {
        match self.get_mut(value)? {
            Cell::Map { entries, .. } => Some(entries),
            _ => None,
        }
    }

    /// A Set's entries.
    #[inline(always)]
    pub(crate) fn set_entries(&self, value: Value) -> Option<&Entries<Value>> {
        match self.get(value)? {
            Cell::Set { entries, .. } => Some(entries),
            _ => None,
        }
    }

    /// `set_entries`, mutably, past the write barrier.
    #[inline(always)]
    pub(crate) fn set_entries_mut(&mut self, value: Value) -> Option<&mut Entries<Value>> {
        match self.get_mut(value)? {
            Cell::Set { entries, .. } => Some(entries),
            _ => None,
        }
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
        self.alloc(Cell::Map { object, entries })
    }

    /// A new Set with `object` as its ordinary object part and `entries`.
    #[inline]
    pub(crate) fn alloc_set(&mut self, object: Object, entries: Entries<Value>) -> Value {
        self.alloc(Cell::Set { object, entries })
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
