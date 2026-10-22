//! Proofs that answer a `[[Get]]` without walking its receiver's chain. A symbol-keyed read
//! (`this[kState]`), and the VM's own reads of named properties (a promise's `constructor`
//! and `then`, an iterator result's `done`), have no compiler cache site, so each read would
//! look the key up in every shape from the receiver to its holder. A proof records, under one
//! `prototype_layout_epoch`, where the walk from a receiver of one shape found `key`: an own
//! data slot, a data slot of a prototype, or nowhere. Every prototype the walk passed was
//! marked a prototype when the proof was made, so any later change to one advances the epoch
//! and retires it; every collection advances it too, so a proof never names a reused record,
//! shape or symbol. The receiver's own layout is its shape, which the proof names.

use super::Value;
use super::property_key::PropertyKey;
use std::cell::Cell;

/// Direct-mapped entries: a power of two. Node's streams and Fastify's request and reply
/// objects read a few dozen distinct symbol keys through a handful of receiver shapes per
/// request, which 256 entries held; the named reads natives make on many shapes share the
/// table, doubled for them.
const SYMBOL_LOAD_PROOF_ENTRIES: usize = 1 << 9;

/// Where a proved walk found the key.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SymbolLoadAnswer {
    /// An own data property in this slot of the receiver's shape.
    Own(u32),
    /// A data property in this slot of `holder`, reached from the receiver's prototype
    /// `prototype`.
    Inherited { prototype: Value, holder: Value, slot: u32 },
    /// No object on the chain from `prototype` has the key.
    Absent { prototype: Value },
    /// An accessor in this slot of `holder`, reached from the receiver's prototype
    /// `prototype`: the read calls its getter, which only the general path does.
    Getter { prototype: Value, holder: Value, slot: u32 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SymbolLoadProof {
    shape: u32,
    key: PropertyKey,
    epoch: u64,
    answer: SymbolLoadAnswer,
}

pub(super) struct SymbolLoadProofs {
    entries: Box<[Cell<Option<SymbolLoadProof>>]>,
}

impl Default for SymbolLoadProofs {
    fn default() -> Self {
        Self { entries: (0..SYMBOL_LOAD_PROOF_ENTRIES).map(|_| Cell::new(None)).collect() }
    }
}

impl SymbolLoadProofs {
    fn index(shape: u32, key: PropertyKey) -> usize {
        super::shape_memo::direct_mapped_index((shape, key), SYMBOL_LOAD_PROOF_ENTRIES)
    }

    /// The answer a proof for `key` on a receiver of `shape` holds under `epoch`.
    #[inline(always)]
    pub(super) fn answer(&self, shape: u32, key: PropertyKey, epoch: u64) -> Option<SymbolLoadAnswer> {
        self.entries[Self::index(shape, key)]
            .get()
            .filter(|proof| proof.shape == shape && proof.key == key && proof.epoch == epoch)
            .map(|proof| proof.answer)
    }

    pub(super) fn record(&self, shape: u32, key: PropertyKey, epoch: u64, answer: SymbolLoadAnswer) {
        self.entries[Self::index(shape, key)].set(Some(SymbolLoadProof { shape, key, epoch, answer }));
    }

    pub(super) fn clear(&self) {
        for entry in self.entries.iter() {
            entry.set(None);
        }
    }
}
