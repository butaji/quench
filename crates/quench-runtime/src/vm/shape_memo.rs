//! Memoized answers about non-dictionary shapes. Such a shape never changes once created, so
//! what it says about a key or a slot is a fact of the pair: the memo answers a repeated
//! question with one probe instead of a walk down the shape's transition chain. A symbol key
//! a live shape names is kept alive by that shape (`append_shape_roots`), so no entry can be
//! reached through a reused symbol. Entries are dropped whenever shape ids or atoms are
//! reassigned (shape compaction, VM reset).

use std::cell::{Cell, OnceCell};
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::hash::{Hash, Hasher};

/// Direct-mapped entries: a power of two. Memory-first (TOD-123): 1024 entries, a quarter
/// of the 4096 sized for the Node HTTP stack's working set.
const SHAPE_MEMO_ENTRIES: usize = 1 << 10;

/// A direct-mapped memo of `ENTRIES` entries (a power of two) that allocates them on its
/// first record. A memo a program never records into costs no memory: built at VM
/// construction, the memos cost every run 48 to 254 KiB of peak RSS on V8-v7 whether or
/// not it used private names, symbol keys or enumeration. The entries are fresh zero pages
/// (`site_table::storage`) whose zero bits are empty entries, so only a page an entry was
/// recorded on becomes resident: writing every entry empty had made the whole table
/// resident at its first record (the symbol-load proofs' 224 KiB on every V8-v7 row).
pub(super) struct DirectMappedTable<T: Copy, const ENTRIES: usize> {
    entries: OnceCell<ZeroedEntries<T>>,
}

/// One entry: empty while `full` is false, which is its zero bits.
struct Entry<T: Copy> {
    full: Cell<bool>,
    value: Cell<MaybeUninit<T>>,
}

/// A table's entries, in a zeroed mapping it owns.
struct ZeroedEntries<T: Copy> {
    base: NonNull<Entry<T>>,
    len: usize,
}

impl<T: Copy> ZeroedEntries<T> {
    fn bytes(len: usize) -> usize {
        (len * size_of::<Entry<T>>()).next_multiple_of(super::site_table::MAPPING_GRANULE_BYTES)
    }

    fn new(len: usize) -> Self {
        const { assert!(align_of::<Entry<T>>() <= super::site_table::MAPPING_GRANULE_BYTES) };
        let base = super::site_table::storage::map_zeroed(Self::bytes(len)).cast();
        Self { base, len }
    }

    #[inline(always)]
    fn entries(&self) -> &[Entry<T>] {
        // SAFETY: the mapping holds `len` entries, and zero bits are a valid (empty) entry.
        unsafe { std::slice::from_raw_parts(self.base.as_ptr(), self.len) }
    }
}

impl<T: Copy> Drop for ZeroedEntries<T> {
    fn drop(&mut self) {
        // SAFETY: the mapping is this table's, and nothing reads it after the drop.
        unsafe { super::site_table::storage::unmap(self.base.cast(), Self::bytes(self.len)) };
    }
}

// SAFETY: the entries own their mapping as a `Box` owns its slice.
unsafe impl<T: Copy + Send> Send for ZeroedEntries<T> {}

impl<T: Copy, const ENTRIES: usize> Default for DirectMappedTable<T, ENTRIES> {
    fn default() -> Self {
        Self { entries: OnceCell::new() }
    }
}

impl<T: Copy, const ENTRIES: usize> DirectMappedTable<T, ENTRIES> {
    const POWER_OF_TWO: () = assert!(ENTRIES.is_power_of_two() && ENTRIES > 1);

    /// The entry at `index`, if one was recorded since the last clear.
    #[inline(always)]
    pub(super) fn get(&self, index: usize) -> Option<T> {
        let entry = &self.entries.get()?.entries()[index];
        // SAFETY: a full entry holds the value `set` wrote.
        entry.full.get().then(|| unsafe { entry.value.get().assume_init() })
    }

    /// Records `value` at `index`, replacing what the entry held.
    #[inline(always)]
    pub(super) fn set(&self, index: usize, value: T) {
        let entries = match self.entries.get() {
            Some(entries) => entries,
            None => self.allocate(),
        };
        let entry = &entries.entries()[index];
        entry.value.set(MaybeUninit::new(value));
        entry.full.set(true);
    }

    /// The first record's allocation, kept out of line so the record and probe paths that
    /// inline `set` and `get` stay as small as the eager table's: inlined, it cost DeltaBlue
    /// `--jit` about 8%.
    #[cold]
    #[inline(never)]
    fn allocate(&self) -> &ZeroedEntries<T> {
        let () = Self::POWER_OF_TWO;
        self.entries.get_or_init(|| ZeroedEntries::new(ENTRIES))
    }

    pub(super) fn clear(&self) {
        self.retain(|_| false);
    }

