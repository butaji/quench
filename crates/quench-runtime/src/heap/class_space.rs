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

use super::records::{AddressRange, RecordSpace};
use crate::staged::layout_offsets::NATIVE_CLASS_CELL_LAYOUT;
use crate::staged::object_encoding::{
    CAPACITY_CLASSES, CLASS_HEADER_BYTES, CLASS_RECORD_SHIFT, DEPARTED_CLASS, REGION_MASK, REGION_SHIFT, departed_header,
};
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

    /// Frees the unallocated rest of region 0's cursor run and empties the cursor, returning how many stubs
    /// that was (`RecordSpace::retire_cursor`). The inline empty object allocation bumps this run, so every
    /// collection retires it with the record cursors, before occupancy or a snapshot reads region 0.
    pub(super) fn retire_stub_cursor(&mut self) -> usize {
        self.regions.0.retire_cursor()
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
    (1 << region) - 1
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
