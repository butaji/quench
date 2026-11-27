//! The bump allocation every tier shares: a record family's cursor run (`RecordCursor`, read
//! through `NATIVE_RECORD_CURSOR_LAYOUT`) hands out its next id while the run lasts. The heap's
//! own allocation (`RecordSpace::cursor_take`) runs it over the concrete stage, and the u-jit's
//! templated allocation (`lane_region::frame_calls::allocate_template`) emits it.

use super::Stage;
use super::region::Region;
use super::layout_offsets::{CLASS_CELL_GRANULE_BYTES, NATIVE_CLASS_CELL_LAYOUT, NATIVE_OBJECT_LAYOUT as OBJECT, NATIVE_RECORD_CURSOR_LAYOUT as CURSOR};
use super::object_encoding::{REGION_SHIFT, region_field_capacity};
use super::value_encoding::{DIRECT_OBJECT_TAG, TAG_DELETED};

/// The cursor at `cursor`'s next id and whether its run still has it.
#[inline]
fn cursor_next<S: Stage>(stage: &mut S, cursor: S::Word) -> (S::Word, S::Cond) {
    let next = stage.load_u32(Region::HeapMeta.mutable(), cursor, CURSOR.next_offset as i32);
    let limit = stage.load_u32(Region::HeapMeta.mutable(), cursor, CURSOR.limit_offset as i32);
    let room = stage.ult(next, limit);
    (next, room)
}

/// Moves the cursor at `cursor` past `next`, the id it just handed out.
#[inline]
fn cursor_bump<S: Stage>(stage: &mut S, cursor: S::Word, next: S::Word) {
    let bumped = stage.add_imm(next, 1u64);
    stage.store_u32(Region::HeapMeta.mutable(), cursor, CURSOR.next_offset as i32, bumped);
}

/// The next id of the cursor run at `cursor`, bumped; a spent run leaves at `miss` (the refill is
/// the allocator's slow path).
#[inline(always)]
pub fn cursor_take<S: Stage>(stage: &mut S, cursor: S::Word, miss: S::Miss) -> Option<S::Word> {
    let (next, room) = cursor_next(stage, cursor);
    stage.guard(room, miss)?;
    cursor_bump(stage, cursor, next);
    Some(next)
}

/// What a templated object allocation writes: the record table and its stride, and the template's
/// layout id (0 where the site has no template yet) and payload word.
#[derive(Clone, Copy)]
pub struct Template<W> {
    pub records: W,
    pub record_size: u64,
    pub layout: W,
    pub words: W,
    pub tag: u64,
}

/// A templated object record from the cursor run at `cursor`: its payload word and layout written,
/// answered as the tagged value. A spent run or a site with no template leaves at `miss`.
#[inline(always)]
pub fn bump_record<S: Stage>(stage: &mut S, cursor: S::Word, template: Template<S::Word>, miss: S::Miss) -> Option<S::Word> {
    let (next, room) = cursor_next(stage, cursor);
    let templated = stage.ne_imm(template.layout, 0u64);
    let inline = stage.all(room, templated);
    stage.guard(inline, miss)?;
    cursor_bump(stage, cursor, next);
    let stride = stage.mul_imm(next, template.record_size);
    let record = stage.add(template.records, stride);
    stage.store_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32, template.words);
    stage.store_u32(Region::ShapeWord.mutable(), record, OBJECT.layout_offset as i32, template.layout);
    Some(stage.or_imm(next, template.tag))
}

/// The words a class cell claim reads: region `k`'s allocation cursor (`Heap::native_cell_cursor`), the class space's
/// base, and the class id the header takes.
#[derive(Clone, Copy)]
pub struct ClassCell<W> {
    pub cursor: W,
    pub cells: W,
    pub class: W,
}

/// An ordinary empty object claimed inline (`EMPTY_OBJECT_ROW`, `Allocation::Empty`): the empty object's class cell,
/// a region 1 cell of the class `cell.class` (captured with the prototype it was made for, and nonzero where
/// captured), through `claim_class_cell` with no fields, so its one slack field is `Value::DELETED`. The class names
/// the shape and prototype link, so nothing else is written. A zero class, or a spent run, leaves at `miss` before
/// anything is written.
#[inline(always)]
pub fn claim_empty_object<S: Stage>(stage: &mut S, cell: ClassCell<S::Word>, miss: S::Miss) -> Option<S::Word> {
    let captured = stage.ne_imm(cell.class, 0u64);
    stage.guard(captured, miss)?;
    let deleted = stage.constant(TAG_DELETED);
    claim_class_cell(stage, 1, cell, &[], deleted, miss)
}

/// A class cell of region `region` from its cursor run, the bump `ClassSpace::claim_cell` makes over the heap. The
/// cursor's next local cell is granule `(region << REGION_SHIFT) + next << region`: its header takes the class id, its
/// fields `fields` in slot order, and its slack `deleted` (`Value::DELETED`). The answer is the direct object word of
/// the granule. The object is fresh, so no barrier; the run was charged when claimed. A spent run leaves at `miss`
/// before anything is written.
#[inline(always)]
pub fn claim_class_cell<S: Stage>(stage: &mut S, region: u32, cell: ClassCell<S::Word>, fields: &[S::Word], deleted: S::Word,
    miss: S::Miss) -> Option<S::Word> {
    debug_assert!(fields.len() <= region_field_capacity(region));
    let (next, room) = cursor_next(stage, cell.cursor);
    stage.guard(room, miss)?;
    cursor_bump(stage, cell.cursor, next);
    let local = stage.mul_imm(next, 1u64 << region);
    let granule = stage.add_imm(local, u64::from(region << REGION_SHIFT));
    let offset = stage.mul_imm(granule, CLASS_CELL_GRANULE_BYTES as u64);
    let base = stage.add(cell.cells, offset);
    stage.store_u64(Region::HeapMeta.mutable(), base, NATIVE_CLASS_CELL_LAYOUT.header_offset as i32, cell.class);
    for slot in 0..region_field_capacity(region) {
        let value = fields.get(slot).copied().unwrap_or(deleted);
        let at = (NATIVE_CLASS_CELL_LAYOUT.fields_offset + slot * NATIVE_CLASS_CELL_LAYOUT.field_size) as i32;
        stage.store_u64(Region::Payload.mutable(), base, at, value);
    }
    Some(stage.or_imm(granule, DIRECT_OBJECT_TAG))
}
