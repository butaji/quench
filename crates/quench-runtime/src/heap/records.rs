use std::alloc::Layout;
use std::alloc::{alloc, dealloc, handle_alloc_error};
use std::mem::{MaybeUninit, size_of};
use std::ptr::NonNull;

use super::slots::{MAX_RECORD_COUNT, RecordId};

const RECORDS_PER_BLOCK: usize = 1_024;
const RECORDS_PER_BLOCK_U32: u32 = RECORDS_PER_BLOCK as u32;
const OCCUPANCY_WORD_BITS: usize = u64::BITS as usize;
const OBJECT_RECORD_SPACE_CAPACITY: usize = 1 << 24;

const _: () = assert!(RECORDS_PER_BLOCK.is_power_of_two());
const _: () = assert!(RECORDS_PER_BLOCK <= MAX_RECORD_COUNT as usize);
const _: () = assert!(OBJECT_RECORD_SPACE_CAPACITY.is_power_of_two());
const _: () = assert!(OBJECT_RECORD_SPACE_CAPACITY.is_multiple_of(RECORDS_PER_BLOCK));

pub(super) struct RecordArena<T> {
    base: NonNull<MaybeUninit<T>>,
    reserved: bool,
    blocks: Vec<RecordBlock>,
    next_block: usize,
}

impl<T> Default for RecordArena<T> {
    fn default() -> Self {
        Self {
            base: NonNull::dangling(),
            reserved: false,
            blocks: Vec::new(),
            next_block: 0,
        }
    }
}

struct RecordBlock {
    occupied: Vec<u64>,
    next_word: usize,
}

impl RecordBlock {
    fn new() -> Self {
        let bitmap_words = RECORDS_PER_BLOCK.div_ceil(OCCUPANCY_WORD_BITS);
        Self {
            occupied: vec![0; bitmap_words],
            next_word: 0,
        }
    }

    fn claim(&mut self) -> Option<usize> {
        for offset in 0..self.occupied.len() {
            let word_index = (self.next_word + offset) % self.occupied.len();
            let free_bits = !self.occupied[word_index];
            if free_bits == 0 {
                continue;
            }
            let bit = free_bits.trailing_zeros() as usize;
            let index = word_index * OCCUPANCY_WORD_BITS + bit;
            if index >= RECORDS_PER_BLOCK {
                continue;
            }
            self.occupied[word_index] |= 1 << bit;
            self.next_word = word_index;
            return Some(index);
        }
        None
    }

    fn is_occupied(&self, index: usize) -> bool {
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        self.occupied
            .get(word_index)
            .is_some_and(|word| word & (1_u64 << bit) != 0)
    }

    fn remove(&mut self, index: usize) -> bool {
        if !self.is_occupied(index) {
            return false;
        }
        let word_index = index / OCCUPANCY_WORD_BITS;
        let bit = index % OCCUPANCY_WORD_BITS;
        self.occupied[word_index] &= !(1_u64 << bit);
        self.next_word = self.next_word.min(word_index);
        true
    }
}

/// Object locators index one fixed address range. Block metadata controls
/// allocation and sweep, but resolving a live locator never walks it.
pub(super) struct ObjectRecordSpace<T> {
    base: NonNull<MaybeUninit<T>>,
    reserved: bool,
    reservation_failed: bool,
    blocks: Vec<ObjectRecordBlock>,
    next_id: u32,
    free: Vec<u32>,
    live: usize,
}

