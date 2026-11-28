//! Object records and cells: the record of a bits word, the cell of a granule, and cell fields.

use super::*;

/// The address of object record `id` in the record table at `records`. Called only by `object_record_of`,
/// `direct_object_record`, and places that already hold a record id (not a `Value`).
#[inline(always)]
pub fn object_record_at<S: Stage>(stage: &mut S, records: S::Word, id: S::Word) -> S::Word {
    let offset = stage.mul_imm(id, OBJECT.record_size as u64);
    stage.add(records, offset)
}

/// The address of the record the direct object reference `bits` names, in the record table at `records`;
/// `miss` where the object is a live cell (it has no record). Its one hop is `object_record_id_of`.
#[inline(always)]
pub fn object_record_of<S: Stage>(stage: &mut S, records: S::Word, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    let id = object_record_id_of(stage, bits, miss)?;
    Some(object_record_at(stage, records, id))
}

/// The record id the direct object reference `bits` names: its granule's stub hop (`stub_record_id`), `miss`
/// where the granule is a live cell's.
#[inline(always)]
pub fn object_record_id_of<S: Stage>(stage: &mut S, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    let granule = super::super::value::direct_object_id(stage, bits);
    stub_record_id(stage, granule, miss)
}

/// The record id of the stub at granule `granule` (a word, as a prototype link or a direct object reference
/// holds it): the high half of the stub's header. A granule in a cell region (`object_encoding::REGION_SHIFT`)
/// names a live class cell, which has no record, so it leaves at `miss`; only region 0 holds stubs. The
/// guard is the granule's region, one shift and a compare, not the header's class.
#[inline(always)]
pub fn stub_record_id<S: Stage>(stage: &mut S, granule: S::Word, miss: S::Miss) -> Option<S::Word> {
    let region = stage.shr_imm(granule, u64::from(super::super::object_encoding::REGION_SHIFT));
    let stub = stage.eq_imm(region, 0u32);
    stage.guard(stub, miss)?;
    let cell = cell_at_granule(stage, granule);
    let header = stage.load_u64(Region::ShapeWord.mutable(), cell, 0);
    Some(stage.shr_imm(header, u64::from(u32::BITS)))
}

/// The address of the class cell at granule `granule` (a word, as a prototype link or a direct object
/// reference holds it): `class_base + granule * CLASS_HEADER_BYTES`, the one addressing mode of a cell.
#[inline(always)]
pub fn cell_at_granule<S: Stage>(stage: &mut S, granule: S::Word) -> S::Word {
    let base = stage.table(Table::ClassSpace);
    let offset = stage.mul_imm(granule, CLASS_HEADER_BYTES as u64);
    stage.add(base, offset)
}

/// The address of the class cell the direct object reference `bits` names (`cell_at_granule` of its
/// granule). Its header holds the object's class (`cell_class`): a direct object's granule in a cell region
/// is always a live cell, so a class-keyed reader needs no test of the object's form.
#[inline(always)]
pub fn cell_of<S: Stage>(stage: &mut S, bits: S::Word) -> S::Word {
    let granule = super::super::value::direct_object_id(stage, bits);
    cell_at_granule(stage, granule)
}

/// The class id in the low half of the header of the cell at `cell`. A class guard (`IcOp::GuardClass`) compares
/// it, so a word that names no live cell's class misses there.
#[inline(always)]
pub fn cell_class<S: Stage>(stage: &mut S, cell: S::Word) -> S::Word {
    stage.load_u32(Region::ShapeWord.mutable(), cell, NATIVE_CLASS_CELL_LAYOUT.class_offset as i32)
}

/// The fields of the class cell at `cell`, which holds `storage_len` values: value `i` is the tagged
/// word at `cell + 8 + 8 * i` (`NATIVE_CLASS_CELL_LAYOUT.fields_offset`). A cell's fields are one flat
/// run, so a class-fixed object's layout is this and no table entry.
#[inline(always)]
pub fn cell_fields<S: Stage>(stage: &mut S, cell: S::Word, storage_len: usize) -> Layout<S::Word, S::Cond> {
    let fields = stage.add_imm(cell, NATIVE_CLASS_CELL_LAYOUT.fields_offset as u64);
    Layout { slots: Slots::Flat(fields), storage_len }
}
