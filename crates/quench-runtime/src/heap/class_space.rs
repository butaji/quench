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

use super::layouts::{class_link, class_shape};
use super::pacing::PromotionAge;
use super::records::{AddressRange, ParallelRecordMarks, RecordFamily, RecordId, RecordSpace, RecordValue};
use crate::staged::layout_offsets::NATIVE_CLASS_CELL_LAYOUT;
use crate::staged::object_encoding::{
    CAPACITY_CLASSES, CLASS_HEADER_BYTES, CLASS_RECORD_SHIFT, DEPARTED_CLASS, REGION_MASK, REGION_SHIFT, departed_header,
    region_field_capacity,
};
use crate::value::Value;
use crate::value_vec::ValueArena;
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

/// The cell regions' parallel mark views, one per region `1..CAPACITY_CLASSES` (`ParallelRecordMarks`).
#[derive(Clone, Copy)]
pub(super) struct ClassCellMarks<'space> {
    regions: (
        ParallelRecordMarks<'space, ClassCell1>,
        ParallelRecordMarks<'space, ClassCell2>,
        ParallelRecordMarks<'space, ClassCell3>,
        ParallelRecordMarks<'space, ClassCell4>,
        ParallelRecordMarks<'space, ClassCell5>,
    ),
}

impl ClassCellMarks<'_> {
    /// Marks cell `g` for a worker if it is new (`ParallelRecordMarks::mark_if_new`). A stub (region 0) has no
    /// mark of its own, as `ClassSpace::mark_cell`: it answers false.
    pub(super) fn mark_if_new(&self, g: u32) -> bool {
        let (region, local) = locate(g);
        let regions = &self.regions;
        match region {
            0 => false,
            1 => regions.0.mark_if_new(local),
            2 => regions.1.mark_if_new(local),
            3 => regions.2.mark_if_new(local),
            4 => regions.3.mark_if_new(local),
            5 => regions.4.mark_if_new(local),
            _ => unreachable!("a parallel mark of a granule past the cell regions"),
        }
    }
}

/// The bytes a region's granules span, and the bytes of the whole space.
const REGION_BYTES: usize = CLASS_HEADER_BYTES << REGION_SHIFT;
const SPACE_BYTES: usize = REGION_BYTES * CAPACITY_CLASSES;

/// The class space of a heap: empty until `ensure_reserved` maps its range.
#[derive(Default)]
pub(super) struct ClassSpace {
    /// The range's first address, which is region 0's base; 0 while unreserved.
    pub(super) base: usize,
    pub(super) regions: (Region0, Region1, Region2, Region3, Region4, Region5),
    /// The range the regions are cut from. It is the last field, so it unmaps after them.
    range: Option<AddressRange>,
}

impl ClassSpace {
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

    /// Claims a region 0 stub for object record `record`: the cell's header is `departed_header(record)`,
    /// so its granule names the record through `record_of`. The stub is claimed from region 0's cursor
    /// (`RecordSpace::bump_id`), charged as an allocation. `None` where the space is full.
    pub(super) fn claim_stub(&mut self, record: u32, allocated: &mut usize) -> Option<u32> {
        if !self.ensure_reserved() {
            return None;
        }
        let local = self.regions.0.bump_id(allocated)?;
        // SAFETY: `local` is a claimed, uninitialized region 0 cell, written whole here.
        unsafe { self.regions.0.record_ptr(local).write(ClassCell0 { header: departed_header(record), fields: [] }) };
        Some(granule(0, local))
    }

    /// Claims a class cell of region `region` for class `class`: its header the live class id, its fields `fields`
    /// in slot order and the slack `Value::DELETED`. The cell comes from the region's cursor (`RecordSpace::bump_id`),
    /// charged as an allocation. `None` where the region is full. `fields` must fit the region.
    pub(super) fn claim_cell(&mut self, region: u32, class: u32, fields: &[Value], allocated: &mut usize) -> Option<u32> {
        debug_assert!((1..CAPACITY_CLASSES as u32).contains(&region) && fields.len() <= region_field_capacity(region));
        if !self.ensure_reserved() {
            return None;
        }
        let local = with_region_mut!(self.regions, region, |space| space.bump_id(allocated))?;
        let granule = granule(region, local);
        let cell = self.cell_ptr(granule);
        // SAFETY: `granule` is a claimed, uninitialized cell of the reserved space, written whole here: its
        // header word, then each of its `region_field_capacity` fields.
        unsafe {
            cell.write(live_header(class));
            let slots = cell.cast::<Value>().add(1);
            for slot in 0..region_field_capacity(region) {
                slots.add(slot).write(fields.get(slot).copied().unwrap_or(Value::DELETED));
            }
        }
        Some(granule)
    }

