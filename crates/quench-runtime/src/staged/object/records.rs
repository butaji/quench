//! Cells: the cell of a granule and its class word and fields. An ordinary object is a class cell; the
//! object record table is gone, so no hop reaches one.

use super::*;

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
