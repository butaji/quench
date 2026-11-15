//! Proofs that answer a `[[Get]]` without walking its receiver's chain. A symbol-keyed read
//! (`this[kState]`), and the VM's own reads of named properties (a promise's `constructor`
//! and `then`, an iterator result's `done`), have no compiler cache site, so each read would
//! look the key up in every shape from the receiver to its holder. A proof records, under one
//! `prototype_layout_epoch`, where the walk from a receiver of one shape found `key`: an own
//! data slot, a data slot of a prototype, or nowhere. Every prototype the walk passed was
//! marked a prototype when the proof was made, so any later change to one advances the epoch
//! and retires it; a collection that frees a prototype, and shape compaction, advance it
//! too, so a proof never names a reused prototype record or shape. A key the shape holds
//! keeps its symbol alive. The receiver's own layout is its shape, which the proof names.

use super::{Host, Value, Vm};
use super::property_key::PropertyKey;

/// Direct-mapped entries: a power of two. Node's streams and Fastify's request and reply
/// objects read a few dozen symbol keys per request, and host natives read named keys on
/// many shapes through the same table: at 512 entries, collisions alone missed 12-21% of
/// undici's and Fastify's reads, at 4096 entries 4-10%.
const SYMBOL_LOAD_PROOF_ENTRIES: usize = 1 << 12;

/// Where a proved walk found the key, from a receiver of one layout (`Object::layout_id`:
/// its shape and its prototype link together). The holder of an inherited answer is not
/// kept: it is the object `hops` prototype links up from the receiver, which the chain epoch
/// keeps fixed (`Vm::proof_holder`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SymbolLoadAnswer {
    /// An own data property in this slot of the receiver's shape.
    Own(u32),
    /// A data property in this slot of the holder `hops` links up.
    Inherited { hops: u8, slot: u32 },
    /// No object on the receiver's chain has the key.
    Absent,
    /// An accessor in this slot of the holder `hops` links up: the read calls its getter,
    /// which only the general path does.
    Getter { hops: u8, slot: u32 },
}

/// K-ENTRY (research/nojit/system/contracts/K-ENTRY.txt): one 16-byte entry, the tag word
/// `layout | key32 << 32` and the answer word `slot:24 | hops:4 | kind:4 | epoch:32`. Zero
/// bits are an empty entry (kind 0).
#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct FactEntry {
    tag: u64,
    answer: u64,
}

const KIND_OWN: u64 = 1;
const KIND_INHERITED: u64 = 2;
const KIND_ABSENT: u64 = 3;
const KIND_GETTER: u64 = 5;
/// Slots and hops an entry can name; a walk past either is not recorded.
const SLOT_LIMIT: u32 = 1 << 24;
const HOPS_LIMIT: u8 = 1 << 4;

impl FactEntry {
    #[inline(always)]
    fn tag(layout: u32, key32: u32) -> u64 {
        u64::from(layout) | u64::from(key32) << 32
    }

    fn encode(answer: SymbolLoadAnswer, epoch: u64) -> Option<u64> {
        let (kind, hops, slot) = match answer {
            SymbolLoadAnswer::Own(slot) => (KIND_OWN, 0, slot),
            SymbolLoadAnswer::Inherited { hops, slot } => (KIND_INHERITED, hops, slot),
            SymbolLoadAnswer::Absent => (KIND_ABSENT, 0, 0),
            SymbolLoadAnswer::Getter { hops, slot } => (KIND_GETTER, hops, slot),
        };
        (slot < SLOT_LIMIT && hops < HOPS_LIMIT)
            .then(|| u64::from(slot) | u64::from(hops) << 24 | kind << 28 | (epoch & 0xffff_ffff) << 32)
    }

