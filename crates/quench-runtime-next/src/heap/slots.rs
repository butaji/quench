use super::Slot;

const SLOTS_PER_SLAB: usize = 2_048;

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

    #[cfg(feature = "profile-memory")]
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

    #[cfg(feature = "profile-memory")]
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
