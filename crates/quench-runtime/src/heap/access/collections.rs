//! Map, Set and Symbol, reached through the heap rather than by matching `Cell`
//! variants at each site. Every caller asks the heap for the record it means, so the
//! storage behind these accessors (today a legacy arena cell) can move into a record
//! space of its own without touching a single call site.

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
        matches!(self.get(value), Some(Cell::Symbol(_)))
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
        match self.get(value)? {
            Cell::Symbol(symbol) => Some(symbol),
            _ => None,
        }
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

    /// A new Symbol.
    #[inline]
    pub(crate) fn alloc_symbol(&mut self, symbol: SymbolData) -> Value {
        self.alloc(Cell::Symbol(symbol))
    }
}