    /// The address of region `region`'s allocation cursor, which a construction's class cell claim bumps
    /// (`staged::bump::claim_class_cell`).
    pub(super) fn cell_cursor_address(&self, region: u32) -> usize {
        debug_assert!((1..CAPACITY_CLASSES as u32).contains(&region));
        with_region!(self.regions, region, |space| std::ptr::addr_of!(space.cursor) as usize)
    }

    /// Whether region 0's cursor has a stub left, which `claim_stub_quick` would claim.
    #[inline(always)]
    pub(super) fn stub_cursor_open(&self) -> bool {
        self.regions.0.cursor_open()
    }

    /// `claim_stub` where it calls nothing out of line: the next stub of region 0's cursor, with the
    /// header `departed_header(record)`. `None`, with nothing claimed, where the run is spent (the
    /// caller then takes `claim_stub`'s slow path, which refills it).
    #[inline(always)]
    pub(super) fn claim_stub_quick(&mut self, record: u32) -> Option<u32> {
        let local = self.regions.0.cursor_take()?;
        // SAFETY: `local` is a claimed, uninitialized region 0 cell, written whole here.
        unsafe { self.regions.0.record_ptr(local).write(ClassCell0 { header: departed_header(record), fields: [] }) };
        Some(granule(0, local))
    }

    /// Frees stub `g` with its dead record (the record sweep's dispose). A stub is a region 0 cell,
    /// or a class cell that departed (`Heap::depart_class_object`) and is its own stub. Region 0's
    /// stubs die only here, and a departed cell dies with its record.
    pub(super) fn release_stub(&mut self, g: u32) {
        let (region, local) = locate(g);
        match region {
            0 => self.regions.0.release_live(local),
            1 => self.regions.1.release_live(local),
            2 => self.regions.2.release_live(local),
            3 => self.regions.3.release_live(local),
            4 => self.regions.4.release_live(local),
            5 => self.regions.5.release_live(local),
            _ => unreachable!("class cells lie in regions 0 to {}", CAPACITY_CLASSES - 1),
        }
    }

