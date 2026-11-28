//! The class space: the cells of class-fixed objects. A cell is a run of granules, `CLASS_HEADER_BYTES`
//! each: a header granule, then the object's fields. A `Value` names a cell by its granule `g`, at
//! `base + g * CLASS_HEADER_BYTES`.
//!
//! Region `k` holds cells of `2^k` granules and starts at granule `k << REGION_SHIFT`. So granule `g`
//! lies in region `g >> REGION_SHIFT`, at local cell `(g & REGION_MASK) >> k`. Each region is a record
//! space over its slice of the one reservation (`RecordSpace::over`), so allocation, sweep and the
//! barrier are the record machinery's.
//!
//! Nothing allocates cells yet: the space reserves its range on first use, and the types and addressing
//! here are what the allocating items build on.

#![allow(dead_code)]

use super::layouts::class_region;
use super::records::{AddressRange, RecordId, RecordSpace, RecordValue};
use super::spill::SpillSpace;
use crate::staged::layout_offsets::NATIVE_CLASS_CELL_LAYOUT;
use crate::staged::object_encoding::{CAPACITY_CLASSES, CLASS_HEADER_BYTES, NO_CLASS, REGION_MASK, REGION_SHIFT, region_field_capacity};

/// The high half of a region 0 stub's header, which names its object record (`stub_header`). A stub is decided by
/// its granule's region, never by its header's low half, which is `NO_CLASS`.
const STUB_RECORD_SHIFT: u32 = u32::BITS;
use crate::value::Value;
use std::mem::{MaybeUninit, size_of};
use std::ptr::NonNull;

// A class cell of region `k`: the header, then `2^k - 1` fields, each a tagged `Value`. A slack field
// holds `Value::DELETED`, so the collector traces every field of a capacity alike.
macro_rules! class_cells {
    ($($name:ident: $fields:literal;)*) => {$(
        #[repr(C)]
        pub(super) struct $name {
            pub(super) header: u64,
            pub(super) fields: [Value; $fields],
        }

        const _: () = assert!(size_of::<$name>() == CLASS_HEADER_BYTES * ($fields + 1));
        const _: () = assert!(std::mem::offset_of!($name, fields) == NATIVE_CLASS_CELL_LAYOUT.fields_offset);
    )*};
}

class_cells! {
    ClassCell0: 0;
    ClassCell1: 1;
    ClassCell2: 3;
    ClassCell3: 7;
    ClassCell4: 15;
    ClassCell5: 31;
}

/// The cells region `k` holds: `2^(REGION_SHIFT - k)`, so every region spans `2^REGION_SHIFT` granules.
pub(super) const fn region_cells(k: u32) -> usize {
    1 << (REGION_SHIFT - k)
}

pub(super) type Region0 = RecordSpace<ClassCell0, { region_cells(0) }>;
pub(super) type Region1 = RecordSpace<ClassCell1, { region_cells(1) }>;
pub(super) type Region2 = RecordSpace<ClassCell2, { region_cells(2) }>;
pub(super) type Region3 = RecordSpace<ClassCell3, { region_cells(3) }>;
pub(super) type Region4 = RecordSpace<ClassCell4, { region_cells(4) }>;
pub(super) type Region5 = RecordSpace<ClassCell5, { region_cells(5) }>;

/// The regions that hold class cells, `1..CAPACITY_CLASSES`. Region 0 holds the object stubs, which
/// their records sweep and count.
pub(super) const CELL_REGIONS: usize = CAPACITY_CLASSES - 1;

/// Runs `$body` with `$space` bound to the region `$region` of `$regions` (a `ClassSpace`'s `regions`),
/// shared or mutable.
macro_rules! with_region {
    ($regions:expr, $region:expr, |$space:ident| $body:expr) => {
        match $region {
            0 => { let $space = &$regions.0; $body }
            1 => { let $space = &$regions.1; $body }
            2 => { let $space = &$regions.2; $body }
            3 => { let $space = &$regions.3; $body }
            4 => { let $space = &$regions.4; $body }
            5 => { let $space = &$regions.5; $body }
            _ => unreachable!("a granule past the class space"),
        }
    };
}

