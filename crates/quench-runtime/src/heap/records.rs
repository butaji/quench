use super::{ArrayRecord, Object};
use std::alloc::Layout;
use std::alloc::{alloc, dealloc};
use std::mem::MaybeUninit;
#[cfg(feature = "profile-memory")]
use std::mem::size_of;
use std::ptr::NonNull;

pub(super) type RecordId = u32;

pub(super) type ObjectRecordSpace = RecordSpace<Object>;
pub(super) type ArrayRecordSpace = RecordSpace<ArrayRecord>;

const RECORDS_PER_BLOCK: usize = 1_024;
const OCCUPANCY_WORD_BITS: usize = u64::BITS as usize;
const RECORD_SPACE_CAPACITY: usize = 1 << 24;

const _: () = assert!(RECORDS_PER_BLOCK.is_power_of_two());
const _: () = assert!(RECORD_SPACE_CAPACITY.is_power_of_two());
const _: () = assert!(RECORD_SPACE_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));
const _: () = assert!(RECORD_SPACE_CAPACITY <= u32::MAX as usize);

/// Each record family owns one fixed address range. Block metadata controls
/// allocation and sweep, but resolving a live locator never walks it.
pub(super) struct RecordSpace<T> {
    base: NonNull<MaybeUninit<T>>,
    reserved: bool,
    reservation_failed: bool,
    blocks: Vec<RecordBlock>,
    next_id: u32,
    free: Vec<u32>,
    live: usize,
}

impl<T> Default for RecordSpace<T> {
    fn default() -> Self {
        Self {
            base: NonNull::dangling(),
            reserved: false,
            reservation_failed: false,
            blocks: Vec::new(),
            next_id: 0,
            free: Vec::new(),
            live: 0,
        }
    }
}