    /// Withholds stub `g`, a dead prototype's, from reuse until `release_withheld_stubs`,
    /// as its record id is withheld: a Value naming the stub names no other object meanwhile.
    pub(super) fn withhold_stub(&mut self, g: u32) {
        let (region, local) = locate(g);
        match region {
            0 => self.regions.0.withhold(local),
            1 => self.regions.1.withhold(local),
            2 => self.regions.2.withhold(local),
            3 => self.regions.3.withhold(local),
            4 => self.regions.4.withhold(local),
            5 => self.regions.5.withhold(local),
            _ => unreachable!("class cells lie in regions 0 to {}", CAPACITY_CLASSES - 1),
        };
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

    /// The header word of granule `g`'s cell: a live class cell's class id (`live_header`), or a
    /// departed one's `departed_header`. The space must be reserved.
    #[inline(always)]
    pub(super) fn header(&self, g: u32) -> u64 {
        // SAFETY: `g` names a cell of the reserved space (`cell_ptr`), whose header is a readable word.
        unsafe { *self.cell_ptr(g) }
    }

    /// Writes granule `g`'s header word. Only the class edges write one (`CellViewMut::set_class`,
    /// `Heap::depart_class_object`).
    #[inline(always)]
    pub(super) fn write_header(&mut self, g: u32, header: u64) {
        // SAFETY: as `header`, and `&mut self` is exclusive.
        unsafe { *self.cell_ptr(g) = header };
    }

    /// Field `slot` of the cell at granule `g`: the word at `8 + 8 * slot` past its header.
    #[inline(always)]
    pub(super) fn field(&self, g: u32, slot: usize) -> Value {
        debug_assert!(slot < region_capacity(g) as usize, "a field lies inside its cell");
        // SAFETY: field `slot` is the `Value` word `slot + 1` of the cell, inside its span (the assert).
        unsafe { self.cell_ptr(g).cast::<Value>().add(1 + slot).read() }
    }

    /// Makes fields `from..region_capacity(g)` of the cell at `g` slack: `Value::DELETED`, which the
    /// collector traces as any field and no object reads.
    pub(super) fn delete_slack(&mut self, g: u32, from: usize) {
        for slot in from..region_capacity(g) as usize {
            // SAFETY: field `slot` lies inside the cell at `g` (`region_capacity`), and `&mut self` is exclusive.
            unsafe { self.cell_ptr(g).cast::<Value>().add(1 + slot).write(Value::DELETED) };
        }
    }

    /// The address of region 0's allocation cursor (`RecordCursor`): its `next` is the granule an inline
    /// empty object allocation takes, and its `limit` the run's end, as the object record cursor's are.
    pub(super) fn stub_cursor_address(&self) -> usize {
        std::ptr::addr_of!(self.regions.0.cursor) as usize
    }

    /// Frees the unallocated rest of every region's cursor run and empties the cursors, returning how many
    /// granules that was (`RecordSpace::retire_cursor`). The inline empty object allocation bumps region 0's run,
    /// and a cell allocation bumps its region's, so every collection retires all of them with the record
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

    /// The class of granule `g`'s cell when the cell is live; `None` where it is departed (its header
    /// names a record) or is a stub. The space must be reserved.
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

    /// The header and the fields of granule `g`'s cell, exclusively (`CellViewMut`). The space must be reserved.
    #[inline(always)]
    pub(super) fn cell_parts_mut(&mut self, g: u32) -> (&mut u64, &mut [Value]) {
        debug_assert!(self.range.is_some());
        let cell = self.cell_ptr(g);
        // SAFETY: the header and the `region_capacity(g)` fields after it lie inside the cell at `g`; they
        // are disjoint, and `&mut self` is exclusive, so no other reference to them exists.
        unsafe { (&mut *cell, std::slice::from_raw_parts_mut(cell.cast::<Value>().add(1), region_capacity(g) as usize)) }
    }

    /// Writes field `slot` of granule `g`'s cell as a `Value`. The caller passed the store's barrier.
    #[inline(always)]
    pub(super) fn set_field(&mut self, g: u32, slot: usize, value: Value) {
        debug_assert!(slot < region_capacity(g) as usize, "a field lies inside its cell");
        // SAFETY: field `slot` lies inside the cell (the assert), and `&mut self` is exclusive.
        unsafe { self.cell_ptr(g).cast::<Value>().add(1 + slot).write(value) };
    }

    /// Marks granule `g`'s cell in its region (`RecordSpace::mark`); whether it was newly marked. A stub (region
    /// 0) has no mark of its own, so there this does nothing and its record is marked instead. A departed cell
    /// in a cell region is marked with its Value: its granule stays live while the Value does.
    pub(super) fn mark_cell(&mut self, g: u32) -> bool {
        let (region, local) = locate(g);
        if region == 0 {
            return false;
        }
        with_region_mut!(self.regions, region, |space| space.mark(local))
    }

    /// Whether cell `g` is marked, live, or aged in its region (`RecordSpace::is_marked`, `is_live`, `is_aged`).
    pub(super) fn is_marked(&self, g: u32) -> bool {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.is_marked(local))
    }