/// `with_region!`, with the region mutable.
macro_rules! with_region_mut {
    ($regions:expr, $region:expr, |$space:ident| $body:expr) => {
        match $region {
            0 => { let $space = &mut $regions.0; $body }
            1 => { let $space = &mut $regions.1; $body }
            2 => { let $space = &mut $regions.2; $body }
            3 => { let $space = &mut $regions.3; $body }
            4 => { let $space = &mut $regions.4; $body }
            5 => { let $space = &mut $regions.5; $body }
            _ => unreachable!("a granule past the class space"),
        }
    };
}

// The collector's view of the cell regions (`marks`) comes after the region macros it uses.
mod marks;
pub(super) use marks::ClassCellMarks;
// The extras half of a live cell's header (`write_class`, `extras_id`, `set_extras_id`).
mod extras;
pub(super) use extras::extras_id_at;

// A cell region's records are its cells: local id `id` of region `k` is the cell at `granule(k, id)`.
macro_rules! cell_record_values {
    ($($name:ident: $region:literal;)*) => {$(
        impl RecordValue for $name {
            fn value_of<const CAPACITY: usize, const HOLE: usize>(_: &RecordSpace<Self, CAPACITY, HOLE>, id: RecordId) -> Value {
                Value::direct_object(granule($region, id))
            }
        }
    )*};
}

cell_record_values! { ClassCell1: 1; ClassCell2: 2; ClassCell3: 3; ClassCell4: 4; ClassCell5: 5; }

/// The bytes a region's granules span, and the bytes of the whole space.
const REGION_BYTES: usize = CLASS_HEADER_BYTES << REGION_SHIFT;
const SPACE_BYTES: usize = REGION_BYTES * CAPACITY_CLASSES;

/// The class space of a heap: empty until `ensure_reserved` maps its range.
#[derive(Default)]
pub(super) struct ClassSpace {
    /// The range's first address, which is region 0's base; 0 while unreserved.
    pub(super) base: usize,
    pub(super) regions: (Region0, Region1, Region2, Region3, Region4, Region5),
    /// One bit per granule: set where a field entry names the cell as its holder (`Heap::note_holder_cell`),
    /// cleared when the cell's fields move (`take_holder_moved`). Grown on demand, so an unmarked space costs nothing.
    holder_cells: Vec<u64>,
    /// Whether a marked holder's fields moved since the VM last cleared its field caches (`take_holder_moves`).
    holder_moved: bool,
    /// The spill arena: the runs of the cells whose class spills (`heap::spill`).
    pub(super) spill: SpillSpace,
    /// The range the regions are cut from. It is the last field, so it unmaps after them.
    range: Option<AddressRange>,
}

impl ClassSpace {
    /// Sets granule `g`'s holder-cell bit.
    pub(super) fn mark_holder_cell(&mut self, g: u32) {
        let word = g as usize / u64::BITS as usize;
        if self.holder_cells.len() <= word {
            self.holder_cells.resize(word + 1, 0);
        }
        self.holder_cells[word] |= 1 << (g % u64::BITS);
    }

    /// Clears granule `g`'s holder-cell bit, returning whether it was set.
    pub(super) fn take_holder_moved(&mut self, g: u32) -> bool {
        let bit = 1u64 << (g % u64::BITS);
        match self.holder_cells.get_mut(g as usize / u64::BITS as usize) {
            Some(word) if *word & bit != 0 => {
                *word &= !bit;
                true
            }
            _ => false,
        }
    }

    /// Records that a marked holder's fields moved (`Heap::holder_fields_moved`), for the VM to drain.
    pub(super) fn note_holder_moved(&mut self) {
        self.holder_moved = true;
    }

