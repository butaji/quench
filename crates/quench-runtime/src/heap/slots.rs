use super::Slot;

pub(super) const SLOTS_PER_SLAB: usize = 2_048;
const FREE_SLOT_WORD_BITS: usize = u64::BITS as usize;
const FREE_SLOT_WORDS: usize = SLOTS_PER_SLAB.div_ceil(FREE_SLOT_WORD_BITS);

const _: () = assert!(SLOTS_PER_SLAB <= u16::MAX as usize);
const _: () = assert!(FREE_SLOT_WORDS <= u32::BITS as usize);

#[derive(Default)]
pub(super) struct FreeSlotBitmap {
    words: [u64; FREE_SLOT_WORDS],
    nonempty_words: u32,
    free_count: u16,
}

impl FreeSlotBitmap {
    pub(super) fn insert(&mut self, offset: usize) {
        debug_assert!(offset < SLOTS_PER_SLAB);
        let word_index = offset / FREE_SLOT_WORD_BITS;
        let mask = 1_u64 << (offset % FREE_SLOT_WORD_BITS);
        let word = &mut self.words[word_index];
        debug_assert_eq!(*word & mask, 0, "free slot inserted twice");
        if *word == 0 {
            self.nonempty_words |= 1_u32 << word_index;
        }
        *word |= mask;
        self.free_count += 1;
        debug_assert!(self.free_count as usize <= SLOTS_PER_SLAB);
    }

    pub(super) fn pop(&mut self) -> Option<u16> {
        let nonempty_words = self.nonempty_words;
        if nonempty_words == 0 {
            debug_assert_eq!(self.free_count, 0);
            return None;
        }

        let word_index = (u32::BITS - 1 - nonempty_words.leading_zeros()) as usize;
        let word = self.words[word_index];
        debug_assert_ne!(word, 0);
        let bit_index = (u64::BITS - 1 - word.leading_zeros()) as usize;
        let mask = 1_u64 << bit_index;
        let remaining = word & !mask;
        self.words[word_index] = remaining;
        if remaining == 0 {
            self.nonempty_words &= !(1_u32 << word_index);
        }
        self.free_count -= 1;

        let offset = word_index * FREE_SLOT_WORD_BITS + bit_index;
        debug_assert!(offset < SLOTS_PER_SLAB);
        Some(offset as u16)
    }

    pub(super) const fn len(&self) -> usize {
        self.free_count as usize
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.nonempty_words == 0
    }

    pub(super) fn merge(&mut self, other: &Self) {
        debug_assert!(
            self.words
                .iter()
                .zip(other.words.iter())
                .all(|(left, right)| left & right == 0)
        );
        for (word_index, (left, right)) in self.words.iter_mut().zip(other.words.iter()).enumerate() {
            if *right == 0 {
                continue;
            }
            *left |= *right;
            self.nonempty_words |= 1_u32 << word_index;
        }
        self.free_count += other.free_count;
        debug_assert!(self.free_count as usize <= SLOTS_PER_SLAB);
    }
}

#[derive(Default)]
pub(super) struct SlotArena {
    slabs: Vec<Box<[Slot]>>,
    len: usize,
}

impl SlotArena {
    pub(super) fn with_small_capacity() -> Self {
        Self::default()
    }

    pub(super) fn push(&mut self, slot: Slot) {
        if self.len == self.capacity() {
            let mut slab = Vec::with_capacity(SLOTS_PER_SLAB);
            slab.resize_with(SLOTS_PER_SLAB, || Slot { cell: None });
            self.slabs.push(slab.into_boxed_slice());
        }
        let index = self.len;
        self.len += 1;
        *self.get_mut(index).unwrap() = slot;
    }

    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut Slot> {
        (index < self.len).then(|| &mut self.slabs[index / SLOTS_PER_SLAB][index % SLOTS_PER_SLAB])
    }

    pub(super) unsafe fn get_unchecked(&self, index: usize) -> &Slot {
        debug_assert!(index < self.len);
        // SAFETY: the caller proves the flat index is live; every slab has the
        // fixed size used by this decomposition.
        unsafe {
            self.slabs
                .get_unchecked(index / SLOTS_PER_SLAB)
                .get_unchecked(index % SLOTS_PER_SLAB)
        }
    }

    pub(super) unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut Slot {
        debug_assert!(index < self.len);
        // SAFETY: same flat-index invariant as `get_unchecked`, with unique
        // access inherited from `&mut self`.
        unsafe {
            self.slabs
                .get_unchecked_mut(index / SLOTS_PER_SLAB)
                .get_unchecked_mut(index % SLOTS_PER_SLAB)
        }
    }

    #[cfg(any(debug_assertions, test, feature = "profile-memory"))]
    pub(super) fn iter(&self) -> impl Iterator<Item = &Slot> {
        self.slabs
            .iter()
            .flat_map(|slab| slab.iter())
            .take(self.len)
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Slot> {
        let len = self.len;
        self.slabs
            .iter_mut()
            .flat_map(|slab| slab.iter_mut())
            .take(len)
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn fully_empty_page_count(&self, page_size: usize) -> usize {
        self.slabs
            .iter()
            .map(|slab| {
                let slab_start = slab.as_ptr() as usize;
                let slab_end = slab_start + slab.len() * std::mem::size_of::<Slot>();
                let mut page_start = slab_start.div_ceil(page_size) * page_size;
                let mut empty_pages = 0;
                while let Some(page_end) = page_start.checked_add(page_size)
                    && page_end <= slab_end
                {
                    let first_slot = (page_start - slab_start) / std::mem::size_of::<Slot>();
                    let end_slot = (page_end - slab_start).div_ceil(std::mem::size_of::<Slot>());
                    if slab[first_slot..end_slot]
                        .iter()
                        .all(|slot| slot.cell.is_none())
                    {
                        empty_pages += 1;
                    }
                    page_start = page_end;
                }
                empty_pages
            })
            .sum()
    }

    /// Occupied slots grouped by slab, each with the flat index of its first
    /// slot, so whole-heap passes run a plain loop per slab.
    pub(super) fn slabs_mut(&mut self) -> impl Iterator<Item = (usize, &mut [Slot])> {
        let len = self.len;
        self.slabs.iter_mut().enumerate().map(move |(slab, slots)| {
            let base = slab * SLOTS_PER_SLAB;
            let occupied = (len - base).min(SLOTS_PER_SLAB);
            (base, &mut slots[..occupied])
        })
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
    use super::{FreeSlotBitmap, SLOTS_PER_SLAB};

    #[test]
    fn free_slot_bitmap_pops_offsets_in_stack_order() {
        let mut free = FreeSlotBitmap::default();
        let offsets = [0, 63, 64, 1023, 1024, SLOTS_PER_SLAB - 1];
        for offset in offsets {
            free.insert(offset);
        }

        assert_eq!(free.len(), offsets.len());
        for offset in offsets.into_iter().rev() {
            assert_eq!(free.pop(), Some(offset as u16));
        }
        assert!(free.is_empty());
        assert_eq!(free.pop(), None);
    }

    #[test]
    fn free_slot_bitmap_merges_disjoint_sweep_results() {
        let mut early = FreeSlotBitmap::default();
        early.insert(0);
        early.insert(64);

        let mut late = FreeSlotBitmap::default();
        late.insert(SLOTS_PER_SLAB - 1);

        early.merge(&late);
        assert_eq!(early.len(), 3);
        assert_eq!(early.pop(), Some((SLOTS_PER_SLAB - 1) as u16));
        assert_eq!(early.pop(), Some(64));
        assert_eq!(early.pop(), Some(0));
        assert!(early.is_empty());
    }
}