impl<T> Default for ObjectRecordSpace<T> {
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

struct ObjectRecordBlock {
    occupied: [u64; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
    marked: [u64; RECORDS_PER_BLOCK / OCCUPANCY_WORD_BITS],
}

pub(super) enum RecordSweep<'a, T> {
    Keep(RecordId, &'a T),
    Dead(RecordId, T),
}

impl ObjectRecordBlock {
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

impl<T> ObjectRecordSpace<T> {
    /// Allocates a dense identity in this record space. Object Values carry
    /// this ID directly, independent of legacy Cell slot allocation.
    pub(super) fn try_insert(&mut self, value: T) -> Result<RecordId, T> {
        if !self.ensure_reserved() {
            return Err(value);
        }
        let reused = self.free.last().copied();
        let id = reused.unwrap_or(self.next_id);
        if id as usize >= OBJECT_RECORD_SPACE_CAPACITY {
            return Err(value);
        }
        let (block_index, record_index) = Self::split_id(id);
        while self.blocks.len() <= block_index {
            self.blocks.push(ObjectRecordBlock::new());
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
        let Ok(layout) = Layout::array::<MaybeUninit<T>>(OBJECT_RECORD_SPACE_CAPACITY) else {
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
    /// `id` must come from a live direct Object Value or Object SlotEntry.
    /// Sweep clears that entry only after removing its record from this space.
    pub(super) unsafe fn get_live(&self, id: RecordId) -> &T {
        debug_assert!(
            self.is_occupied(id),
            "live slot names an initialized record"
        );
        // SAFETY: guaranteed by the caller's live-slot invariant.
        unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() }
    }

    /// # Safety
    /// `id` must come from a live direct Object Value or Object SlotEntry, and
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
        self.blocks.len() * RECORDS_PER_BLOCK * size_of::<T>()
            + self.blocks.capacity() * size_of::<ObjectRecordBlock>()
            + self.free.capacity() * size_of::<RecordId>()
    }
}

impl<T> Drop for ObjectRecordSpace<T> {
    fn drop(&mut self) {
        if !self.reserved {
            return;
        }
        let layout = Layout::array::<MaybeUninit<T>>(OBJECT_RECORD_SPACE_CAPACITY)
            .expect("ObjectRecord space layout was validated at reservation");
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

impl<T> RecordArena<T> {
    pub(super) fn insert(&mut self, value: T) -> RecordId {
        assert!(
            size_of::<T>() != 0,
            "record blocks require non-zero-sized records"
        );
        self.ensure_reserved();
        for offset in 0..self.blocks.len() {
            let block_index = (self.next_block + offset) % self.blocks.len();
            if let Some(record_index) = self.blocks[block_index].claim() {
                self.next_block = block_index;
                let id = Self::record_id(block_index, record_index);
                // SAFETY: claim set the occupancy bit for a previously vacant
                // ID, and the reserved range keeps this address stable.
                unsafe {
                    self.base
                        .as_ptr()
                        .add(id as usize)
                        .write(MaybeUninit::new(value))
                };
                return id;
            }
        }

        assert!(
            self.blocks.len() < MAX_RECORD_COUNT as usize / RECORDS_PER_BLOCK,
            "record arena exhausted its encoded locator space"
        );
        let mut block = RecordBlock::new();
        let record_index = block.claim().expect("a new record block is empty");
        let block_index = self.blocks.len();
        self.blocks.push(block);
        self.next_block = block_index;
        let id = Self::record_id(block_index, record_index);
        // SAFETY: the new block claims this previously vacant ID, and the
        // reserved range keeps this address stable.
        unsafe {
            self.base
                .as_ptr()
                .add(id as usize)
                .write(MaybeUninit::new(value))
        };
        id
    }

    #[cfg(test)]
    pub(super) fn get(&self, id: RecordId) -> Option<&T> {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get(block_index)?
            .is_occupied(record_index)
            .then(|| {
                // SAFETY: the occupancy bit proves initialization.
                unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() }
            })
    }

    /// # Safety
    /// `id` must be named by a live Cell `SlotEntry`; the slot entry is cleared
    /// before this record is released during sweep.
    pub(super) unsafe fn get_live(&self, id: RecordId) -> &T {
        debug_assert!(self.is_occupied(id), "live Cell locator names a record");
        // SAFETY: guaranteed by the caller's SlotEntry liveness invariant.
        unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_ref() }
    }

    /// # Safety
    /// `id` must be named by a live Cell `SlotEntry`, and the caller must have
    /// exclusive access to the heap.
    pub(super) unsafe fn get_live_mut(&mut self, id: RecordId) -> &mut T {
        debug_assert!(self.is_occupied(id), "live Cell locator names a record");
        // SAFETY: guaranteed by the caller's liveness invariant and `&mut self`.
        unsafe { (*self.base.as_ptr().add(id as usize)).assume_init_mut() }
    }

    pub(super) fn remove(&mut self, id: RecordId) -> Option<T> {
        let (block_index, record_index) = Self::split_id(id);
        if !self.blocks.get_mut(block_index)?.remove(record_index) {
            return None;
        }
        // SAFETY: the occupancy bit proved initialization, and clearing it
        // transfers the value to the caller exactly once.
        Some(unsafe { self.base.as_ptr().add(id as usize).read().assume_init() })
    }

    fn record_id(block_index: usize, record_index: usize) -> RecordId {
        let id = block_index
            .checked_mul(RECORDS_PER_BLOCK)
            .and_then(|base| base.checked_add(record_index))
            .expect("record block index fits usize");
        debug_assert!(
            id < MAX_RECORD_COUNT as usize,
            "record id exceeds its encoded locator space"
        );
        id as RecordId
    }

    fn split_id(id: RecordId) -> (usize, usize) {
        (
            (id / RECORDS_PER_BLOCK_U32) as usize,
            (id % RECORDS_PER_BLOCK_U32) as usize,
        )
    }

