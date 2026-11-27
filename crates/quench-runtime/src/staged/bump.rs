//! The bump allocation every tier shares: a record family's cursor run (`RecordCursor`, read
//! through `NATIVE_RECORD_CURSOR_LAYOUT`) hands out its next id while the run lasts. The heap's
//! own allocation (`RecordSpace::cursor_take`) runs it over the concrete stage, and the u-jit's
//! templated allocation (`lane_region::frame_calls::allocate_template`) emits it.

use super::Stage;
use super::region::Region;
use super::layout_offsets::{CLASS_CELL_GRANULE_BYTES, NATIVE_CLASS_CELL_LAYOUT, NATIVE_OBJECT_LAYOUT as OBJECT, NATIVE_RECORD_CURSOR_LAYOUT as CURSOR, OBJECT_RECORD_STUB};
use super::object_encoding::departed_header_word;
use super::value_encoding::DIRECT_OBJECT_TAG;

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

/// The words an inline empty object claim reads: the object space's record cursor, the region 0 stub cursor (both
/// `RecordCursor` addresses), the class space's base, the object record base, and the captured template's layout and
/// payload words.
#[derive(Clone, Copy)]
pub struct EmptyObject<W> {
    pub object_cursor: W,
    pub stub_cursor: W,
    pub cells: W,
    pub records: W,
    pub layout: W,
    pub words: W,
}

/// An ordinary empty object claimed inline (`EMPTY_OBJECT_ROW`): one record id from the object cursor and one granule
/// from the region 0 stub cursor, both bumped. The stub's cell takes `departed_header_word(id)`
/// (`object_encoding`: `DEPARTED_CLASS` low, the id above `CLASS_RECORD_SHIFT`), the record takes the
/// template's payload and layout and its stub granule, and the answer is the direct object word of the granule. The
/// object is fresh, so no barrier. Both runs were occupied and charged when claimed, so no occupancy is written. A
/// zero template or a spent run leaves at `miss` before anything is written.
#[inline(always)]
pub fn claim_empty_object<S: Stage>(stage: &mut S, object: EmptyObject<S::Word>, miss: S::Miss) -> Option<S::Word> {
    // The cursor pointers are always filled while a lane runs (`idle`, `reread_object_bases`), so only the template,
    // which is captured lazily, is tested before the runs are read.
    let templated = stage.ne_imm(object.layout, 0u64);
    stage.guard(templated, miss)?;
    let (next, room) = cursor_next(stage, object.object_cursor);
    let (granule, stub_room) = cursor_next(stage, object.stub_cursor);
    let room = stage.all(room, stub_room);
    stage.guard(room, miss)?;
    cursor_bump(stage, object.object_cursor, next);
    cursor_bump(stage, object.stub_cursor, granule);
    let header = departed_header_word(stage, next);
    let cell_offset = stage.mul_imm(granule, CLASS_CELL_GRANULE_BYTES as u64);
    let cell = stage.add(object.cells, cell_offset);
    stage.store_u64(Region::HeapMeta.mutable(), cell, NATIVE_CLASS_CELL_LAYOUT.header_offset as i32, header);
    let stride = stage.mul_imm(next, OBJECT.record_size as u64);
    let record = stage.add(object.records, stride);
    stage.store_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32, object.words);
    stage.store_u32(Region::ShapeWord.mutable(), record, OBJECT.layout_offset as i32, object.layout);
    stage.store_u32(Region::Records.mutable(), record, OBJECT_RECORD_STUB as i32, granule);
    Some(stage.or_imm(granule, DIRECT_OBJECT_TAG))
}
