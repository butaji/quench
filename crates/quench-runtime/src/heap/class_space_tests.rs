use super::class_space::{
    ClassCell0, ClassCell1, ClassCell3, ClassCell5, ClassSpace, granule, header_class, header_record,
    live_header, locate, region_capacity, region_cells, stub_header,
};
use super::layouts::LAYOUT_CAPACITY;
use crate::staged::layout_offsets::NATIVE_CLASS_CELL_LAYOUT;
use crate::staged::object_encoding::{CAPACITY_CLASSES, CLASS_HEADER_BYTES, NO_CLASS, REGION_SHIFT};
use std::mem::size_of;

#[test]
fn a_granule_names_its_region_and_local_cell() {
    for region in 0..CAPACITY_CLASSES as u32 {
        let cells = region_cells(region) as u32;
        assert_eq!(granule(region, 0), region << REGION_SHIFT);
        for local in [0, 1, cells / 2, cells - 1] {
            assert_eq!(locate(granule(region, local)), (region, local));
        }
    }
}

#[test]
fn a_region_packs_its_cells_end_to_end_up_to_the_next_region() {
    for region in 0..CAPACITY_CLASSES as u32 {
        assert_eq!(granule(region, 1) - granule(region, 0), 1 << region);
    }
    for region in 0..CAPACITY_CLASSES as u32 - 1 {
        let end = granule(region, region_cells(region) as u32 - 1) + (1 << region);
        assert_eq!(end, granule(region + 1, 0));
    }
}

#[test]
fn a_cell_granule_names_the_capacity_of_its_region() {
    for region in 0..CAPACITY_CLASSES as u32 {
        let capacity = (1u32 << region) - 1;
        assert_eq!(region_capacity(granule(region, 0)), capacity);
        assert_eq!(region_capacity(granule(region, region_cells(region) as u32 - 1)), capacity);
        assert_eq!(capacity as usize + 1, 1 << region);
    }
    assert_eq!(region_capacity(granule(5, 0)), 31);
}

#[test]
fn a_cell_is_one_header_granule_and_its_capacity_in_fields() {
    assert_eq!(size_of::<ClassCell0>(), CLASS_HEADER_BYTES);
    assert_eq!(size_of::<ClassCell1>(), 2 * CLASS_HEADER_BYTES);
    assert_eq!(size_of::<ClassCell3>(), 8 * CLASS_HEADER_BYTES);
    assert_eq!(size_of::<ClassCell5>(), 32 * CLASS_HEADER_BYTES);
    assert_eq!(NATIVE_CLASS_CELL_LAYOUT.fields_offset, std::mem::offset_of!(ClassCell3, fields));
}

#[test]
fn a_live_header_holds_only_its_class_id() {
    let header = live_header(0x1234);
    assert_eq!(header >> 32, 0);
    assert_eq!(header_class(header), 0x1234);
}

#[test]
fn a_stub_header_holds_no_class_and_the_record_id() {
    let header = stub_header(77);
    assert_eq!(header as u32, NO_CLASS);
    assert_eq!(header >> 32, 77);
    assert_eq!(header_record(header), 77);
}

#[test]
fn no_class_is_past_the_class_table() {
    assert!(NO_CLASS as usize >= LAYOUT_CAPACITY);
}

#[test]
fn each_region_starts_at_its_granule() {
    let mut space = ClassSpace::default();
    assert!(space.ensure_reserved());
    assert!(space.ensure_reserved());
    let cell = |region, local| space.cell_ptr(granule(region, local)).cast_const();
    assert_eq!(space.regions.0.base_ptr(), Some(cell(0, 0).cast::<ClassCell0>()));
    assert_eq!(space.regions.1.base_ptr(), Some(cell(1, 0).cast::<ClassCell1>()));
    assert_eq!(space.regions.5.base_ptr(), Some(cell(5, 0).cast::<ClassCell5>()));
    assert_eq!(cell(2, 3) as usize, space.base + granule(2, 3) as usize * CLASS_HEADER_BYTES);
}