    /// Returns and clears the moved-holder flag.
    pub(super) fn take_holder_moves(&mut self) -> bool {
        std::mem::take(&mut self.holder_moved)
    }

    /// Maps the space's range and cuts its regions from it on the first call; `false` where that failed.
    pub(super) fn ensure_reserved(&mut self) -> bool {
        if self.range.is_some() {
            return true;
        }
        let Some(range) = AddressRange::map(SPACE_BYTES) else {
            return false;
        };
        let base = range.base();
        // SAFETY: region `k` spans `REGION_BYTES` from `k * REGION_BYTES`, and holds `region_cells(k)`
        // cells of that span, all inside `range`. `range` is the last field, so it outlives the regions.
        self.regions = unsafe {
            (
                RecordSpace::over(region_start(base, 0), region_cells(0)),
                RecordSpace::over(region_start(base, 1), region_cells(1)),
                RecordSpace::over(region_start(base, 2), region_cells(2)),
                RecordSpace::over(region_start(base, 3), region_cells(3)),
                RecordSpace::over(region_start(base, 4), region_cells(4)),
                RecordSpace::over(region_start(base, 5), region_cells(5)),
            )
        };
        self.base = base.as_ptr() as usize;
        self.range = Some(range);
        true
    }

    /// The address of granule `g`'s cell: `base + g * CLASS_HEADER_BYTES`. The space must be reserved.
    pub(super) fn cell_ptr(&self, g: u32) -> *mut u64 {
        debug_assert!(self.range.is_some() && (locate(g).0 as usize) < CAPACITY_CLASSES);
        (self.base + g as usize * CLASS_HEADER_BYTES) as *mut u64
    }

    /// The record id of the ordinary object whose stub is granule `g` (`stub_record_at`). The
    /// space must be reserved, and `g` must name a live stub.
    #[inline(always)]
    pub(super) fn record_of(&self, g: u32) -> u32 {
        debug_assert!(self.range.is_some());
        stub_record_at(self.base, g)
    }

    /// Claims a region 0 stub for object record `record`: the cell's header is `stub_header(record)`,
    /// so its granule names the record through `record_of`. The stub is claimed from region 0's cursor
    /// (`RecordSpace::bump_id`), charged as an allocation. `None` where the space is full.
    pub(super) fn claim_stub(&mut self, record: u32, allocated: &mut usize) -> Option<u32> {
        if !self.ensure_reserved() {
            return None;
        }
        let local = self.regions.0.bump_id(allocated)?;
        // SAFETY: `local` is a claimed, uninitialized region 0 cell, written whole here.
        unsafe { self.regions.0.record_ptr(local).write(ClassCell0 { header: stub_header(record), fields: [] }) };
        Some(granule(0, local))
    }

    /// Claims a class cell of region `region` for class `class`: its header the live class id, its fields `fields`
    /// in slot order and the slack `Value::DELETED`. The cell comes from the region's cursor (`RecordSpace::bump_id`),
    /// charged as an allocation. `None` where the region is full. `fields` must fit the region.
    pub(super) fn claim_cell(&mut self, region: u32, class: u32, fields: &[Value], allocated: &mut usize) -> Option<u32> {
        debug_assert!((1..CAPACITY_CLASSES as u32).contains(&region));
        // A class names its region (`layouts::intern_class`), so a cell of the class is a cell of that region.
        debug_assert_eq!(class_region(class), region, "a cell's class names the region it is claimed from");
        if !self.ensure_reserved() {
            return None;
        }
        let local = with_region_mut!(self.regions, region, |space| space.bump_id(allocated))?;
        let granule = granule(region, local);
        // SAFETY: `granule` is a claimed, uninitialized cell of the reserved space; its header is written here, and
        // its fields by `lay_out`, which writes every inline word.
        unsafe { self.cell_ptr(granule).write(live_header(class)) };
        self.lay_out(granule, None, fields);
        Some(granule)
    }

