//! Proofs that answer a `[[Get]]` without walking its receiver's chain. A symbol-keyed read
//! (`this[kState]`), and the VM's own reads of named properties (a promise's `constructor`
//! and `then`, an iterator result's `done`), have no compiler cache site, so each read would
//! look the key up in every shape from the receiver to its holder. A proof records where the
//! walk from a receiver of one layout found `key`: an own data slot, a data slot or accessor
//! of a prototype, or nowhere. Every prototype the walk passed was marked a prototype, and
//! the proof's entry is held under their subjects (`heap::Dependencies::prove_fact`): a
//! change to one, or its death, clears the entry, and shape compaction clears the table, so
//! a proof never names a changed chain or a reused record or shape, and a use compares no
//! epoch. A key the shape holds keeps its symbol alive.

use super::{Value, Vm};
use super::property_key::PropertyKey;

/// Direct-mapped entries: a power of two. Node's streams and Fastify's request and reply
/// objects read a few dozen symbol keys per request, and host natives read named keys on
/// many shapes through the same table: at 512 entries, collisions alone missed 12-21% of
/// undici's and Fastify's reads, at 4096 entries 4-10%.
const SYMBOL_LOAD_PROOF_ENTRIES: usize = 1 << 12;

/// Where a proved walk found the key, from a receiver of one layout (`Object::layout_id`:
/// its shape and its prototype link together). The holder of an inherited answer is not
/// kept: it is the object `hops` prototype links up from the receiver, which the entry's
/// subjects keep fixed (`Vm::proof_holder`).
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
/// `layout | key32 << 32` and the answer word `slot:24 | hops:4 | kind:4`. Zero bits are an
/// empty entry (kind 0). An inherited, absent or getter answer holds while every prototype
/// its walk passed keeps its layout: the walk's subjects list the entry, and a change to any
/// of them clears its tag (`heap::FactStore`).
#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct FactEntry {
    tag: u64,
    answer: u64,
}

pub(super) const KIND_OWN: u64 = 1;
pub(super) const KIND_INHERITED: u64 = 2;
pub(super) const KIND_ABSENT: u64 = 3;
/// The receiver's own shape lacks the key; nothing said of its chain, so no read answers by it.
pub(super) const KIND_ABSENT_OWN: u64 = 4;
/// An accessor `hops` links up (0: the receiver's own), whose getter and setter the holder's
/// slot attributes give.
pub(super) const KIND_GETTER: u64 = 5;
/// `KIND_GETTER` whose slot holds the getter itself (an accessor without a setter).
pub(super) const KIND_GETTER_SLOT: u64 = 6;
/// `KIND_GETTER` whose slot holds an accessor pair cell.
pub(super) const KIND_GETTER_PAIR: u64 = 7;

/// The parts of an answer word (`FactEntry`): its kind, slot and hops.
#[derive(Clone, Copy)]
pub(super) struct FactWord(u64);

impl FactWord {
    #[inline(always)]
    pub(super) fn kind(self) -> u64 {
        (self.0 >> 28) & 0xf
    }
    #[inline(always)]
    pub(super) fn slot(self) -> usize {
        (self.0 & u64::from(SLOT_LIMIT - 1)) as usize
    }
    #[inline(always)]
    pub(super) fn hops(self) -> u8 {
        ((self.0 >> 24) & 0xf) as u8
    }
}
/// Slots and hops an entry can name; a walk past either is not recorded.
const SLOT_LIMIT: u32 = 1 << 24;
const HOPS_LIMIT: u8 = 1 << 4;

impl FactEntry {
    #[inline(always)]
    fn tag(layout: u32, key32: u32) -> u64 {
        u64::from(layout) | u64::from(key32) << 32
    }