    #[allow(dead_code)]
    pub(super) fn allocated_bytes(&self) -> usize {
        self.blocks.len() * RECORDS_PER_BLOCK * size_of::<T>()
            + self.blocks.capacity() * size_of::<RecordBlock>()
            + self
                .blocks
                .iter()
                .map(|block| block.occupied.capacity() * size_of::<u64>())
                .sum::<usize>()
    }

    fn ensure_reserved(&mut self) {
        if self.reserved {
            return;
        }
        let layout = Layout::array::<MaybeUninit<T>>(MAX_RECORD_COUNT as usize)
            .expect("encoded record locator range has a representable layout");
        // SAFETY: the layout is the full encoded record range. The allocator
        // reserves virtual address space and pages become resident as records
        // are written.
        let Some(base) = NonNull::new(unsafe { alloc(layout) }.cast()) else {
            handle_alloc_error(layout);
        };
        self.base = base;
        self.reserved = true;
    }

    fn is_occupied(&self, id: RecordId) -> bool {
        let (block_index, record_index) = Self::split_id(id);
        self.blocks
            .get(block_index)
            .is_some_and(|block| block.is_occupied(record_index))
    }
}

impl<T> Drop for RecordArena<T> {
    fn drop(&mut self) {
        if !self.reserved {
            return;
        }
        for (block_index, block) in self.blocks.iter().enumerate() {
            for (word_index, word) in block.occupied.iter().copied().enumerate() {
                let mut live = word;
                while live != 0 {
                    let bit = live.trailing_zeros() as usize;
                    let id =
                        block_index * RECORDS_PER_BLOCK + word_index * OCCUPANCY_WORD_BITS + bit;
                    if id < MAX_RECORD_COUNT as usize {
                        // SAFETY: every occupied bit names one initialized T.
                        unsafe {
                            self.base
                                .as_ptr()
                                .add(id)
                                .as_mut()
                                .unwrap()
                                .assume_init_drop()
                        };
                    }
                    live &= live - 1;
                }
            }
        }
        let layout = Layout::array::<MaybeUninit<T>>(MAX_RECORD_COUNT as usize)
            .expect("encoded record locator range has a representable layout");
        // SAFETY: base was allocated with this exact layout in ensure_reserved.
        unsafe { dealloc(self.base.as_ptr().cast(), layout) };
    }
}

#[cfg(test)]
mod tests {
    use super::{OBJECT_RECORD_SPACE_CAPACITY, ObjectRecordSpace, RECORDS_PER_BLOCK, RecordArena};
    use std::mem::size_of;

    #[test]
    fn record_ids_resolve_across_blocks_and_reuse_vacancies() {
        let mut arena = RecordArena::default();
        let first = arena.insert(String::from("first"));
        let second = arena.insert(String::from("second"));
        assert_eq!(first, 0);
        assert_eq!(second, 1);
        // SAFETY: both IDs were returned by successful insertions and are live.
        let first_address = unsafe { arena.get_live(first) } as *const String as usize;
        // SAFETY: both IDs were returned by successful insertions and are live.
        let second_address = unsafe { arena.get_live(second) } as *const String as usize;
        assert_eq!(second_address - first_address, size_of::<String>());
        assert_eq!(arena.get(first).map(String::as_str), Some("first"));
        assert_eq!(arena.remove(first).as_deref(), Some("first"));
        assert!(arena.get(first).is_none());

        let reused = arena.insert(String::from("reused"));
        assert_eq!(reused, first);
        assert_eq!(arena.get(reused).map(String::as_str), Some("reused"));
        assert_eq!(arena.get(second).map(String::as_str), Some("second"));

        let last_in_first_block = (2..RECORDS_PER_BLOCK)
            .map(|value| arena.insert(value.to_string()))
            .last()
            .unwrap();
        let first_in_second_block = arena.insert(String::from("next"));
        assert_eq!(last_in_first_block as usize, RECORDS_PER_BLOCK - 1);
        assert_eq!(first_in_second_block as usize, RECORDS_PER_BLOCK);
        assert_eq!(
            arena.get(first_in_second_block).map(String::as_str),
            Some("next")
        );
    }

    #[test]
    fn object_record_addresses_are_dense_and_reuse_vacancies() {
        let mut arena = ObjectRecordSpace::default();
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
        assert!(OBJECT_RECORD_SPACE_CAPACITY > RECORDS_PER_BLOCK);
        assert_eq!(arena.remove_live(first), "first");
        assert_eq!(arena.try_insert(String::from("reused")).unwrap(), first);
        // SAFETY: the slot-index id was reinitialized by the insertion above.
        assert_eq!(unsafe { arena.get_live(first) }, "reused");
    }

    #[test]
    fn object_record_sweep_uses_marks_and_recycles_dead_ids() {
        use super::RecordSweep;

        let mut arena = ObjectRecordSpace::default();
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