    /// Keeps the recorded entries `keep` accepts and empties the rest.
    pub(super) fn retain(&self, keep: impl Fn(&T) -> bool) {
        for entry in self.entries.get().into_iter().flat_map(ZeroedEntries::entries) {
            // SAFETY: a full entry holds the value `set` wrote.
            if entry.full.get() && !keep(&unsafe { entry.value.get().assume_init() }) {
                entry.full.set(false);
            }
        }
    }
}

/// One question kind about shapes: `K` names what is asked of a shape, `V` is the answer.
pub(super) struct ShapeMemo<K: Copy + Eq + Hash, V: Copy> {
    entries: DirectMappedTable<(u32, K, V), SHAPE_MEMO_ENTRIES>,
}

/// The slot a property key has in a shape, or `None` for its absence.
pub(super) type ShapeSlotMemo = ShapeMemo<super::property_key::PropertyKey, Option<u32>>;
/// The attributes a slot has in a shape, as its transition chain gives them.
pub(super) type ShapeAttributeMemo = ShapeMemo<u32, Option<super::ShapeAttributes>>;
/// The shape an empty object of a shape reaches by a plain copy (`Vm::copy_plain_properties`)
/// of a source of the keyed shape: its enumerable data properties added in order.
pub(super) type PlainCopyShapeMemo = ShapeMemo<u32, u32>;
/// What `PlainAssignShapeMemo` answers for a target: the shape the copy reached, the cell of
/// the target prototype's chain it was proved over (`heap::ChainCell`), and, for a source whose contiguous slot words
/// encode as the reached shape's (`Heap::slot_encodings_match`), the owner epoch that proof holds
/// under (`words`), so the words may be copied as they are.
#[derive(Clone, Copy)]
pub(super) struct PlainAssignShape {
    /// The shape the copy reached.
    pub(super) shape: u32,
    /// The cell of the target prototype's chain the copy proved intercepts none of the keys.
    pub(super) cell: crate::heap::ChainCell,
    /// The owner epoch under which the source's words are the reached shape's words, if proved.
    pub(super) words: Option<u64>,
}

/// `PlainCopyShapeMemo` for `Set` (`Object.assign`): keyed also by the target's prototype,
/// whose chain the copy proved intercepts none of the keys, and valid while that chain's cell
/// holds (`heap::ChainCell`).
pub(super) type PlainAssignShapeMemo = ShapeMemo<(u32, super::Value), PlainAssignShape>;

impl<K: Copy + Eq + Hash, V: Copy> Default for ShapeMemo<K, V> {
    fn default() -> Self {
        Self { entries: DirectMappedTable::default() }
    }
}

/// The entry `key` maps to in a direct-mapped table of `entries`, a power of two above one:
/// the top bits of its Fx hash. An Fx hash's low bits depend only on its input's low bits,
/// so masking them would crowd keys built from small shape ids and heap indices into a few
/// entries, which then evict each other on every use.
#[inline(always)]
pub(super) fn direct_mapped_index(key: impl Hash, entries: usize) -> usize {
    debug_assert!(entries.is_power_of_two() && entries > 1);
    direct_mapped_index_by_shift(key, direct_mapped_shift(entries))
}

/// The shift that keeps the top bits of a hash naming one of `entries`
/// (`direct_mapped_index`), for a table that derives it once for its size.
#[inline(always)]
pub(super) const fn direct_mapped_shift(entries: usize) -> u32 {
    u64::BITS - entries.trailing_zeros()
}

/// `direct_mapped_index` of a table whose size `shift` names (`direct_mapped_shift`).
#[inline(always)]
pub(super) fn direct_mapped_index_by_shift(key: impl Hash, shift: u32) -> usize {
    let mut hasher = rustc_hash::FxHasher::default();
    key.hash(&mut hasher);
    (hasher.finish() >> shift) as usize
}

impl<K: Copy + Eq + Hash, V: Copy> ShapeMemo<K, V> {
    #[inline(always)]
    fn index(shape: u32, key: K) -> usize {
        direct_mapped_index((shape, key), SHAPE_MEMO_ENTRIES)
    }

    /// The memoized answer for `key` in `shape`, if one is recorded.
    #[inline(always)]
    pub(super) fn get(&self, shape: u32, key: K) -> Option<V> {
        match self.entries.get(Self::index(shape, key)) {
            Some((recorded, recorded_key, value)) if recorded == shape && recorded_key == key => {
                Some(value)
            }
            _ => None,
        }
    }

    pub(super) fn record(&self, shape: u32, key: K, value: V) {
        self.entries.set(Self::index(shape, key), (shape, key, value));
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
    }
}

impl super::Vm {
    /// Whether `shape` holds array index `index` as a key: only a shape whose memo says it has
    /// an index key (`shape_has_index_key`) is asked for the index's canonical atom.
    pub(super) fn shape_names_index(&self, shape: u32, index: usize) -> bool {
        self.shape_has_index_key(shape)
            && self.lookup_array_index_atom(index).is_some_and(|atom| self.shape_slot(shape, atom).is_some())
    }
}
