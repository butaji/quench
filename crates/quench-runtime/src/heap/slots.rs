use std::mem::size_of;

const SLOTS_PER_SLAB: usize = 2_048;
const SLOT_KIND_BITS: u32 = 2;
const SLOT_KIND_MASK: u32 = (1 << SLOT_KIND_BITS) - 1;
const LEGACY_CELL_KIND: u32 = 2;
pub(super) const SLOT_ENTRY_WIDTH_BYTES: usize = size_of::<u32>();
const RECORD_ID_BITS: u32 = u32::BITS - SLOT_KIND_BITS;
pub(super) const MAX_RECORD_COUNT: u32 = 1 << RECORD_ID_BITS;
pub(super) type RecordId = u32;

const _: () = assert!(size_of::<SlotEntry>() == SLOT_ENTRY_WIDTH_BYTES);
const _: () = assert!(MAX_RECORD_COUNT.is_power_of_two());
const _: () = assert!(MAX_RECORD_COUNT - 1 <= u32::MAX >> SLOT_KIND_BITS);
const _: () = assert!(LEGACY_CELL_KIND <= SLOT_KIND_MASK);

#[derive(Default)]
pub(super) struct SlotArena {
    slabs: Vec<Box<[SlotEntry]>>,
    len: usize,
}

/// Stable Value identity mapped to one external, non-moving heap payload.
/// Two low bits distinguish canonical Object records from legacy Cell records.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SlotEntry(u32);

impl SlotEntry {
    pub(super) const fn empty() -> Self {
        Self(0)
    }

    pub(super) fn cell(record_id: RecordId) -> Self {
        Self::tag(record_id, LEGACY_CELL_KIND)
    }

    fn tag(record_id: RecordId, kind: u32) -> Self {
        debug_assert!(
            record_id < MAX_RECORD_COUNT,
            "record locator exceeds the configured id range"
        );
        debug_assert!(kind > 0 && kind <= SLOT_KIND_MASK);
        Self((record_id << SLOT_KIND_BITS) | kind)
    }

    pub(super) const fn is_live(self) -> bool {
        self.0 != 0
    }

    pub(super) fn cell_record_id(self) -> Option<RecordId> {
        self.record_id(LEGACY_CELL_KIND)
    }

    fn record_id(self, kind: u32) -> Option<RecordId> {
        ((self.0 & SLOT_KIND_MASK) == kind).then_some(self.0 >> SLOT_KIND_BITS)
    }

    pub(super) fn clear(&mut self) {
        self.0 = 0;
    }
}

impl SlotArena {
    pub(super) fn with_small_capacity() -> Self {
        Self::default()
    }

    pub(super) fn push(&mut self, slot: SlotEntry) {
        if self.len == self.capacity() {
            let mut slab = Vec::with_capacity(SLOTS_PER_SLAB);
            slab.resize_with(SLOTS_PER_SLAB, SlotEntry::empty);
            self.slabs.push(slab.into_boxed_slice());
        }
        let index = self.len;
        self.len += 1;
        *self.get_mut(index).unwrap() = slot;
    }

    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut SlotEntry> {
        (index < self.len).then(|| &mut self.slabs[index / SLOTS_PER_SLAB][index % SLOTS_PER_SLAB])
    }

    pub(super) fn get(&self, index: usize) -> Option<&SlotEntry> {
        (index < self.len).then(|| &self.slabs[index / SLOTS_PER_SLAB][index % SLOTS_PER_SLAB])
    }

    pub(super) unsafe fn get_unchecked(&self, index: usize) -> &SlotEntry {
        debug_assert!(index < self.len);
        // SAFETY: the caller proves the flat index is live; every slab has the
        // fixed size used by this decomposition.
        unsafe {
            self.slabs
                .get_unchecked(index / SLOTS_PER_SLAB)
                .get_unchecked(index % SLOTS_PER_SLAB)
        }
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn iter(&self) -> impl Iterator<Item = &SlotEntry> {
        self.slabs
            .iter()
            .flat_map(|slab| slab.iter())
            .take(self.len)
    }

    pub(super) fn clear(&mut self) {
        self.slabs.clear();
        self.len = 0;
    }

    pub(super) const fn len(&self) -> usize {
        self.len
    }

    pub(super) fn capacity(&self) -> usize {
        self.slabs.len() * SLOTS_PER_SLAB
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_RECORD_COUNT, SLOT_ENTRY_WIDTH_BYTES, SlotEntry};
    use std::mem::size_of;

    #[test]
    fn slot_entry_is_a_32_bit_tagged_record_id() {
        assert_eq!(size_of::<SlotEntry>(), SLOT_ENTRY_WIDTH_BYTES);
        assert_eq!(SLOT_ENTRY_WIDTH_BYTES, size_of::<u32>());

        let cell = SlotEntry::cell(MAX_RECORD_COUNT - 1);
        assert_eq!(cell.cell_record_id(), Some(MAX_RECORD_COUNT - 1));

        let mut cleared = cell;
        cleared.clear();
        assert!(!cleared.is_live());
        assert_eq!(cleared.cell_record_id(), None);
    }
}
