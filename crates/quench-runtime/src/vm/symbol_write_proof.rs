//! Proofs that a prototype chain lets a symbol-keyed `[[Set]]` add the key to its receiver.
//! A symbol-keyed write (`this[kState] = ...`) has no compiler cache site, so a repeated add
//! through the same chain would walk every prototype each time. A proof records that, under
//! one `prototype_layout_epoch`, the chain from `prototype` holds no accessor or read-only
//! property named `key` (`Vm::add_prototype_guard`). Every link was marked a prototype when
//! the proof was made, so any later change to one advances the epoch and retires it; every
//! collection advances it too, so a proof never names a reused record.

use super::Value;
use std::cell::Cell;

/// Direct-mapped entries: a power of two. Node's stream constructors write about a dozen
/// distinct symbol keys through a handful of prototype chains per request; 64 entries hold
/// that working set in 1.5 KiB.
const SYMBOL_WRITE_PROOF_ENTRIES: usize = 1 << 6;

#[derive(Clone, Copy, PartialEq, Eq)]
struct SymbolWriteProof {
    prototype: Value,
    key: Value,
    epoch: u64,
}

pub(super) struct SymbolWriteProofs {
    entries: Box<[Cell<Option<SymbolWriteProof>>]>,
}

impl Default for SymbolWriteProofs {
    fn default() -> Self {
        Self { entries: (0..SYMBOL_WRITE_PROOF_ENTRIES).map(|_| Cell::new(None)).collect() }
    }
}

impl SymbolWriteProofs {
    fn index(prototype: Value, key: Value) -> usize {
        super::shape_memo::direct_mapped_index((prototype, key), SYMBOL_WRITE_PROOF_ENTRIES)
    }

    /// Whether a proof for `key` through the chain from `prototype` holds under `epoch`.
    #[inline(always)]
    pub(super) fn holds(&self, prototype: Value, key: Value, epoch: u64) -> bool {
        self.entries[Self::index(prototype, key)].get()
            == Some(SymbolWriteProof { prototype, key, epoch })
    }

    pub(super) fn record(&self, prototype: Value, key: Value, epoch: u64) {
        self.entries[Self::index(prototype, key)].set(Some(SymbolWriteProof { prototype, key, epoch }));
    }

    pub(super) fn clear(&self) {
        for entry in self.entries.iter() {
            entry.set(None);
        }
    }
}