struct RecordBlock {
    occupied: [u64; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
    marked: [u64; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
}

pub(super) enum RecordSweep<'a, T> {
    Keep(RecordId, &'a T),
    Dead(RecordId, T),
}

#[cfg(feature = "profile-memory")]
#[derive(Clone, Copy)]
pub(crate) struct RecordSpaceMemory {
    pub(crate) high_water: usize,
    pub(crate) live_records: usize,
    pub(crate) record_bytes: usize,
    pub(crate) block_metadata_bytes: usize,
    pub(crate) free_ids: usize,
    pub(crate) free_id_capacity: usize,
}

impl RecordBlock {
    fn new() -> Self {
        Self {
            occupied: [0; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
            marked: [0; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
        }
    }

    fn claim(&mut self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        let mask = 1_u64 << bit;
        let word = &mut self.occupied[word_index];
        if *word & mask != 0 {
            return false;
        }
        *word |= mask;
        true
    }

    fn is_occupied(&self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        self.occupied[word_index] & (1_u64 << bit) != 0
    }

    #[cfg(test)]
    fn release(&mut self, index: usize) {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        debug_assert!(self.is_occupied(index));
        self.occupied[word_index] &= !(1_u64 << bit);
    }

    fn mark(&mut self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        let mask = 1_u64 << bit;
        if self.marked[word_index] & mask != 0 {
            return false;
        }
        debug_assert!(self.occupied[word_index] & mask != 0);
        self.marked[word_index] |= mask;
        true
    }

    fn is_marked(&self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        self.marked[word_index] & (1_u64 << bit) != 0
    }
}

impl<T> RecordSpace<T> {
    /// Allocates a dense identity in this record space. The family's Value
    /// tag carries this ID directly, independent of legacy Cell slot allocation.
    pub(super) fn try_insert(&mut self, value: T) -> Result<RecordId, T> {
        if !self.ensure_reserved() {
            return Err(value);
        }
        let reused = self.free.last().copied();
        let id = reused.unwrap_or(self.next_id);
        if id as usize >= RECORD_SPACE_CAPACITY {
            return Err(value);
        }
        let (block_index, record_index) = Self::split_id(id);
        while self.blocks.len() <= block_index {
            self.blocks.push(RecordBlock::new());
        }
        if !self.blocks[block_index].claim(record_index) {
            return Err(value);
        }
        // SAFETY: the occupancy bit claims this previously uninitialized id.
        unsafe {
            self.base
                .as_ptr()
                .add(id as usize)
                .write(MaybeUninit::new(value));
        };
        if reused.is_some() {
            self.free.pop();
        } else {
            self.next_id += 1;
        }
        self.live += 1;
        Ok(id)
    }

    fn ensure_reserved(&mut self) -> bool {
        if self.reserved {
            return true;
        }
        if self.reservation_failed {
            return false;
        }
        let Ok(layout) = Layout::array::<MaybeUninit<T>>(RECORD_SPACE_CAPACITY) else {
            self.reservation_failed = true;
            return false;
        };
        // SAFETY: the layout is a nonzero record range. The mapping is kept
        // for the arena lifetime and memory is initialized a block at a time.
        let Some(base) = NonNull::new(unsafe { alloc(layout) }.cast()) else {
            self.reservation_failed = true;
            return false;
        };
        self.base = base;
        self.reserved = true;
        true
    }

    /// # Safety
    /// `id` must come from a live direct Value of this record family. Sweep removes the
    /// record only after all weak owners have been pruned.
    pub(super) unsafe fn get_live(&self, id: RecordId) -> &T {
        debug_assert!(
            self.is_occupied(id),
            "live slot names an initialized record"
        );
        // SAFETY: guaranteed by the caller's live-slot invariant.
        unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() }
    }

    /// # Safety
    /// `id` must come from a live direct Value of this record family, and
    /// the caller must have exclusive access to the heap.
    pub(super) unsafe fn get_live_mut(&mut self, id: RecordId) -> &mut T {
        debug_assert!(
            self.is_occupied(id),
            "live slot names an initialized record"
        );
        // SAFETY: guaranteed by the caller's live-slot and exclusive-access invariants.
        unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_mut() }
    }

    #[cfg(test)]
    pub(super) fn remove_live(&mut self, id: RecordId) -> T {
        debug_assert!(self.is_occupied(id), "sweep removes an initialized record");
        let (block_index, record_index) = Self::split_id(id);
        self.blocks[block_index].release(record_index);
        self.blocks[block_index].marked[record_index / OCCUPANCY_WORD_BITS] &=
            !(1_u64 << (record_index % OCCUPANCY_WORD_BITS));
        self.free.push(id);
        self.live -= 1;
        // SAFETY: release cleared the bit and this id was initialized exactly once.
        unsafe { self.base.as_ptr().add(id as usize).read().assume_init() }
    }

    pub(super) fn mark(&mut self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get_mut(block_index)
            .is_some_and(|block| block.is_occupied(record_index) && block.mark(record_index))
    }

    pub(super) fn is_marked(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get(block_index)
            .is_some_and(|block| block.is_occupied(record_index) && block.is_marked(record_index))
    }

    pub(super) fn live_count(&self) -> usize {
        self.live
    }

    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(super) fn high_water(&self) -> usize {
        self.next_id as usize
    }

    /// Sweeps by bitmap word. Dead occupied records are transferred to the
    /// callback exactly once; marked records survive and have their mark bit
    /// cleared for the next collection.
    pub(super) fn sweep(&mut self, mut visit: impl FnMut(RecordSweep<'_, T>)) -> (usize, usize) {
        let mut live_count = 0;
        let mut freed_count = 0;
        for (block_index, block) in self.blocks.iter_mut().enumerate() {
            for word_index in 0..block.occupied.len() {
                let occupied = block.occupied[word_index];
                let marked = block.marked[word_index];
                debug_assert_eq!(marked & !occupied, 0);
                let mut live = marked;
                let mut dead = occupied & !marked;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    let id = (block_index * RECORDS_PER_BLOCK + record_index) as RecordId;
                    // SAFETY: a mark bit is set only for an occupied record.
                    visit(RecordSweep::Keep(id, unsafe {
                        (*self.base.as_ptr().add(id as usize)).assume_init_ref()
                    }));
                    live &= live - 1;
                    live_count += 1;
                }
                while dead != 0 {
                    let bit = dead.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    let id = (block_index * RECORDS_PER_BLOCK + record_index) as RecordId;
                    block.occupied[word_index] &= !(1_u64 << bit);
                    self.free.push(id);
                    // SAFETY: the occupied bit proves initialization and its
                    // removal transfers this record to the drop callback.
                    let value = unsafe { self.base.as_ptr().add(id as usize).read().assume_init() };
                    visit(RecordSweep::Dead(id, value));
                    dead &= dead - 1;
                    freed_count += 1;
                }
                block.marked[word_index] = 0;
            }
        }
        self.live = live_count;
        (live_count, freed_count)
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn for_each_live(&self, mut visit: impl FnMut(RecordId, &T)) {
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    let id = (block_index * RECORDS_PER_BLOCK + record_index) as RecordId;
                    // SAFETY: every occupied bit names an initialized record.
                    visit(id, unsafe {
                        (*self.base.as_ptr().add(id as usize)).assume_init_ref()
                    });
                    live &= live - 1;
                }
            }
        }
    }

