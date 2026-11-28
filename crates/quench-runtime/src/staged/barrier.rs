//! The store barrier's mark test, written once over `Stage`: the owner of an object record is
//! unmarked where its mark block exists and its mark bit is clear, which is when a store of a heap
//! value into the record needs no barrier log (`RecordSpace::store_logs`). Each caller decides the
//! immediate case itself, since that is a branch in one stage and a return in another. The one
//! place the mark blocks' layout is read; the b-jit stencil build mounts this file too.

use super::object_encoding::{REGION_MASK, REGION_SHIFT};
use super::region::Region;
use super::stage::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::value::ValueLayout;

/// Where a record space's mark block list lies in the dispatch payload: the byte offsets of its
/// block table pointer and of its block count.
#[derive(Clone, Copy)]
pub struct MarkList {
    pub blocks: usize,
    pub count: usize,
}

/// Continues where the record `record_id` is unmarked: its block lies below the count `list` names
/// in the dispatch `payload`, and its mark bit is clear. A failed condition leaves at `miss`.
#[inline(always)]
pub fn unmarked<S: Stage>(stage: &mut S, payload: S::Word, list: MarkList, record_id: S::Word,
    miss: S::Miss) -> Option<()> {
    let blocks = stage.load_u64(Region::Dispatch.mutable(), payload, list.blocks as i32);
    let count = stage.load_u64(Region::Dispatch.mutable(), payload, list.count as i32);
    unmarked_in(stage, blocks, count, record_id, miss)
}

/// The bit test `unmarked` and `class_unmarked` share: `record_id`'s block lies below `count` in the block table
/// at `blocks`, and its mark bit is clear. A failed condition leaves at `miss`.
#[inline(always)]
fn unmarked_in<S: Stage>(stage: &mut S, blocks: S::Word, count: S::Word, record_id: S::Word,
    miss: S::Miss) -> Option<()> {
    let block = stage.shr_imm(record_id, u64::from(OBJECT.mark_records_per_block.trailing_zeros()));
    let live = stage.ult(block, count);
    stage.guard(live, miss)?;
    let bit = mark_bit(stage, blocks, record_id);
    let unset = stage.eq_imm(bit, 0_u64);
    stage.guard(unset, miss)
}

/// Continues where the class cell at `granule` is unmarked (`ClassSpace::store_logs`): its region's mark list is the
/// (blocks, count) pair at byte `16 * region` of the table at `table_offset` in the dispatch `payload`
/// (`JsStepHead::class_mark_lists`), and the cell's local index in that region names its mark bit, as `locate` does.
/// `granule` names a live cell, so its region is one the table holds. A failed condition leaves at `miss`.
#[inline(always)]
pub fn class_unmarked<S: Stage>(stage: &mut S, payload: S::Word, table_offset: usize, granule: S::Word,
    miss: S::Miss) -> Option<()> {
    let region = stage.shr_imm(granule, u64::from(REGION_SHIFT));
    let within = stage.and_imm(granule, u64::from(REGION_MASK));
    let local = stage.shr(within, region);
    let entry_offset = stage.mul_imm(region, (2 * size_of::<usize>()) as u64);
    let entry = stage.add(payload, entry_offset);
    let blocks = stage.load_u64(Region::Dispatch.mutable(), entry, table_offset as i32);
    let count = stage.load_u64(Region::Dispatch.mutable(), entry, (table_offset + size_of::<usize>()) as i32);
    unmarked_in(stage, blocks, count, local, miss)
}