    fn encode(answer: SymbolLoadAnswer) -> Option<u64> {
        let (kind, hops, slot) = match answer {
            SymbolLoadAnswer::Own(slot) => (KIND_OWN, 0, slot),
            SymbolLoadAnswer::Inherited { hops, slot } => (KIND_INHERITED, hops, slot),
            SymbolLoadAnswer::Absent => (KIND_ABSENT, 0, 0),
            SymbolLoadAnswer::Getter { hops, slot } => (KIND_GETTER, hops, slot),
        };
        (slot < SLOT_LIMIT && hops < HOPS_LIMIT)
            .then(|| u64::from(slot) | u64::from(hops) << 24 | kind << 28)
    }

    /// The answer this entry's word holds.
    #[inline(always)]
    fn decode(word: u64) -> Option<SymbolLoadAnswer> {
        let slot = (word & u64::from(SLOT_LIMIT - 1)) as u32;
        let hops = ((word >> 24) & 0xf) as u8;
        match (word >> 28) & 0xf {
            KIND_OWN => Some(SymbolLoadAnswer::Own(slot)),
            KIND_INHERITED => Some(SymbolLoadAnswer::Inherited { hops, slot }),
            KIND_ABSENT => Some(SymbolLoadAnswer::Absent),
            KIND_GETTER | KIND_GETTER_SLOT | KIND_GETTER_PAIR => Some(SymbolLoadAnswer::Getter { hops, slot }),
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

/// The entries, then one `u32` per entry: the record id of an inherited or getter answer's
/// holder (`SymbolLoadProofs::holder`), so a hit names its holder without walking `hops`
/// links. The subjects that keep the answer keep its holder, as they keep its hops.
const FACT_BYTES: usize = SYMBOL_LOAD_PROOF_ENTRIES * (size_of::<FactEntry>() + size_of::<u32>());

impl FactEntries {
    #[inline(always)]
    fn slot(&self, index: usize) -> Option<&std::cell::Cell<FactEntry>> {
        // SAFETY: the mapping holds `SYMBOL_LOAD_PROOF_ENTRIES` entries, zero bits empty ones,
        // and `index` is masked to that range by `SymbolLoadProofs::index`.
        self.base.get().map(|base| unsafe { &*base.as_ptr().add(index) })
    }

    /// The holder word of entry `index`, beside the entries in the same mapping.
    #[inline(always)]
    fn holder(&self, index: usize) -> Option<&std::cell::Cell<u32>> {
        // SAFETY: the holder words follow the `SYMBOL_LOAD_PROOF_ENTRIES` entries in the mapping.
        self.base.get().map(|base| unsafe { &*base.as_ptr().add(SYMBOL_LOAD_PROOF_ENTRIES).cast::<std::cell::Cell<u32>>().add(index) })
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

    /// The answer a proof for `key` on a receiver of `layout` holds.
    #[inline(always)]
    pub(super) fn answer(&self, layout: u32, key: PropertyKey) -> Option<SymbolLoadAnswer> {
        let key32 = key.key32()?;
        let entry = self.entries.slot(Self::index(layout, key32))?.get();
        if entry.tag != FactEntry::tag(layout, key32) {
            return None;
        }
        FactEntry::decode(entry.answer)
    }

    /// The answer word of the entry for `key32` on a receiver of `layout`: the one probe a
    /// generic load makes, an own slot answered from it at once.
    #[inline(always)]
    pub(super) fn word(&self, layout: u32, key32: u32) -> Option<FactWord> {
        let entry = self.entries.slot(Self::index(layout, key32))?.get();
        (entry.tag == FactEntry::tag(layout, key32)).then_some(FactWord(entry.answer))
    }

    /// The record id of the holder the entry for `key32` on `layout` names, where its answer
    /// (checked by the caller) is an inherited one or a getter.
    #[inline(always)]
    pub(super) fn holder(&self, layout: u32, key32: u32) -> Option<u32> {
        self.entries.holder(Self::index(layout, key32)).map(std::cell::Cell::get)
    }

    /// Records `answer` for `key` on receivers of `layout`, with its holder's record id plus
    /// one (0 for none), and the entry it took: a caller whose answer read prototypes holds
    /// the entry under their subjects (`heap::Dependencies::prove_fact`).
    pub(super) fn record_with_holder(&self, layout: u32, key: PropertyKey, answer: SymbolLoadAnswer, holder: u32) -> Option<u32> {
        let (key32, word) = (key.key32()?, FactEntry::encode(answer)?);
        Some(self.store(layout, key32, word, holder))
    }

    /// Records an accessor `hops` links up at `slot`, of the accessor kind `kind`
    /// (`KIND_GETTER`, `KIND_GETTER_SLOT` or `KIND_GETTER_PAIR`), with its holder's record id
    /// plus one (0 for none), and the entry it took.
    pub(super) fn record_accessor(&self, layout: u32, key32: u32, hops: u8, slot: u32, kind: u64, holder: u32) -> Option<u32> {
        (slot < SLOT_LIMIT && hops < HOPS_LIMIT).then(|| self.store(layout, key32, u64::from(slot) | u64::from(hops) << 24 | kind << 28, holder))
    }

    fn store(&self, layout: u32, key32: u32, word: u64, holder: u32) -> u32 {
        let base = match self.entries.base.get() {
            Some(base) => *base,
            None => self.entries.allocate(),
        };
        let index = Self::index(layout, key32);
        // SAFETY: `index` is below the mapping's entry count.
        unsafe { &*base.as_ptr().add(index) }.set(FactEntry { tag: FactEntry::tag(layout, key32), answer: word });
        if let Some(cell) = self.entries.holder(index) {
            cell.set(holder);
        }
        index as u32
    }

    /// The entries as the heap's dependency table clears them, once a record mapped them.
    pub(super) fn store_handle(&self) -> Option<crate::heap::FactStore> {
        const { assert!(size_of::<FactEntry>() == size_of::<[std::cell::Cell<u64>; 2]>()) };
        self.entries.base.get().map(|base| crate::heap::FactStore::new(base.cast(), SYMBOL_LOAD_PROOF_ENTRIES as u32))
    }

    /// Records that a receiver of `layout` lacks `key32` among its own names, where no entry
    /// for the pair says more (any other answer implies it).
    pub(super) fn record_absent_own(&self, layout: u32, key32: u32) {
        let base = match self.entries.base.get() {
            Some(base) => *base,
            None => self.entries.allocate(),
        };
        let index = Self::index(layout, key32);
        let tag = FactEntry::tag(layout, key32);
        // SAFETY: `index` is below the mapping's entry count.
        let entry = unsafe { &*base.as_ptr().add(index) };
        if entry.get().tag != tag {
            entry.set(FactEntry { tag, answer: KIND_ABSENT_OWN << 28 });
        }
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
    pub(super) fn record_site_own_slot(&self, site: usize, layout: u32, key: Value, property: PropertyKey) {
        if let Some(SymbolLoadAnswer::Own(slot)) = self.answer(layout, property) {
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

impl Vm {
    /// The getter a symbol-keyed read of `key` on `object` calls, where a proof records it
    /// (`SymbolLoadAnswer::Getter`).
    pub fn proved_symbol_getter(&self, object: Value, key: Value) -> Option<Value> {
        self.proved_getter(object, PropertyKey::symbol(key))
    }

    /// The getter a read of `key` on `object` calls, where a proof records it: the accessor
    /// fact decoder (`Vm::fact_accessor`) answers it for the receiver's layout.
    pub(super) fn proved_getter(&self, object: Value, key: PropertyKey) -> Option<Value> {
        let data = self.heap.ordinary_object(object)?;
        self.fact_accessor(data, key, false).flatten()
    }

    /// The holder of an inherited proof answer: the object `hops` prototype links up from the
    /// receiver record `data`. The subjects the answer's entry is held under fix every link,
    /// which may be any object whose shape answers the key (`Vm::prove_load`), an array's
    /// among them.
    #[inline(always)]
    pub(super) fn proof_holder<'a>(&'a self, data: &'a crate::heap::Object, hops: u8) -> Option<&'a crate::heap::Object> {
        let mut link = data;
        for _ in 0..hops {
            link = self.heap.object_ref(link.proto())?.record();
        }
        Some(link)
    }
}
