use super::Slot;

pub(super) const SLOTS_PER_SLAB: usize = 2_048;

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

    pub(super) fn get(&self, index: usize) -> Option<&Slot> {
        (index < self.len).then(|| &self.slabs[index / SLOTS_PER_SLAB][index % SLOTS_PER_SLAB])
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

    #[cfg(any(test, feature = "profile-memory"))]
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

/// The empty slots below the arena length, one bit each. Allocation takes the lowest set bit
/// at or after a cursor that freeing moves back, so reuse runs forward through memory and the
/// set costs a bit per slot instead of an index per free slot.
#[derive(Default)]
pub(super) struct FreeSlots {
    bits: Vec<u64>,
    /// No word before this one has a set bit.
    cursor: usize,
    count: usize,
}

impl FreeSlots {
    pub(super) const fn len(&self) -> usize {
        self.count
    }

    pub(super) fn clear(&mut self) {
        self.bits.clear();
        self.cursor = 0;
        self.count = 0;
    }

    /// Makes room for every slot below `slots`.
    pub(super) fn cover(&mut self, slots: usize) {
        let words = slots.div_ceil(u64::BITS as usize);
        if self.bits.len() < words {
            self.bits.resize(words, 0);
        }
    }

    pub(super) fn insert(&mut self, index: usize) {
        let word = index / u64::BITS as usize;
        self.cover(index + 1);
        let bit = 1 << (index % u64::BITS as usize);
        debug_assert_eq!(self.bits[word] & bit, 0, "slot freed twice");
        self.bits[word] |= bit;
        self.count += 1;
        self.cursor = self.cursor.min(word);
    }

    /// Records `count` slots whose bits were set through `atomic_bits`.
    pub(super) fn add_inserted(&mut self, count: usize) {
        self.count += count;
        self.cursor = 0;
    }

    /// The bitmap as atomics, for workers that set bits in disjoint words.
    pub(super) fn atomic_bits(&mut self) -> &[std::sync::atomic::AtomicU64] {
        // SAFETY: `AtomicU64` has `u64`'s size and alignment; the exclusive borrow keeps any
        // plain access out while the atomic view lives.
        unsafe {
            &*(std::ptr::from_mut::<[u64]>(self.bits.as_mut_slice())
                as *const [std::sync::atomic::AtomicU64])
        }
    }

    pub(super) fn pop(&mut self) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        while let Some(&word) = self.bits.get(self.cursor) {
            if word != 0 {
                let bit = word.trailing_zeros() as usize;
                self.bits[self.cursor] = word & (word - 1);
                self.count -= 1;
                return Some(self.cursor * u64::BITS as usize + bit);
            }
            self.cursor += 1;
        }
        unreachable!("free slot count exceeds the set bits")
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn memory_bytes(&self) -> usize {
        self.bits.capacity() * size_of::<u64>()
    }
}