/// The mark bit (0 or 1) of the record `record_id` in the block table at `blocks`, whose block the
/// caller knows exists. The heap's own barrier (`RecordSpace::store_logs`) reads it over the
/// concrete stage, so every tier and the interpreter test the same bit the same way.
#[inline(always)]
pub fn mark_bit<S: Stage>(stage: &mut S, blocks: S::Word, record_id: S::Word) -> S::Word {
    let block = stage.shr_imm(record_id, u64::from(OBJECT.mark_records_per_block.trailing_zeros()));
    let block_offset = stage.mul_imm(block, OBJECT.mark_block_size as u64);
    let block_address = stage.add(blocks, block_offset);
    let within = stage.and_imm(record_id, (OBJECT.mark_records_per_block - 1) as u64);
    let word_index = stage.shr_imm(within, u64::from(OBJECT.mark_word_bits.trailing_zeros()));
    let word_offset = stage.mul_imm(word_index, size_of::<u64>() as u64);
    let word_address = stage.add(block_address, word_offset);
    let marks = stage.load_u64(Region::HeapMeta.mutable(), word_address, OBJECT.mark_offset as i32);
    let bit_index = stage.and_imm(within, (OBJECT.mark_word_bits - 1) as u64);
    let marked = stage.shr(marks, bit_index);
    stage.and_imm(marked, 1_u64)
}

