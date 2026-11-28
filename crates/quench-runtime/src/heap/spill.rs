//! Spill runs of class cells (departure-free S2): a class cell whose class's shape is longer than its region's
//! capacity keeps its last inline slot, `spill_from` (`layouts::spill_from`, the capacity less one), as the spill
//! word: the word offset of the cell's run in the class space's spill arena (`SpillSpace`). A run is an id, never an
//! address, so the arena may reallocate and the heap stays plain data.
//!
//! A run is `2^k` words, preceded by one word holding `k` (its size class). A cell that spills reads its fields from
//! `spill_from` on through its run: field `slot` is `run[slot - spill_from]`. Freed runs go to a free list per size
//! class and are reused. The sweep of a dead cell frees its run (`dispose_dead_runs`).
//!
//! Whether a cell spills is decided by its class (the shape's length), so every accessor takes the cell's `spill`
//! slot: `Heap::live_spill_from` of the live cell.

use super::Heap;
use super::class_space::{ClassSpace, granule, header_class, region_capacity};
use super::layouts;
use super::records::RecordId;
use crate::staged::object_encoding::{CLASS_HEADER_BYTES, DEPARTED_CLASS};
use crate::value::Value;

/// The spill arena of the class space: runs of words, and the freed runs by size class.
#[derive(Default)]
pub(super) struct SpillSpace {
    /// Each run's size-class word, then its `2^k` words. Only offsets name runs, so the arena may grow.
    words: Vec<Value>,
    /// The offsets of freed runs, by size class `k`.
    free: Vec<Vec<u32>>,
}

/// The inline words of a cell and its run's words, from the cell's capacity slice `inline` under spill slot `spill`:
/// the words below the spill slot, and the run the spill word names. Without a spill slot every word is inline.
pub(super) fn split_cell<'a>(inline: &'a [Value], arena: &'a SpillSpace, spill: Option<u32>) -> (&'a [Value], &'a [Value]) {
    match spill {
        None => (inline, &[]),
        Some(from) => (&inline[..from as usize], arena.run(spill_offset_of(inline[from as usize]))),
    }
}

/// The run offset a spill word holds.
#[inline(always)]
pub(super) fn spill_offset_of(word: Value) -> u32 {
    word.to_bits() as u32
}

impl SpillSpace {
    /// The arena's words and its free lists, as the heap snapshot keeps them (`heap::snapshot`): a spilled cell's
    /// spill word names a run by offset here, so the arena is restored with the cells.
    pub(super) fn snapshot_parts(&self) -> (&[Value], &[Vec<u32>]) {
        (&self.words, &self.free)
    }

    /// The arena of a snapshot's `snapshot_parts`.
    pub(super) fn from_snapshot(words: Vec<Value>, free: Vec<Vec<u32>>) -> Self {
        Self { words, free }
    }

    /// The size class of a run of `needed` words: the smallest `k` with `2^k >= needed`.
    fn class_for(needed: usize) -> u32 {
        needed.max(1).next_power_of_two().trailing_zeros()
    }

    /// Claims a run of at least `needed` words, all `Value::DELETED`, and returns its offset. A freed run of the size
    /// class is reused before the arena grows.
    pub(super) fn alloc(&mut self, needed: usize) -> u32 {
        let class = Self::class_for(needed);
        let size = 1usize << class;
        if let Some(offset) = self.free.get_mut(class as usize).and_then(Vec::pop) {
            self.words[offset as usize..offset as usize + size].fill(Value::DELETED);
            return offset;
        }
        let start = self.words.len();
        self.words.push(Value::from_bits(u64::from(class)));
        self.words.resize(start + 1 + size, Value::DELETED);
        (start + 1) as u32
    }

    /// Frees the run at `offset` for reuse by its size class.
    pub(super) fn free_run(&mut self, offset: u32) {
        let class = self.words[offset as usize - 1].to_bits() as usize;
        if self.free.len() <= class {
            self.free.resize_with(class + 1, Vec::new);
        }
        self.free[class].push(offset);
    }

    /// The words of the run at `offset`, its whole size class.
    #[inline(always)]
    pub(super) fn run(&self, offset: u32) -> &[Value] {
        let size = self.run_len(offset);
        &self.words[offset as usize..offset as usize + size]
    }

    /// The words of the run at `offset`, exclusively.
    #[inline(always)]
    pub(super) fn run_mut(&mut self, offset: u32) -> &mut [Value] {
        let size = self.run_len(offset);
        &mut self.words[offset as usize..offset as usize + size]
    }

    /// The number of words of the run at `offset`.
    #[inline(always)]
    pub(super) fn run_len(&self, offset: u32) -> usize {
        1 << self.words[offset as usize - 1].to_bits()
    }

    /// Copies the first `n` words of the run at `from` to the run at `to`.
    pub(super) fn copy_run(&mut self, from: u32, to: u32, n: usize) {
        self.words.copy_within(from as usize..from as usize + n, to as usize);
    }
}