    /// The address of region `region`'s allocation cursor, which a construction's class cell claim bumps
    /// (`staged::bump::claim_class_cell`).
    pub(super) fn cell_cursor_address(&self, region: u32) -> usize {
        debug_assert!((1..CAPACITY_CLASSES as u32).contains(&region));
        with_region!(self.regions, region, |space| std::ptr::addr_of!(space.cursor) as usize)
    }

    /// Frees stub `g` with its dead record (the record sweep's dispose). A stub is a region 0 cell of an
    /// array-kinded or other record object; a class cell is never a stub.
    pub(super) fn release_stub(&mut self, g: u32) {
        let (region, local) = locate(g);
        debug_assert_eq!(region, 0, "a stub is a region 0 cell");
        self.regions.0.release_live(local);
    }

    /// Withholds stub `g`, a dead prototype's, from reuse until `release_withheld_stubs`,
    /// as its record id is withheld: a Value naming the stub names no other object meanwhile.
    pub(super) fn withhold_stub(&mut self, g: u32) {
        let (region, local) = locate(g);
        debug_assert_eq!(region, 0, "a stub is a region 0 cell");
        self.regions.0.withhold(local);
    }

    /// Makes every withheld stub reusable again, in every region (`RecordSpace::release_withheld`).
    pub(super) fn release_withheld_stubs(&mut self) {
        self.regions.0.release_withheld();
        self.regions.1.release_withheld();
        self.regions.2.release_withheld();
        self.regions.3.release_withheld();
        self.regions.4.release_withheld();
        self.regions.5.release_withheld();
    }

    /// The header word of granule `g`'s cell: a class cell's class and extras (`live_header`), or a
    /// stub's record (`stub_header`). The space must be reserved.
    #[inline(always)]
    pub(super) fn header(&self, g: u32) -> u64 {
        // SAFETY: `g` names a cell of the reserved space (`cell_ptr`), whose header is a readable word.
        unsafe { *self.cell_ptr(g) }
    }

    /// Writes granule `g`'s header word. Only the class edges write one (`CellViewMut::set_class`, `write_class`).
    #[inline(always)]
    pub(super) fn write_header(&mut self, g: u32, header: u64) {
        // SAFETY: as `header`, and `&mut self` is exclusive.
        unsafe { *self.cell_ptr(g) = header };
    }

    /// Frees the unallocated rest of every region's cursor run and empties the cursors, returning how many
    /// granules that was (`RecordSpace::retire_cursor`). An inline empty object allocation and every other cell
    /// allocation bump their region's run, so every collection retires all of them with the record
    /// cursors, before occupancy or a snapshot reads the space.
    pub(super) fn retire_cursors(&mut self) -> usize {
        let regions = &mut self.regions;
        regions.0.retire_cursor()
            + regions.1.retire_cursor()
            + regions.2.retire_cursor()
            + regions.3.retire_cursor()
            + regions.4.retire_cursor()
            + regions.5.retire_cursor()
    }

    /// The class of granule `g`'s cell when the granule is a class cell; `None` for a stub (its header
    /// names a record). The space must be reserved.
    #[inline(always)]
    pub(super) fn live_class(&self, g: u32) -> Option<u32> {
        debug_assert!(self.range.is_some());
        live_class_at(self.base, g)
    }

    /// The fields of granule `g`'s cell: `region_capacity(g)` tagged values, a slack one `Value::DELETED`.
    /// The space must be reserved.
    #[inline(always)]
    pub(super) fn cell_fields(&self, g: u32) -> &[Value] {
        debug_assert!(self.range.is_some());
        // SAFETY: `g` names a cell of this reserved space, and nothing writes the fields while `self` is borrowed.
        unsafe { cell_fields_at(self.base, g) }
    }