/// Continues where storing `value` into the record `record_id` logs nothing
/// (`RecordSpace::store_logs` while no counts are kept): an immediate value names no heap record
/// and joins no log; a heap value needs its owner `unmarked`. A marked owner leaves at `miss`.
#[inline(always)]
pub fn value_unlogged<S: Stage>(stage: &mut S, payload: S::Word, list: MarkList, record_id: S::Word,
    value: S::Word, miss: S::Miss) -> Option<()> {
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    let zero = stage.constant(0);
    stage.choose_leaving(heap, #[inline(always)] |stage| unmarked(stage, payload, list, record_id, miss).map(|()| zero),
        #[inline(always)] |_| Some(zero)).map(drop)
}

/// Continues where storing `value` into the direct array `base` logs nothing, the whole rule of
/// `RecordSpace::store_logs`: a heap value needs its owner a direct array and `unmarked`; an
/// immediate needs that too only while old records keep counts (the `counts` bit of the payload's
/// store-watch word at byte `watch`), when overwriting a reference deletes an edge. An immediate
/// with counts clear logs nothing whatever `base` is, so that path reads no base (an entry view's
/// row holds none). Otherwise leaves at `miss`.
#[inline(always)]
pub fn counted_store_unlogged<S: Stage>(stage: &mut S, payload: S::Word, (watch, counts): (usize, u64), list: MarkList,
    base: S::Word, value: S::Word, miss: S::Miss) -> Option<()> {
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    let word = stage.load_u64(Region::Dispatch.mutable(), payload, watch as i32);
    let counted_bit = stage.and_imm(word, counts);
    let counted = stage.ne_imm(counted_bit, 0_u64);
    let tested = stage.any(heap, counted);
    let zero = stage.constant(0);
    stage.choose_leaving(tested, #[inline(always)] |stage| {
        let direct = super::value::is_direct_array(stage, base);
        stage.guard(direct, miss)?;
        let record_id = stage.and_imm(base, u64::from(u32::MAX));
        unmarked(stage, payload, list, record_id, miss).map(|()| zero)
    }, #[inline(always)] |_| Some(zero)).map(drop)
}

/// The observation half of the store gate: the dispatch `payload`'s store-watch word (at byte `watch`)
/// is clear, so nothing observes the store (slot types tracked, reference counts kept). Otherwise leaves at `miss`.
/// A store into a holder the code proved fresh (`LocalProofs::fresh`) needs this half alone, since the barrier's
/// log is elided for it.
#[inline(always)]
pub fn store_untracked<S: Stage>(stage: &mut S, payload: S::Word, watch: usize, miss: S::Miss) -> Option<()> {
    let tracked = stage.load_u64(Region::Dispatch.mutable(), payload, watch as i32);
    let untracked = stage.eq_imm(tracked, 0_u64);
    stage.guard(untracked, miss)
}

/// The store gate every tier shares: the dispatch `payload`'s store-watch word (at byte `watch`)
/// is clear, so nothing observes the store, and `value_unlogged` holds. Otherwise leaves at `miss`.
#[inline(always)]
pub fn store_unlogged<S: Stage>(stage: &mut S, payload: S::Word, watch: usize, list: MarkList, record_id: S::Word,
    value: S::Word, miss: S::Miss) -> Option<()> {
    store_untracked(stage, payload, watch, miss)?;
    value_unlogged(stage, payload, list, record_id, value, miss)
}

/// `store_unlogged` of a store whose value the slot's type already holds (`Stage::held_store_gate`):
/// slot observation sees nothing of it, so the watch word's tracking bit is not asked; only the
/// barrier's log is, as `RecordSpace::store_logs` reads it: while old records keep counts (bit
/// `counts` of the watch word at byte `watch`) any value needs its owner `unmarked`, else
/// `value_unlogged` decides.
#[inline(always)]
pub fn held_unlogged<S: Stage>(stage: &mut S, payload: S::Word, (watch, counts): (usize, u64), list: MarkList,
    record_id: S::Word, value: S::Word, miss: S::Miss) -> Option<()> {
    let word = stage.load_u64(Region::Dispatch.mutable(), payload, watch as i32);
    let kept = stage.and_imm(word, counts);
    let counted = stage.ne_imm(kept, 0_u64);
    let zero = stage.constant(0);
    stage.choose_leaving(counted, #[inline(always)] |stage| unmarked(stage, payload, list, record_id, miss).map(|()| zero),
        #[inline(always)] |stage| value_unlogged(stage, payload, list, record_id, value, miss).map(|()| zero)).map(drop)
}

/// `value_unlogged` of a class cell (`class_unmarked` for its owner's mark), the class variant of `RecordSpace::store_logs`
/// for the cell `granule`. An immediate value joins no log; a heap value needs its cell unmarked.
#[inline(always)]
pub fn class_value_unlogged<S: Stage>(stage: &mut S, payload: S::Word, table_offset: usize, granule: S::Word,
    value: S::Word, miss: S::Miss) -> Option<()> {
    let tag = stage.and_imm(value, ValueLayout::TAG_MASK);
    let heap = stage.eq_imm(tag, ValueLayout::HEAP_TAG);
    let zero = stage.constant(0);
    stage.choose_leaving(heap, #[inline(always)] |stage| class_unmarked(stage, payload, table_offset, granule, miss).map(|()| zero),
        #[inline(always)] |_| Some(zero)).map(drop)
}

/// `store_unlogged` of a store into the class cell `granule`: the class variant, with `class_value_unlogged` for the
/// value's log. Otherwise leaves at `miss`.
#[inline(always)]
pub fn class_store_unlogged<S: Stage>(stage: &mut S, payload: S::Word, watch: usize, table_offset: usize, granule: S::Word,
    value: S::Word, miss: S::Miss) -> Option<()> {
    store_untracked(stage, payload, watch, miss)?;
    class_value_unlogged(stage, payload, table_offset, granule, value, miss)
}

/// `held_unlogged` of a store into the class cell `granule`: the class variant, the same rule over the cell's region's
/// mark list (`class_unmarked`) where `held_unlogged` reads the object records'.
#[inline(always)]
pub fn class_held_unlogged<S: Stage>(stage: &mut S, payload: S::Word, (watch, counts): (usize, u64), table_offset: usize,
    granule: S::Word, value: S::Word, miss: S::Miss) -> Option<()> {
    let word = stage.load_u64(Region::Dispatch.mutable(), payload, watch as i32);
    let kept = stage.and_imm(word, counts);
    let counted = stage.ne_imm(kept, 0_u64);
    let zero = stage.constant(0);
    stage.choose_leaving(counted, #[inline(always)] |stage| class_unmarked(stage, payload, table_offset, granule, miss).map(|()| zero),
        #[inline(always)] |stage| class_value_unlogged(stage, payload, table_offset, granule, value, miss).map(|()| zero)).map(drop)
}