/// The disposal of a dead cell region's records (`RecordSpace::sweep_disposing`): a dead cell that spilled frees its
/// run. The cell's class is read from its header, which still names it, and `spill_of` says where that class spills.
/// The record is forgotten: a cell's words own nothing else.
pub(super) fn dispose_dead_runs<'a, T, F: Fn(u32) -> Option<u32>>(
    arena: &'a mut SpillSpace,
    base: usize,
    region: u32,
    spill_of: &'a F,
) -> impl FnMut(RecordId, &T) -> bool + 'a {
    move |local, _| {
        let g = granule(region, local);
        let cell = (base + g as usize * CLASS_HEADER_BYTES) as *const u64;
        // SAFETY: a dead cell of the reserved space keeps its header and fields until its record is forgotten here.
        let class = header_class(unsafe { *cell });
        if class != DEPARTED_CLASS {
            if let Some(from) = spill_of(class) {
                // SAFETY: the spill word is the cell's field `from`, one word past its header.
                let word = unsafe { *cell.add(1 + from as usize) };
                arena.free_run(spill_offset_of(word));
            }
        }
        false
    }
}

impl ClassSpace {
    /// The inline words and the run's words of cell `g` under spill slot `spill`. The space must be reserved.
    #[inline(always)]
    pub(super) fn cell_words(&self, g: u32, spill: Option<u32>) -> (&[Value], &[Value]) {
        split_cell(self.cell_fields(g), &self.spill, spill)
    }

    /// The inline words and the run's words of cell `g` under spill slot `spill`, exclusively.
    pub(super) fn cell_words_mut(&mut self, g: u32, spill: Option<u32>) -> (&mut [Value], &mut [Value]) {
        let capacity = region_capacity(g) as usize;
        // SAFETY: the cell's capacity fields lie inside the cell at `g`, and `&mut self` is exclusive; the arena is a
        // separate allocation, borrowed disjointly below.
        let inline = unsafe { std::slice::from_raw_parts_mut(self.cell_ptr(g).cast::<Value>().add(1), capacity) };
        match spill {
            None => {
                let empty: &mut [Value] = &mut [];
                (inline, empty)
            }
            Some(from) => {
                let offset = spill_offset_of(inline[from as usize]);
                let (words, _) = inline.split_at_mut(from as usize);
                (words, self.spill.run_mut(offset))
            }
        }
    }

    /// The header and the inline and run words of cell `g` under spill slot `spill`, exclusively.
    pub(super) fn cell_parts_mut(&mut self, g: u32, spill: Option<u32>) -> (&mut u64, &mut [Value], &mut [Value]) {
        // SAFETY: the header is the cell's first word, disjoint from its fields (`cell_words_mut`).
        let header = unsafe { &mut *self.cell_ptr(g) };
        let (words, run) = self.cell_words_mut(g, spill);
        (header, words, run)
    }

    /// The offset of cell `g`'s run, read from its spill word `spill`.
    #[inline(always)]
    fn spill_offset(&self, g: u32, spill: u32) -> u32 {
        spill_offset_of(self.cell_fields(g)[spill as usize])
    }

    /// Writes the spill word of cell `g` at slot `spill`: the offset of its run.
    #[inline(always)]
    fn set_spill_word(&mut self, g: u32, spill: u32, offset: u32) {
        self.set_word(g, None, spill as usize, Value::from_bits(u64::from(offset)));
    }

    /// The word of slot `slot` of cell `g` under spill slot `spill`: a run word past the spill slot, else the inline word.
    #[inline(always)]
    pub(super) fn word(&self, g: u32, spill: Option<u32>, slot: usize) -> Value {
        match spill {
            Some(from) if slot >= from as usize => self.spill.run(self.spill_offset(g, from))[slot - from as usize],
            _ => {
                debug_assert!(slot < region_capacity(g) as usize, "an inline field lies inside its cell");
                // SAFETY: the inline field `slot` lies inside the cell at `g` (the assert).
                unsafe { self.cell_ptr(g).cast::<Value>().add(1 + slot).read() }
            }
        }
    }

    /// Writes the word of slot `slot` of cell `g` under spill slot `spill`. The caller passed the store's barrier.
    #[inline(always)]
    pub(super) fn set_word(&mut self, g: u32, spill: Option<u32>, slot: usize, value: Value) {
        match spill {
            Some(from) if slot >= from as usize => {
                let offset = self.spill_offset(g, from);
                self.spill.run_mut(offset)[slot - from as usize] = value;
            }
            _ => {
                debug_assert!(slot < region_capacity(g) as usize, "an inline field lies inside its cell");
                // SAFETY: the inline field `slot` lies inside the cell at `g` (the assert), and `&mut self` is exclusive.
                unsafe { self.cell_ptr(g).cast::<Value>().add(1 + slot).write(value) };
            }
        }
    }