    /// The answer this entry's word holds while the chain epoch's low half is `epoch32`: an
    /// own answer whatever the epoch, since the layout fixes the receiver's shape.
    #[inline(always)]
    fn decode(word: u64, epoch32: u32) -> Option<SymbolLoadAnswer> {
        let slot = (word & u64::from(SLOT_LIMIT - 1)) as u32;
        let hops = ((word >> 24) & 0xf) as u8;
        let current = (word >> 32) as u32 == epoch32;
        match (word >> 28) & 0xf {
            KIND_OWN => Some(SymbolLoadAnswer::Own(slot)),
            KIND_INHERITED if current => Some(SymbolLoadAnswer::Inherited { hops, slot }),
            KIND_ABSENT if current => Some(SymbolLoadAnswer::Absent),
            KIND_GETTER if current => Some(SymbolLoadAnswer::Getter { hops, slot }),
            _ => None,
        }
    }
}

/// The fact entries, in a zeroed mapping made at the first record, so a program that never
/// records keeps none of its 64 KiB resident.
#[derive(Default)]
struct FactEntries {
    base: std::cell::OnceCell<std::ptr::NonNull<std::cell::Cell<FactEntry>>>,
}

const FACT_BYTES: usize = SYMBOL_LOAD_PROOF_ENTRIES * size_of::<FactEntry>();

impl FactEntries {
    #[inline(always)]
    fn slot(&self, index: usize) -> Option<&std::cell::Cell<FactEntry>> {
        // SAFETY: the mapping holds `SYMBOL_LOAD_PROOF_ENTRIES` entries, zero bits empty ones,
        // and `index` is masked to that range by `SymbolLoadProofs::index`.
        self.base.get().map(|base| unsafe { &*base.as_ptr().add(index) })
    }

    #[cold]
    #[inline(never)]
    fn allocate(&self) -> std::ptr::NonNull<std::cell::Cell<FactEntry>> {
        const { assert!(align_of::<FactEntry>() <= super::site_table::MAPPING_GRANULE_BYTES) };
        *self.base.get_or_init(|| super::site_table::storage::map_zeroed(FACT_BYTES.next_multiple_of(super::site_table::MAPPING_GRANULE_BYTES)).cast())
    }
}

impl Drop for FactEntries {
    fn drop(&mut self) {
        if let Some(base) = self.base.get() {
            // SAFETY: the mapping is this table's, and nothing reads it after the drop.
            unsafe { super::site_table::storage::unmap(base.cast(), FACT_BYTES.next_multiple_of(super::site_table::MAPPING_GRANULE_BYTES)) };
        }
    }
}

#[derive(Default)]
pub(super) struct SymbolLoadProofs {
    entries: FactEntries,
    /// The chain epoch's high half when the table last recorded: a record under another
    /// empties the table first, so an entry's 32-bit epoch never matches a later epoch with
    /// the same low half.
    epoch_high: std::cell::Cell<u32>,
    /// Per read site: the own data slot a proof gave the site's last receiver layout and key
    /// (`SiteOwnRead`), so the site's next read of that pair is one compare.
    sites: super::shape_memo::DirectMappedTable<SiteOwnRead, SITE_OWN_READ_ENTRIES>,
}

/// Direct-mapped site entries: a power of two. A request's symbol-keyed reads run at a few
/// hundred lane records (Node's Readable state reads), most of them monomorphic.
pub(super) const SITE_OWN_READ_ENTRIES: usize = 1 << 9;

/// A site's last own-slot answer: what `SymbolLoadAnswer::Own` said for `key` on a receiver
/// of `layout`, recorded at the read site `site` (a lane record's address). The site only
/// spreads the entries; the answer is a fact of the layout and key alone, which no epoch
/// retires.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SiteOwnRead {
    site: usize,
    layout: u32,
    key: Value,
    slot: u32,
}

impl SymbolLoadProofs {
    #[inline(always)]
    fn index(layout: u32, key32: u32) -> usize {
        ((layout ^ key32.rotate_left(16)).wrapping_mul(0x9E37_79B1) >> 20) as usize
    }

    /// The answer a proof for `key` on a receiver of `layout` holds under the chain epoch `epoch`.
    #[inline(always)]
    pub(super) fn answer(&self, layout: u32, key: PropertyKey, epoch: u64) -> Option<SymbolLoadAnswer> {
        let key32 = key.key32()?;
        let entry = self.entries.slot(Self::index(layout, key32))?.get();
        if entry.tag != FactEntry::tag(layout, key32) {
            return None;
        }
        FactEntry::decode(entry.answer, epoch as u32)
    }

