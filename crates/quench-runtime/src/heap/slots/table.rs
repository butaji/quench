//! The cell slots of targets without an address reservation: slabs behind a table.

use super::super::Slot;
use super::SLOTS_PER_SLAB;

#[cfg(not(unix))]
#[derive(Default)]
pub(super) struct SlotArena {
    slabs: Vec<Box<[Slot]>>,
    len: usize,
}

#[cfg(not(unix))]
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

    #[inline(always)]
    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut Slot> {
        (index < self.len).then(|| &mut self.slabs[index / SLOTS_PER_SLAB][index % SLOTS_PER_SLAB])
    }

    pub(super) fn get_two_mut(
        &mut self,
        first: usize,
        second: usize,
    ) -> Option<(&mut Slot, &mut Slot)> {
        if first == second || first >= self.len || second >= self.len {
            return None;
        }
        let first_slab = first / SLOTS_PER_SLAB;
        let second_slab = second / SLOTS_PER_SLAB;
        let first_offset = first % SLOTS_PER_SLAB;
        let second_offset = second % SLOTS_PER_SLAB;
        if first_slab == second_slab {
            let slab = &mut self.slabs[first_slab];
            if first_offset < second_offset {
                let (before_second, after_second) = slab.split_at_mut(second_offset);
                Some((&mut before_second[first_offset], &mut after_second[0]))
            } else {
                let (before_first, after_first) = slab.split_at_mut(first_offset);
                Some((&mut after_first[0], &mut before_first[second_offset]))
            }
        } else if first_slab < second_slab {
            let (before_second, after_second) = self.slabs.split_at_mut(second_slab);
            Some((
                &mut before_second[first_slab][first_offset],
                &mut after_second[0][second_offset],
            ))
        } else {
            let (before_first, after_first) = self.slabs.split_at_mut(first_slab);
            Some((
                &mut after_first[0][first_offset],
                &mut before_first[second_slab][second_offset],
            ))
        }
    }

    #[inline(always)] pub(super) unsafe fn get_unchecked(&self, index: usize) -> &Slot {
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

    #[inline(always)] pub(super) const fn len(&self) -> usize {
        self.len
    }

    pub(super) fn capacity(&self) -> usize {
        self.slabs.len() * SLOTS_PER_SLAB
    }
}