    pub(super) fn is_live(&self, g: u32) -> bool {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.is_live(local))
    }

    pub(super) fn is_aged(&self, g: u32) -> bool {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.is_aged(local))
    }

    /// Whether a store of `stored` into cell `g` needs its region's barrier log (`RecordSpace::store_logs`).
    pub(super) fn store_logs(&self, g: u32, stored: Value) -> bool {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.store_logs(local, stored))
    }

    /// The reference-count primitives of cell `g` (`RecordSpace::rc_*`), for a cell region.
    pub(super) fn rc_pin(&self, g: u32) {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.rc_pin(local))
    }

    pub(super) fn rc_increment(&self, g: u32) {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.rc_increment(local))
    }

    pub(super) fn rc_decrement(&self, g: u32) -> bool {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.rc_decrement(local))
    }

    pub(super) fn rc_count(&self, g: u32) -> u8 {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.rc_count(local))
    }

    #[cfg(test)]
    pub(super) fn rc_exact_count(&self, g: u32) -> u32 {
        let (region, local) = locate(g);
        with_region!(self.regions, region, |space| space.rc_exact_count(local))
    }

    /// The write barrier for a write into live cell `g` (`RecordSpace::barrier`). Only cell regions hold live cells.
    pub(super) fn barrier(&mut self, g: u32, properties: &ValueArena) {
        let (region, local) = locate(g);
        let regions = &mut self.regions;
        match region {
            1 => regions.1.barrier(local, properties),
            2 => regions.2.barrier(local, properties),
            3 => regions.3.barrier(local, properties),
            4 => regions.4.barrier(local, properties),
            5 => regions.5.barrier(local, properties),
            _ => unreachable!("a live class cell lies in a cell region"),
        }
    }

    /// Sweeps the cell regions (`RecordSpace::sweep`): an unmarked cell is freed. Cells own no memory, so
    /// nothing is disposed. Region 0's stubs die with their records and are not swept here.
    pub(super) fn sweep_cells(&mut self, full: bool, age: PromotionAge) -> (usize, usize) {
        let regions = &mut self.regions;
        let sweeps = [
            regions.1.sweep(full, age, false, |_| {}),
            regions.2.sweep(full, age, false, |_| {}),
            regions.3.sweep(full, age, false, |_| {}),
            regions.4.sweep(full, age, false, |_| {}),
            regions.5.sweep(full, age, false, |_| {}),
        ];
        sweeps.into_iter().fold((0, 0), |(live, freed), (swept_live, swept_freed)| (live + swept_live, freed + swept_freed))
    }

    /// The cell regions as record families, which `Heap::record_families` lists after the record spaces.
    pub(super) fn cell_families(&self) -> [&dyn RecordFamily; CELL_REGIONS] {
        [
            &self.regions.1 as &dyn RecordFamily,
            &self.regions.2 as &dyn RecordFamily,
            &self.regions.3 as &dyn RecordFamily,
            &self.regions.4 as &dyn RecordFamily,
            &self.regions.5 as &dyn RecordFamily,
        ]
    }

    pub(super) fn cell_families_mut(&mut self) -> [&mut dyn RecordFamily; CELL_REGIONS] {
        let regions = &mut self.regions;
        [
            &mut regions.1 as &mut dyn RecordFamily,
            &mut regions.2 as &mut dyn RecordFamily,
            &mut regions.3 as &mut dyn RecordFamily,
            &mut regions.4 as &mut dyn RecordFamily,
            &mut regions.5 as &mut dyn RecordFamily,
        ]
    }

    /// The parallel mark's views of the cell regions, taken with the record families' (`ParallelMarkView::new`).
    pub(super) fn parallel_marks(&mut self) -> ClassCellMarks<'_> {
        let regions = &mut self.regions;
        ClassCellMarks {
            regions: (
                regions.1.parallel_marks(),
                regions.2.parallel_marks(),
                regions.3.parallel_marks(),
                regions.4.parallel_marks(),
                regions.5.parallel_marks(),
            ),
        }
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
/// high half of the stub's header, which `departed_header` writes. Reading it is the one hop from a
/// direct object `Value` to its record, so it needs only the base, not the heap.
#[inline(always)]
pub(crate) fn stub_record_at(base: usize, g: u32) -> u32 {
    // SAFETY: a direct object `Value` names a stub granule of the reserved class space, whose
    // header is a readable u64 at `base + g * CLASS_HEADER_BYTES`.
    let header = unsafe { *((base + g as usize * CLASS_HEADER_BYTES) as *const u64) };
    debug_assert_eq!(header_class(header), DEPARTED_CLASS, "a stub names its record only once its object departed");
    header_record(header)
}

/// The class of granule `g`'s cell in the class space at `base`, when the cell is live. `None` where it is
/// departed: its header then names a record (`departed_header`), and the record hop is the answer.
#[inline(always)]
pub(super) fn live_class_at(base: usize, g: u32) -> Option<u32> {
    // SAFETY: a direct object `Value` names a granule of the reserved class space, whose header is a readable u64.
    let header = unsafe { *((base + g as usize * CLASS_HEADER_BYTES) as *const u64) };
    let class = header_class(header);
    (class != DEPARTED_CLASS).then_some(class)
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

    /// The region 0 stub cursor (`ClassSpace::stub_cursor_address`), which an inline empty object allocation
    /// bumps (`JsStepHead::stub_cursor`). It is a field of this heap, which stays where it is while the VM
    /// runs; a native entry rereads it with the other record bases.
    #[inline(always)]
    pub fn native_stub_cursor(&self) -> usize { self.class_space.stub_cursor_address() }

    /// The allocation cursor of cell region `region` (1 to 5), which a class cell claim bumps
    /// (`staged::bump::claim_class_cell`), read as `native_stub_cursor` is.
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

pub(super) const fn header_record(header: u64) -> u32 {
    (header >> CLASS_RECORD_SHIFT) as u32
}