    /// The own data slot a proof gives `key32` on a receiver of `layout`: the hit path every
    /// generic load tries first, one tag compare and one kind compare.
    #[inline(always)]
    pub(super) fn own_slot(&self, layout: u32, key32: u32) -> Option<u32> {
        let entry = self.entries.slot(Self::index(layout, key32))?.get();
        (entry.tag == FactEntry::tag(layout, key32) && (entry.answer >> 28) & 0xf == KIND_OWN)
            .then_some((entry.answer & u64::from(SLOT_LIMIT - 1)) as u32)
    }

    pub(super) fn record(&self, layout: u32, key: PropertyKey, epoch: u64, answer: SymbolLoadAnswer) {
        let (Some(key32), Some(word)) = (key.key32(), FactEntry::encode(answer, epoch)) else {
            return;
        };
        let high = (epoch >> 32) as u32;
        if high != self.epoch_high.get() {
            self.clear_entries();
            self.epoch_high.set(high);
        }
        let base = match self.entries.base.get() {
            Some(base) => *base,
            None => self.entries.allocate(),
        };
        // SAFETY: `index` is below the mapping's entry count.
        unsafe { &*base.as_ptr().add(Self::index(layout, key32)) }.set(FactEntry { tag: FactEntry::tag(layout, key32), answer: word });
    }

    /// The own data slot site `site` last read `key` from on a receiver of `layout`.
    #[inline(always)]
    pub(super) fn site_own_slot(&self, site: usize, layout: u32, key: Value) -> Option<u32> {
        self.sites
            .get(site_index(site))
            .filter(|read| read.site == site && read.layout == layout && read.key == key)
            .map(|read| read.slot)
    }

    /// Records at `site` the own slot a proof gives `property` on a receiver of `layout`, if it
    /// gives one, for reads whose key value is `key`.
    pub(super) fn record_site_own_slot(&self, site: usize, layout: u32, key: Value, property: PropertyKey, epoch: u64) {
        if let Some(SymbolLoadAnswer::Own(slot)) = self.answer(layout, property, epoch) {
            self.sites.set(site_index(site), SiteOwnRead { site, layout, key, slot });
        }
    }

    fn clear_entries(&self) {
        if let Some(base) = self.entries.base.get() {
            for index in 0..SYMBOL_LOAD_PROOF_ENTRIES {
                // SAFETY: `index` is below the mapping's entry count.
                unsafe { &*base.as_ptr().add(index) }.set(FactEntry { tag: 0, answer: 0 });
            }
        }
    }

    pub(super) fn clear(&self) {
        self.clear_entries();
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

impl<H: Host> Vm<H> {
    /// The getter a symbol-keyed read of `key` on `object` calls, where a proof records it
    /// (`SymbolLoadAnswer::Getter`).
    pub(super) fn proved_symbol_getter(&self, object: Value, key: Value) -> Option<Value> {
        self.proved_getter(object, PropertyKey::symbol(key))
    }

    /// The getter a read of `key` on `object` calls, where a proof records it.
    pub(super) fn proved_getter(&self, object: Value, key: PropertyKey) -> Option<Value> {
        let data = self.heap.ordinary_object(object)?;
        let epoch = crate::heap::prototype_layout_epoch();
        match self.symbol_load_proofs.answer(data.layout_id(), key, epoch)? {
            SymbolLoadAnswer::Getter { hops, slot } if !data.has_side_descriptors() => {
                let holder = self.proof_holder(data, hops)?;
                self.slot_attributes(holder, slot as usize)?.getter
            }
            _ => None,
        }
    }

    /// The holder of an inherited proof answer: the object `hops` prototype links up from the
    /// receiver record `data`. The chain epoch the answer was checked under fixes every link.
    #[inline(always)]
    pub(super) fn proof_holder<'a>(&'a self, data: &'a crate::heap::Object, hops: u8) -> Option<&'a crate::heap::Object> {
        let mut link = data;
        for _ in 0..hops {
            link = self.heap.object_record(link.proto())?;
        }
        Some(link)
    }
}
