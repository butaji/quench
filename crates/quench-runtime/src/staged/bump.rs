//! The bump allocation every tier shares: a record family's cursor run (`RecordCursor`, read
//! through `NATIVE_RECORD_CURSOR_LAYOUT`) hands out its next id while the run lasts. The heap's
//! own allocation (`RecordSpace::cursor_take`) runs it over the concrete stage, and the u-jit's
//! templated allocation (`lane_region::frame_calls::allocate_template`) emits it.

use super::Stage;
use super::region::Region;
use crate::heap::{NATIVE_OBJECT_LAYOUT as OBJECT, NATIVE_RECORD_CURSOR_LAYOUT as CURSOR};

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
