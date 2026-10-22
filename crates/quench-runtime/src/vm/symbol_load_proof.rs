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
    entries: super::shape_memo::DirectMappedTable<SymbolLoadProof, SYMBOL_LOAD_PROOF_ENTRIES>,
    /// Per read site: the own data slot a proof gave the site's last receiver shape and key
    /// (`SiteOwnRead`), so the site's next read of that pair is one compare.
    sites: super::shape_memo::DirectMappedTable<SiteOwnRead, SITE_OWN_READ_ENTRIES>,
}

/// Direct-mapped site entries: a power of two. A request's symbol-keyed reads run at a few
/// hundred lane records (Node's Readable state reads), most of them monomorphic.
pub(super) const SITE_OWN_READ_ENTRIES: usize = 1 << 9;

/// A site's last own-slot answer: what `SymbolLoadAnswer::Own` said for `key` on a receiver
/// of `shape` under `epoch`, recorded at the read site `site` (a lane record's address).
/// The site only spreads the entries; the answer is a fact of the shape and key alone.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SiteOwnRead {
    site: usize,
    shape: u32,
    key: Value,
    epoch: u64,
    slot: u32,
}

impl Default for SymbolLoadProofs {
    fn default() -> Self {
        Self { entries: Default::default(), sites: Default::default() }
    }
}

impl SymbolLoadProofs {
    fn index(shape: u32, key: PropertyKey) -> usize {
        super::shape_memo::direct_mapped_index((shape, key), SYMBOL_LOAD_PROOF_ENTRIES)
    }

    /// The answer a proof for `key` on a receiver of `shape` holds under `epoch`.
    #[inline(always)]
    pub(super) fn answer(&self, shape: u32, key: PropertyKey, epoch: u64) -> Option<SymbolLoadAnswer> {
        self.entries
            .get(Self::index(shape, key))
            .filter(|proof| proof.shape == shape && proof.key == key && proof.epoch == epoch)
            .map(|proof| proof.answer)
    }

    pub(super) fn record(&self, shape: u32, key: PropertyKey, epoch: u64, answer: SymbolLoadAnswer) {
        self.entries.set(Self::index(shape, key), SymbolLoadProof { shape, key, epoch, answer });
    }

    /// The own data slot site `site` last read `key` from on a receiver of `shape`, while
    /// `epoch` holds.
    #[inline(always)]
    pub(super) fn site_own_slot(&self, site: usize, shape: u32, key: Value, epoch: u64) -> Option<u32> {
        self.sites
            .get(site_index(site))
            .filter(|read| read.site == site && read.shape == shape && read.key == key && read.epoch == epoch)
            .map(|read| read.slot)
    }

    /// Records at `site` the own slot a proof gives `key` on a receiver of `shape`, if it
    /// gives one under `epoch`.
    pub(super) fn record_site_own_slot(&self, site: usize, shape: u32, key: Value, epoch: u64) {
        if let Some(SymbolLoadAnswer::Own(slot)) = self.answer(shape, PropertyKey::symbol(key), epoch) {
            self.sites.set(site_index(site), SiteOwnRead { site, shape, key, epoch, slot });
        }
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
        self.sites.clear();
    }
}

/// Lane records are at least this many bytes apart (a handler pointer and an operand word),
/// so a site's address shifted by it spreads neighbouring sites over the entries.
const SITE_ADDRESS_SHIFT: u32 = std::mem::size_of::<usize>().trailing_zeros() + 1;

/// The entry of a `SITE_OWN_READ_ENTRIES`-entry site table that lane site `site` (a record's
/// address) maps to: its address past the alignment of a record.
pub(super) fn site_index(site: usize) -> usize {
    (site >> SITE_ADDRESS_SHIFT) & (SITE_OWN_READ_ENTRIES - 1)
}
