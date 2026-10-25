//! Proofs that a prototype chain lets a symbol-keyed `[[Set]]` add the key to its receiver.
//! A symbol-keyed write (`this[kState] = ...`) has no compiler cache site, so a repeated add
//! through the same chain would walk every prototype each time. A proof records that, under
//! one `prototype_layout_epoch`, the chain from `prototype` holds no accessor or read-only
//! property named `key` (`Vm::add_prototype_guard`). Every link was marked a prototype when
//! the proof was made, so any later change to one advances the epoch and retires it; every
//! collection advances it too, so a proof never names a reused record.

use super::Value;

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
    entries: super::shape_memo::DirectMappedTable<SymbolWriteProof, SYMBOL_WRITE_PROOF_ENTRIES>,
    /// Per lane write site: the write it made last (`SiteSymbolWrite`), so the site's next
    /// write to a receiver of that shape is one compare and the store. Sites map as the
    /// symbol read sites do (`symbol_load_proof::site_index`).
    sites: super::shape_memo::DirectMappedTable<
        SiteSymbolWrite,
        { super::symbol_load_proof::SITE_OWN_READ_ENTRIES },
    >,
}

/// What `Vm::set_plain_symbol_property` did for `key` on an extensible receiver of the
/// non-dictionary `shape` under `epoch`, recorded at the write site `site` (a lane record's
/// address). Either step is a fact of the shape, key and epoch alone; the site only spreads
/// the entries.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SiteSymbolWrite {
    site: usize,
    shape: u32,
    key: Value,
    epoch: u64,
    step: SymbolWriteStep,
}

/// A recorded symbol write's effect on its receiver.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SymbolWriteStep {
    /// The shape holds `key` as a writable own data property in this slot.
    Replace(u32),
    /// The shape lacks `key`, a receiver whose prototype is `prototype` gets it by the
    /// transition to `target`: the add a proof for that chain admitted (`holds`).
    Add { prototype: Value, target: u32 },
}

impl Default for SymbolWriteProofs {
    fn default() -> Self {
        Self {
            entries: Default::default(),
            sites: Default::default(),
        }
    }
}

impl SymbolWriteProofs {
    fn index(prototype: Value, key: Value) -> usize {
        super::shape_memo::direct_mapped_index((prototype, key), SYMBOL_WRITE_PROOF_ENTRIES)
    }

    /// Whether a proof for `key` through the chain from `prototype` holds under `epoch`.
    #[inline(always)]
    pub(super) fn holds(&self, prototype: Value, key: Value, epoch: u64) -> bool {
        self.entries.get(Self::index(prototype, key))
            == Some(SymbolWriteProof {
                prototype,
                key,
                epoch,
            })
    }

    pub(super) fn record(&self, prototype: Value, key: Value, epoch: u64) {
        self.entries.set(
            Self::index(prototype, key),
            SymbolWriteProof {
                prototype,
                key,
                epoch,
            },
        );
    }

    /// The step site `site` last made writing `key` to a receiver of `shape`, while `epoch`
    /// holds.
    #[inline(always)]
    pub(super) fn site_step(
        &self,
        site: usize,
        shape: u32,
        key: Value,
        epoch: u64,
    ) -> Option<SymbolWriteStep> {
        self.sites
            .get(super::symbol_load_proof::site_index(site))
            .filter(|write| {
                write.site == site
                    && write.shape == shape
                    && write.key == key
                    && write.epoch == epoch
            })
            .map(|write| write.step)
    }

    pub(super) fn record_site_step(
        &self,
        site: usize,
        shape: u32,
        key: Value,
        epoch: u64,
        step: SymbolWriteStep,
    ) {
        self.sites.set(
            super::symbol_load_proof::site_index(site),
            SiteSymbolWrite {
                site,
                shape,
                key,
                epoch,
                step,
            },
        );
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
        self.sites.clear();
    }
}
