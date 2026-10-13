use super::Slot;

pub(super) const SLOTS_PER_SLAB: usize = 2_048;

/// The heap's cell slots. On Unix one address range reserved for every cell the heap may
/// hold keeps slots contiguous and in place, so a slot is one offset from the base; the
/// range is made accessible a slab at a time as the heap grows, and inaccessible reserved
/// pages are neither resident nor committed. Other targets keep slabs behind a table.
#[cfg(unix)]
pub(super) struct SlotArena {
    /// The reservation's first slot, dangling while there is none: the one load a slot
    /// access needs.
    base: *mut Slot,
    reservation: Option<reservation::Reservation>,
    len: usize,
}

#[cfg(unix)]
mod reservation {
    use super::{SLOTS_PER_SLAB, Slot};
    use std::ptr::NonNull;

    /// An anonymous mapping holding `reserved` slots, the first `accessible` of them readable
    /// and writable.
    pub(super) struct Reservation {
        pub(super) base: NonNull<Slot>,
        pub(super) reserved: usize,
        pub(super) accessible: usize,
    }

    /// The smallest reservation tried when the address space refuses a larger one; a heap
    /// that outgrows the reservation it obtained reaches the cell limit there.
    const MINIMUM_RESERVED_SLOTS: usize = SLOTS_PER_SLAB * 16;

    impl Reservation {
        /// Reserves room for `wanted` slots, halving the request while the system refuses it.
        pub(super) fn new(wanted: usize) -> Self {
            let mut reserved = wanted.next_multiple_of(SLOTS_PER_SLAB);
            loop {
                let bytes = reserved * size_of::<Slot>();
                // SAFETY: an anonymous private mapping with no access touches no existing memory.
                let base = unsafe {
                    libc::mmap(
                        std::ptr::null_mut(),
                        bytes,
                        libc::PROT_NONE,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                        -1,
                        0,
                    )
                };
                if base != libc::MAP_FAILED {
                    return Self {
                        base: NonNull::new(base.cast()).expect("mmap returned a mapping"),
                        reserved,
                        accessible: 0,
                    };
                }
                assert!(
                    reserved > MINIMUM_RESERVED_SLOTS,
                    "heap slot reservation failed"
                );
                reserved = (reserved / 2).next_multiple_of(SLOTS_PER_SLAB);
            }
        }

        /// Makes the next slab readable and writable.
        pub(super) fn extend(&mut self) {
            assert!(self.accessible < self.reserved, "heap cell limit reached");
            let bytes = SLOTS_PER_SLAB * size_of::<Slot>();
            // SAFETY: the slab lies inside the reservation, after every accessible slot.
            let result = unsafe {
                libc::mprotect(
                    self.base.as_ptr().add(self.accessible).cast(),
                    bytes,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
            assert_eq!(result, 0, "heap slot commit failed");
            self.accessible += SLOTS_PER_SLAB;
        }
    }

    impl Drop for Reservation {
        fn drop(&mut self) {
            // SAFETY: the mapping is this reservation's own and no slot outlives it.
            unsafe {
                libc::munmap(self.base.as_ptr().cast(), self.reserved * size_of::<Slot>());
            }
        }
    }
}

#[cfg(unix)]
impl Default for SlotArena {
    fn default() -> Self {
        Self {
            base: std::ptr::NonNull::dangling().as_ptr(),
            reservation: None,
            len: 0,
        }
    }
}

#[cfg(unix)]
impl Drop for SlotArena {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(unix)]
impl SlotArena {
    pub(super) fn with_small_capacity() -> Self {
        Self::default()
    }

    #[inline(always)]
    fn base(&self) -> *mut Slot {
        self.base
    }

    pub(super) fn push(&mut self, slot: Slot) {
        if self.len == self.capacity() {
            let reservation = self
                .reservation
                .get_or_insert_with(|| reservation::Reservation::new(super::MAX_HEAP_CELLS));
            reservation.extend();
            self.base = reservation.base.as_ptr();
        }
        // SAFETY: `extend` made the slot at `len` accessible; it holds no initialized value.
        unsafe { self.base().add(self.len).write(slot) };
        self.len += 1;
    }

    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut Slot> {
        // SAFETY: slots below `len` are initialized and accessible.
        (index < self.len).then(|| unsafe { &mut *self.base().add(index) })
    }

    pub(super) fn get(&self, index: usize) -> Option<&Slot> {
        // SAFETY: as for `get_mut`.
        (index < self.len).then(|| unsafe { &*self.base().add(index) })
    }

    pub(super) unsafe fn get_unchecked(&self, index: usize) -> &Slot {
        debug_assert!(index < self.len);
        // SAFETY: the caller proves the index is below `len`.
        unsafe { &*self.base().add(index) }
    }

    pub(super) unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut Slot {
        debug_assert!(index < self.len);
        // SAFETY: as for `get_unchecked`, with unique access inherited from `&mut self`.
        unsafe { &mut *self.base().add(index) }
    }

    #[cfg(any(test, feature = "profile-memory"))]
    fn occupied(&self) -> &[Slot] {
        // SAFETY: the first `len` slots are initialized and accessible.
        unsafe { std::slice::from_raw_parts(self.base(), self.len) }
    }

    fn occupied_mut(&mut self) -> &mut [Slot] {
        // SAFETY: as for `occupied`, with unique access.
        unsafe { std::slice::from_raw_parts_mut(self.base(), self.len) }
    }

    #[cfg(any(test, feature = "profile-memory"))]
    pub(super) fn iter(&self) -> impl Iterator<Item = &Slot> {
        self.occupied().iter()
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Slot> {
        self.occupied_mut().iter_mut()
    }

    /// Occupied slots grouped by slab, each with the flat index of its first
    /// slot, so whole-heap passes run a plain loop per slab.
    pub(super) fn slabs_mut(&mut self) -> impl Iterator<Item = (usize, &mut [Slot])> {
        self.occupied_mut()
            .chunks_mut(SLOTS_PER_SLAB)
            .enumerate()
            .map(|(slab, slots)| (slab * SLOTS_PER_SLAB, slots))
    }

    pub(super) fn clear(&mut self) {
        let occupied: *mut [Slot] = self.occupied_mut();
        self.len = 0;
        // SAFETY: the slots were initialized and `len` no longer covers them.
        unsafe { std::ptr::drop_in_place(occupied) };
        // Unmapping returns the slots' pages; a later push reserves afresh.
        self.reservation = None;
        self.base = std::ptr::NonNull::dangling().as_ptr();
    }

    pub(super) const fn len(&self) -> usize {
        self.len
    }

    pub(super) fn capacity(&self) -> usize {
        self.reservation
            .as_ref()
            .map_or(0, |reservation| reservation.accessible)
    }
}

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