    /// The header word of granule `g`'s cell, by reference (`CellView`). The space must be reserved.
    #[inline(always)]
    pub(super) fn cell_header(&self, g: u32) -> &u64 {
        debug_assert!(self.range.is_some());
        // SAFETY: `g` names a cell of the reserved space; its header is a readable word that nothing writes
        // while `self` is borrowed.
        unsafe { &*self.cell_ptr(g) }
    }

    /// Whether a store of `stored` into cell `g` needs its region's barrier log (`RecordSpace::store_logs`).
    pub(super) fn store_logs(&self, g: u32, stored: Value) -> bool {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.store_logs(local, stored))
    }

    /// The write barrier for a write into live cell `g` (`RecordSpace::barrier`). Only cell regions hold live cells.
    pub(super) fn barrier(&mut self, g: u32) {
        let (region, local) = locate(g);
        let regions = &mut self.regions;
        match region {
            1 => regions.1.barrier(local),
            2 => regions.2.barrier(local),
            3 => regions.3.barrier(local),
            4 => regions.4.barrier(local),
            5 => regions.5.barrier(local),
            _ => unreachable!("a live class cell lies in a cell region"),
        }
    }

    /// The mark blocks of region `region` and their count (`RecordSpace::native_mark_blocks`), which a native
    /// store into a cell of that region reads through `JsStepHead::class_mark_lists`.
    pub(super) fn native_mark_blocks(&self, region: u32) -> (*const u8, usize) {
        with_region!(self.regions, region, |space| space.native_mark_blocks())
    }

    /// The number of moves of the spill arena's words (`SpillSpace::moves`), which `native_layout_epoch` counts.
    pub(super) fn spill_moves(&self) -> u64 {
        self.spill.moves()
    }

    /// Blocks made so far in every region, which `native_layout_epoch` counts: a new block can move
    /// the mark blocks native code caches.
    pub(super) fn block_count(&self) -> usize {
        self.regions.0.block_count()
            + self.regions.1.block_count()
            + self.regions.2.block_count()
            + self.regions.3.block_count()
            + self.regions.4.block_count()
            + self.regions.5.block_count()
    }
}

/// The record id of the ordinary object whose stub is granule `g` in the class space at `base`: the
/// high half of the stub's header (`stub_header`). Reading it is the one hop from a direct object `Value`
/// to its record, so it needs only the base, not the heap. Only a region 0 granule is a stub.
#[inline(always)]
pub(crate) fn stub_record_at(base: usize, g: u32) -> u32 {
    debug_assert_eq!(locate(g).0, 0, "a stub is a region 0 granule");
    // SAFETY: a direct object `Value` names a stub granule of the reserved class space, whose
    // header is a readable u64 at `base + g * CLASS_HEADER_BYTES`.
    let header = unsafe { *((base + g as usize * CLASS_HEADER_BYTES) as *const u64) };
    header_record(header)
}

/// The class of granule `g`'s class cell in the class space at `base`; `None` for a region 0 stub, which has no
/// class. A class cell is always live (no cell departs), so its header's class half is the answer.
#[inline(always)]
pub(super) fn live_class_at(base: usize, g: u32) -> Option<u32> {
    if !is_cell_granule(g) {
        return None;
    }
    // SAFETY: a direct object `Value` names a granule of the reserved class space, whose header is a readable u64.
    let header = unsafe { *((base + g as usize * CLASS_HEADER_BYTES) as *const u64) };
    Some(header_class(header))
}

/// The fields of granule `g`'s cell in the class space at `base`: `region_capacity(g)` values after its header.
///
/// # Safety
/// `g` names a cell of the reserved space at `base`, and nothing writes its fields while the slice lives.
#[inline(always)]
pub(super) unsafe fn cell_fields_at<'a>(base: usize, g: u32) -> &'a [Value] {
    let fields = (base + g as usize * CLASS_HEADER_BYTES + NATIVE_CLASS_CELL_LAYOUT.fields_offset) as *const Value;
    // SAFETY: the caller's contract; the cell's fields are `Value`s, written whole.
    unsafe { std::slice::from_raw_parts(fields, region_capacity(g) as usize) }
}

