//! Proofs that a prototype chain lets a symbol-keyed `[[Set]]` add the key to its receiver.
//! A symbol-keyed write (`this[kState] = ...`) has no compiler cache site, so a repeated add
//! through the same chain would walk every prototype each time. A proof records that, while
//! the chain's cell holds (`heap::ChainCell`), the chain from `prototype` holds no accessor or
//! read-only property named `key` (`Vm::add_prototype_guard`). Every link was marked a
//! prototype when the proof was made, so a later change to one, or its death, retires the
//! cell; a change to a prototype off the chain leaves it.

use super::Value;

/// Direct-mapped entries: a power of two. Node's stream constructors write about a dozen
/// distinct symbol keys through a handful of prototype chains per request; 64 entries hold
/// that working set in 1.5 KiB.
const SYMBOL_WRITE_PROOF_ENTRIES: usize = 1 << 6;

/// Entries each write site keeps, most recent first. A constructor shared by two object
/// families writes one symbol to two shapes in turn: Node's `Stream` constructor runs for
/// every request's IncomingMessage and its ServerResponse, so a single entry would be
/// replaced at every call and the write would take the full path each time.
const SITE_WRITE_WAYS: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
struct SymbolWriteProof {
    prototype: Value,
    key: Value,
    cell: crate::heap::ChainCell,
}

#[derive(Default)]
pub(super) struct SymbolWriteProofs {
    entries: super::shape_memo::DirectMappedTable<SymbolWriteProof, SYMBOL_WRITE_PROOF_ENTRIES>,
    /// Per lane write site: the write it made last (`SiteSymbolWrite`), so the site's next
    /// write to a receiver of that shape is one compare and the store. Sites map as the
    /// symbol read sites do (`symbol_load_proof::site_index`).
    sites: super::shape_memo::DirectMappedTable<
        SiteSymbolWrite,
        { super::symbol_load_proof::SITE_OWN_READ_ENTRIES * SITE_WRITE_WAYS },
    >,
}

/// What `Vm::set_plain_symbol_property` did for `key` on an extensible receiver of the
/// non-dictionary `shape`, recorded at the write site `site` (a lane record's address), held
/// while `cell` does: the add's chain proof's, or for a replace, which reads no prototype, the
/// cell of no chain. Either step is a fact of the shape, key and cell alone; the site only
/// spreads the entries.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SiteSymbolWrite {
    site: usize,
    shape: u32,
    key: Value,
    cell: crate::heap::ChainCell,
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


impl SymbolWriteProofs {
    fn index(prototype: Value, key: Value) -> usize {
        super::shape_memo::direct_mapped_index((prototype, key), SYMBOL_WRITE_PROOF_ENTRIES)
    }

    /// The cell of a proof for `key` through the chain from `prototype`, where one holds.
    #[inline(always)]
    pub(super) fn proof_cell(&self, prototype: Value, key: Value) -> Option<crate::heap::ChainCell> {
        self.entries
            .get(Self::index(prototype, key))
            .filter(|proof| proof.prototype == prototype && proof.key == key && proof.cell.holds())
            .map(|proof| proof.cell)
    }

    pub(super) fn record(&self, prototype: Value, key: Value, cell: crate::heap::ChainCell) {
        self.entries.set(Self::index(prototype, key), SymbolWriteProof { prototype, key, cell });
    }

    /// The step site `site` last made writing `key` to a receiver of `shape`, while its cell
    /// holds.
    #[inline(always)]
    pub(super) fn site_step(&self, site: usize, shape: u32, key: Value) -> Option<SymbolWriteStep> {
        let first = Self::site_entry(site);
        (first..first + SITE_WRITE_WAYS)
            .filter_map(|index| self.sites.get(index))
            .find(|write| write.site == site && write.shape == shape && write.key == key && write.cell.holds())
            .map(|write| write.step)
    }

    /// Records the step as the site's most recent write, keeping the one before it.
    pub(super) fn record_site_step(&self, site: usize, shape: u32, key: Value, cell: crate::heap::ChainCell, step: SymbolWriteStep) {
        let first = Self::site_entry(site);
        if let Some(previous) = self.sites.get(first) {
            self.sites.set(first + 1, previous);
        }
        self.sites.set(first, SiteSymbolWrite { site, shape, key, cell, step });
    }

    /// The first of the `SITE_WRITE_WAYS` entries of lane site `site`.
    #[inline(always)]
    fn site_entry(site: usize) -> usize {
        super::symbol_load_proof::site_index(site) * SITE_WRITE_WAYS
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
        self.sites.clear();
    }
}
