//! Map/Set/WeakMap/WeakSet hash and probe: one lookup body.
//!
//! Every Map and Set lookup and insert step (`Map.prototype.get`, `has`, `set`, `delete`,
//! `Set.prototype.add`, `has`, `delete`, `groupBy`, the get-or-insert pair, the set relations)
//! hashes its key once with `key_hash` and finds the entry with `probe`. The entry table is a
//! flat array of slots and folded hashes with its own key index, not a layout a `Stage` can
//! address, so both are plain functions over the heap's entries rather than `Stage` bodies. The
//! WeakMap and WeakSet lookups are answered by the same receiver dispatch in `vm::collections`
//! (`pure_collection_lookup`); their identity map is keyed by the value itself.

use crate::Value;
use crate::heap::{CollectionEntries, CollectionEntry, Heap, number_key_hash};

/// The SameValueZero hash of `key`, under which SameValueZero-equal keys agree: numbers by value
/// (`number_key_hash`), strings and BigInts by content, anything else by identity
/// (`Heap::non_number_key_hash`). A key is hashed once per lookup, and an insert reuses the hash.
#[inline(always)]
pub(crate) fn key_hash(heap: &Heap, key: Value) -> u64 {
    match key.as_number() {
        Some(number) => number_key_hash(number),
        None => heap.non_number_key_hash(key),
    }
}

/// The slot of the entry in `entries` whose key is SameValueZero to `key`, whose hash is
/// `key_hash` (from `key_hash`). `same_value_zero` decides keys spelled differently; identical
/// bits are SameValueZero (a NaN included), so it runs only for those.
#[inline(always)]
pub(crate) fn probe<T: CollectionEntry>(
    entries: &CollectionEntries<T>,
    key: Value,
    key_hash: u64,
    same_value_zero: impl Fn(Value) -> bool,
) -> Option<usize> {
    entries.position(key, key_hash, same_value_zero)
}
