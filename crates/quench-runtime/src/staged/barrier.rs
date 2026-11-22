//! The store barrier's mark test, written once over `Stage`: the owner of an object record is
//! unmarked where its mark block exists and its mark bit is clear, which is when a store of a heap
//! value into the record needs no barrier log (`RecordSpace::store_logs`). Each caller decides the
//! immediate case itself, since that is a branch in one stage and a return in another. The one
//! place the mark blocks' layout is read; the b-jit stencil build mounts this file too.

use super::region::Region;
use super::stage::Stage;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;

/// Where a record space's mark block list lies in the dispatch payload: the byte offsets of its
/// block table pointer and of its block count.
#[derive(Clone, Copy)]
pub struct MarkList {
    pub(crate) blocks: usize,
    pub(crate) count: usize,
}

/// Continues where the record `record_id` is unmarked: its block lies below the count `list` names
/// in the dispatch `payload`, and its mark bit is clear. A failed condition leaves at `miss`.
pub fn unmarked<S: Stage>(stage: &mut S, payload: S::Word, list: MarkList, record_id: S::Word,
    miss: S::Miss) -> Option<()> {
    let blocks = stage.load_u64(Region::Dispatch.mutable(), payload, list.blocks as i32);
    let count = stage.load_u64(Region::Dispatch.mutable(), payload, list.count as i32);
    let block = stage.shr_imm(record_id, u64::from(OBJECT.mark_records_per_block.trailing_zeros()));
    let live = stage.ult(block, count);
    stage.guard(live, miss)?;
    let block_offset = stage.mul_imm(block, OBJECT.mark_block_size as u64);
    let block_address = stage.add(blocks, block_offset);
    let within = stage.and_imm(record_id, (OBJECT.mark_records_per_block - 1) as u64);
    let word_index = stage.shr_imm(within, u64::from(OBJECT.mark_word_bits.trailing_zeros()));
    let word_offset = stage.mul_imm(word_index, size_of::<u64>() as u64);
    let word_address = stage.add(block_address, word_offset);
    let marks = stage.load_u64(Region::HeapMeta.mutable(), word_address, OBJECT.mark_offset as i32);
    let bit_index = stage.and_imm(within, (OBJECT.mark_word_bits - 1) as u64);
    let marked = stage.shr(marks, bit_index);
    let bit = stage.and_imm(marked, 1_u64);
    let unset = stage.eq_imm(bit, 0_u64);
    stage.guard(unset, miss)
}