    pub(super) fn for_each_live_mut(&mut self, mut visit: impl FnMut(RecordId, &mut T)) {
        let base = self.base;
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let record_index = word_index * OCCUPANCY_WORD_BITS + bit;
                    let id = (block_index * RECORDS_PER_BLOCK + record_index) as RecordId;
                    // SAFETY: each occupied ID is visited once and `&mut self`
                    // excludes every other reference into this space.
                    visit(id, unsafe {
                        (*base.as_ptr().add(id as usize)).assume_init_mut()
                    });
                    live &= live - 1;
                }
            }
        }
    }

    fn is_occupied(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get(block_index)
            .is_some_and(|block| block.is_occupied(record_index))
    }

    pub(super) fn is_live(&self, id: RecordId) -> bool {
        id < self.next_id && self.is_occupied(id)
    }

    fn split_id(id: RecordId) -> (usize, usize) {
        let id = id as usize;
        (id / RECORDS_PER_BLOCK, id % RECORDS_PER_BLOCK)
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn allocated_bytes(&self) -> usize {
        let memory = self.memory_breakdown();
        memory.record_bytes
            + memory.block_metadata_bytes
            + memory.free_id_capacity * size_of::<RecordId>()
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn memory_breakdown(&self) -> RecordSpaceMemory {
        RecordSpaceMemory {
            high_water: self.next_id as usize,
            live_records: self.live,
            record_bytes: self.blocks.len() * RECORDS_PER_BLOCK * size_of::<T>(),
            block_metadata_bytes: self.blocks.capacity() * size_of::<RecordBlock>(),
            free_ids: self.free.len(),
            free_id_capacity: self.free.capacity(),
        }
    }
}

impl<T> Drop for RecordSpace<T> {
    fn drop(&mut self) {
        if !self.reserved {
            return;
        }
        let layout = Layout::array::<MaybeUninit<T>>(RECORD_SPACE_CAPACITY)
            .expect("record-space layout was validated at reservation");
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let id = (block_index * RECORDS_PER_BLOCK
                        + word_index * OCCUPANCY_WORD_BITS
                        + bit) as RecordId;
                    // SAFETY: every set bit denotes exactly one initialized record.
                    unsafe {
                        self.base
                            .as_ptr()
                            .add(id as usize)
                            .as_mut()
                            .unwrap()
                            .assume_init_drop()
                    };
                    live &= live - 1;
                }
            }
        }
        // SAFETY: this was allocated with the layout recomputed above.
        unsafe { dealloc(self.base.as_ptr().cast(), layout) };
    }
}

#[cfg(test)]
mod tests {
    use super::{RECORD_SPACE_CAPACITY, RECORDS_PER_BLOCK, RecordSpace};
    use std::mem::size_of;

    #[test]
    fn object_record_addresses_are_dense_and_reuse_vacancies() {
        let mut arena = RecordSpace::default();
        let first = arena.try_insert(String::from("first")).unwrap();
        let second = arena.try_insert(String::from("second")).unwrap();
        // SAFETY: both ids were returned by successful insertions and remain live.
        let first_address = unsafe { arena.get_live(first) } as *const String as usize;
        // SAFETY: both ids were returned by successful insertions and remain live.
        let second_address = unsafe { arena.get_live(second) } as *const String as usize;
        assert_eq!(second_address - first_address, size_of::<String>());

        let mut last = second;
        for value in 2..=RECORDS_PER_BLOCK {
            last = arena
                .try_insert(format!("record-{value}"))
                .expect("record-space reservation has spare capacity");
        }
        assert_eq!(last as usize, RECORDS_PER_BLOCK);
        // SAFETY: the first id remains live after the second block is added.
        assert_eq!(unsafe { arena.get_live(first) }, "first");
        assert!(RECORD_SPACE_CAPACITY > RECORDS_PER_BLOCK);
        assert_eq!(arena.remove_live(first), "first");
        assert_eq!(arena.try_insert(String::from("reused")).unwrap(), first);
        // SAFETY: the slot-index id was reinitialized by the insertion above.
        assert_eq!(unsafe { arena.get_live(first) }, "reused");
    }

    #[test]
    fn object_record_sweep_uses_marks_and_recycles_dead_ids() {
        use super::RecordSweep;

        let mut arena = RecordSpace::default();
        let dead = arena.try_insert(String::from("dead")).unwrap();
        let live = arena.try_insert(String::from("live")).unwrap();
        assert!(arena.mark(live));
        assert!(!arena.mark(live));

        let mut kept = Vec::new();
        let mut dropped = Vec::new();
        let (live_count, freed_count) = arena.sweep(|record| match record {
            RecordSweep::Keep(id, value) => kept.push((id, value.clone())),
            RecordSweep::Dead(id, value) => dropped.push((id, value)),
        });
        assert_eq!((live_count, freed_count), (1, 1));
        assert_eq!(kept, [(live, String::from("live"))]);
        assert_eq!(dropped, [(dead, String::from("dead"))]);
        assert!(arena.is_live(live));
        assert!(!arena.is_live(dead));
        assert_eq!(arena.try_insert(String::from("reused")).unwrap(), dead);
    }
}