/// Region `k`'s first address, as the record space of its cells.
fn region_start<T>(base: NonNull<u8>, k: u32) -> NonNull<MaybeUninit<T>> {
    // SAFETY: region `k` starts inside the space's range (`SPACE_BYTES`).
    let start = unsafe { NonNull::new_unchecked(base.as_ptr().add(k as usize * REGION_BYTES)) };
    start.cast()
}

/// The granule of local cell `local` in region `region`.
pub(super) const fn granule(region: u32, local: u32) -> u32 {
    debug_assert!((region as usize) < CAPACITY_CLASSES && local < region_cells(region) as u32);
    (region << REGION_SHIFT) | (local << region)
}

/// The region and local cell index of granule `g`.
pub(super) const fn locate(g: u32) -> (u32, u32) {
    let region = g >> REGION_SHIFT;
    (region, (g & REGION_MASK) >> region)
}

impl super::Heap {
    /// The class space's base: granule `g`'s cell is at this plus `g * CLASS_HEADER_BYTES`. Zero until the
    /// space is reserved. Read per heap (`staged::Table::ClassSpace`), never a fixed address.
    #[inline(always)]
    pub fn class_space_base(&self) -> usize { self.class_space.base }

    /// The allocation cursor of cell region `region` (1 to 5), which a class cell claim bumps
    /// (`staged::bump::claim_class_cell`). It is a field of this heap, which stays where it is while the VM
    /// runs; a native entry rereads it with the other record bases.
    #[inline(always)]
    pub fn native_cell_cursor(&self, region: u32) -> usize { self.class_space.cell_cursor_address(region) }
}

/// Whether granule `g` lies in a class cell's region (1 to 5), where an object is a cell until it departs.
/// Region 0 holds the stubs of the record table's objects, which never hold a class.
pub(super) const fn is_cell_granule(g: u32) -> bool {
    g >> REGION_SHIFT != 0
}

/// The fields a cell at granule `granule` holds: its region's capacity, `2^k - 1` for region `k`. A
/// cell's capacity class is its address's region, so no class word records it.
pub(crate) const fn region_capacity(granule: u32) -> u32 {
    let (region, _) = locate(granule);
    assert!((region as usize) < CAPACITY_CLASSES, "granule past the class space");
    region_field_capacity(region) as u32
}

/// The header word of a live class cell: the class id in the low half, zero above.
pub(super) const fn live_header(class: u32) -> u64 {
    class as u64
}

pub(super) const fn header_class(header: u64) -> u32 {
    header as u32
}

/// The header word of a region 0 stub naming object record `record`: the record id in the high half, `NO_CLASS`
/// in the low half (a stub has no class).
pub(super) const fn stub_header(record: u32) -> u64 {
    NO_CLASS as u64 | ((record as u64) << STUB_RECORD_SHIFT)
}

/// The record id a stub's header names (`stub_header`).
pub(super) const fn header_record(header: u64) -> u32 {
    (header >> STUB_RECORD_SHIFT) as u32
}

/// Where a class cell's header holds its extras id (`ExtrasSpace`), above the class half.
pub(super) const EXTRAS_SHIFT: u32 = u32::BITS;

/// The extras id of a live cell's header: `NO_EXTRAS` where the cell has none.
pub(super) const fn header_extras(header: u64) -> u32 {
    (header >> EXTRAS_SHIFT) as u32
}

/// `header` with its class half set to `class` and its high half kept, so a class edit keeps the cell's extras.
pub(super) const fn header_with_class(header: u64, class: u32) -> u64 {
    (header & !(u32::MAX as u64)) | class as u64
}
