//! Object records and cells: the record of a bits word, the cell of a granule, and cell fields.

use super::*;

/// The address of object record `id` in the record table at `records`. Called only by `object_record_of`,
/// `direct_object_record`, and places that already hold a record id (not a `Value`).
#[inline(always)]
pub fn object_record_at<S: Stage>(stage: &mut S, records: S::Word, id: S::Word) -> S::Word {
    let offset = stage.mul_imm(id, OBJECT.record_size as u64);
    stage.add(records, offset)
}

/// The address of the object record the direct object reference `bits` names, in the record table at
/// `records`; `miss` where the object is live (it has no record). The only staged resolver
/// (`object_record_id_of` is its hop's first half).
#[inline(always)]
pub fn object_record_of<S: Stage>(stage: &mut S, records: S::Word, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    let id = object_record_id_of(stage, bits, miss)?;
    Some(object_record_at(stage, records, id))
}

/// The record id the direct object reference `bits` names: its granule's departed hop
/// (`departed_record_id`), `miss` where the object is live.
#[inline(always)]
pub fn object_record_id_of<S: Stage>(stage: &mut S, bits: S::Word, miss: S::Miss) -> Option<S::Word> {
    let granule = super::super::value::direct_object_id(stage, bits);
    departed_record_id(stage, granule, miss)
}

/// The record id of the object whose stub granule is `granule` (a word, as a prototype link or a
/// direct object reference holds it), where that object departed: the high half of its class cell's
/// header (`object_encoding::departed_header`). The class guard is that the cell's class is
/// `DEPARTED_CLASS`; a live cell (a class id, no record) leaves at `miss`, and the slow path decides it
/// (`ObjectRef::Cell`). R4-D1b(d) makes the guard's immediate the expected class and deletes the record
/// shape guard, so each read keeps one guard.
#[inline(always)]
pub fn departed_record_id<S: Stage>(stage: &mut S, granule: S::Word, miss: S::Miss) -> Option<S::Word> {
    let cell = cell_at_granule(stage, granule);
    let class = cell_class(stage, cell);
    let departed = stage.eq_imm(class, u64::from(DEPARTED_CLASS));
    stage.guard(departed, miss)?;
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
/// granule). Its header is the object's class word (`cell_class`) while the object is live, and
/// `departed_header` once it departed: a reader tests the class before it takes either path.
#[inline(always)]
#[allow(dead_code)] // read by the class readers the R4-D2..D4 callers adopt
pub fn cell_of<S: Stage>(stage: &mut S, bits: S::Word) -> S::Word {
    let granule = super::super::value::direct_object_id(stage, bits);
    cell_at_granule(stage, granule)
}

/// The class id in the low half of the header of the cell at `cell`. It is `DEPARTED_CLASS` where the
/// object departed, which no class guard names (`IcOp::GuardClass`), so a departed object misses there.
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