    /// Stores `value` at slot `slot` of cell `g`, whose spill slot is `from` before the store and `to` after it (equal
    /// for a store into a field the shape already holds). A cell that first spills moves its inline word at the spill
    /// slot into a new run; a run too short for the slot is replaced by a larger one, its words copied, and the old
    /// one freed. Returns whether a run was allocated, which moves the fields a holder reads by address (the holder edge).
    pub(super) fn put_word(&mut self, g: u32, from: Option<u32>, to: Option<u32>, slot: usize, value: Value) -> bool {
        let Some(spill) = to else {
            self.set_word(g, None, slot, value);
            return false;
        };
        let index = slot - spill as usize;
        if from.is_some() {
            let offset = self.spill_offset(g, spill);
            if self.spill.run_len(offset) > index {
                self.set_word(g, to, slot, value);
                return false;
            }
            let old_len = self.spill.run_len(offset);
            let new = self.spill.alloc((index + 1).max(old_len * 2));
            self.spill.copy_run(offset, new, old_len);
            self.spill.free_run(offset);
            self.set_spill_word(g, spill, new);
        } else {
            let first = self.word(g, None, spill as usize);
            let new = self.spill.alloc(index + 1);
            self.spill.run_mut(new)[0] = first;
            self.set_spill_word(g, spill, new);
        }
        self.set_word(g, to, slot, value);
        true
    }

    /// Makes the fields of cell `g` `words` (the stored words of its shape, `words.len()` of them), the cell having
    /// spill slot `from` before: inline below the spill slot, and from it a run where they exceed the region's
    /// capacity. A run that is long enough is reused in place; the rest of the cell is `Value::DELETED`. Returns
    /// whether the fields moved: a run was allocated, or the cell stopped spilling.
    pub(super) fn lay_out(&mut self, g: u32, from: Option<u32>, words: &[Value]) -> bool {
        let capacity = region_capacity(g) as usize;
        let spill = capacity - 1;
        if words.len() <= capacity {
            let moved = match from {
                Some(_) => {
                    let offset = self.spill_offset(g, spill as u32);
                    self.spill.free_run(offset);
                    true
                }
                None => false,
            };
            let (inline, _) = self.cell_words_mut(g, None);
            inline[..words.len()].copy_from_slice(words);
            inline[words.len()..].fill(Value::DELETED);
            return moved;
        }
        let needed = words.len() - spill;
        let (offset, moved) = match from {
            Some(_) => {
                let old = self.spill_offset(g, spill as u32);
                if self.spill.run_len(old) >= needed {
                    (old, false)
                } else {
                    self.spill.free_run(old);
                    (self.spill.alloc(needed), true)
                }
            }
            None => (self.spill.alloc(needed), true),
        };
        let run = self.spill.run_mut(offset);
        run[..needed].copy_from_slice(&words[spill..]);
        run[needed..].fill(Value::DELETED);
        let (inline, _) = self.cell_words_mut(g, None);
        inline[..spill].copy_from_slice(&words[..spill]);
        inline[spill] = Value::from_bits(u64::from(offset));
        moved
    }

    /// Frees the run of cell `g` under spill slot `spill`, if it spills, and makes the spill word slack. Returns whether it did.
    pub(super) fn drop_run(&mut self, g: u32, spill: Option<u32>) -> bool {
        let Some(from) = spill else {
            return false;
        };
        let offset = self.spill_offset(g, from);
        self.spill.free_run(offset);
        self.set_word(g, None, from as usize, Value::DELETED);
        true
    }
}

impl Heap {
    /// The spill slot of a class cell of region `region` whose shape is `shape` (`layouts::spill_from`): `None` where
    /// the shape's fields fit the region.
    #[inline(always)]
    pub(crate) fn shape_spill_from(&self, region: u32, shape: u32) -> Option<u32> {
        layouts::spill_from(region, self.properties.shape_length(shape))
    }

    /// The spill slot of live class cell `granule`, from its class (`class_spill_from`).
    #[inline(always)]
    pub(crate) fn live_spill_from(&self, granule: u32) -> Option<u32> {
        let class = header_class(self.class_space.header(granule));
        self.class_spill_from(class)
    }

    /// The field `slot` of live class cell `granule`, read through its run where it spills.
    #[inline(always)]
    pub(crate) fn class_slot_value(&self, granule: u32, slot: usize) -> Value {
        self.class_space.word(granule, self.live_spill_from(granule), slot)
    }

    /// The fields of marked holder cell `granule` moved (a run was allocated or grown, or dropped): a held entry
    /// read the holder's slots by address, so the holder edge fires as a departure does (`note_holder_departure`).
    pub(crate) fn holder_fields_moved(&mut self, granule: u32) {
        if self.class_space.take_holder_cell(granule) {
            self.class_space.note_holder_departure();
            self.request_collection();
        }
    }
}
