//! The store barrier's mark test, written once over the machine primitives: whether a store of a
//! value into object record `record_id` needs no barrier log (`RecordSpace::store_logs`): an
//! immediate value names no heap record, and a heap value joins the log only where its owner is
//! marked. The one place a specializer reads the mark blocks' layout.

use super::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::value::ValueLayout;

/// Where a record space's mark block list lies in the dispatch payload: the byte offsets of its
/// block table pointer and of its block count.
#[derive(Clone, Copy)]
pub(crate) struct MarkList {
    pub(crate) blocks: usize,
    pub(crate) count: usize,
}

/// The 64-bit constant `bits`.
fn word<M: MachineScalar>(machine: &mut M, bits: u64) -> M::Word {
    machine.raw_const(Lane::I64, bits as i64)
}

/// `row` of `a` and the constant `bits`.
fn with<M: MachineScalar>(machine: &mut M, row: Binary, a: M::Word, bits: u64) -> M::Word {
    let b = word(machine, bits);
    machine.raw_binary(row, a, b)
}

/// Continues where a store of `value` into object record `record_id` needs no barrier log, and
/// hands `guard` each condition that must hold for it: the owner's mark block exists, and its mark
/// bit is clear. `flags` are the accesses of the payload's words and of the mark words;
/// `payload` is the dispatch payload `list` offsets into.
pub(crate) fn unlogged_store<M: MachineAccess + MachineFlow>(machine: &mut M, flags: (M::Flags, M::Flags),
    payload: M::Word, list: MarkList, (record_id, value): (M::Word, M::Word), mut guard: impl FnMut(&mut M, M::Word)) {
    let (payload_flags, mark_flags) = flags;
    let tag = with(machine, Binary::And, value, ValueLayout::TAG_MASK);
    let immediate = machine.raw_compare_imm(Relation::NotEqual, tag, ValueLayout::HEAP_TAG as i64);
    let (heap, write) = (machine.new_block(), machine.new_block());
    machine.branch_if(immediate, (write, &[]), (heap, &[]));
    machine.enter(heap);
    let blocks = machine.raw_load(Lane::I64, payload_flags, payload, list.blocks as i32);
    let count = machine.raw_load(Lane::I64, payload_flags, payload, list.count as i32);
    let block = with(machine, Binary::ShiftRightUnsigned, record_id, u64::from(OBJECT.mark_records_per_block.trailing_zeros()));
    let live = machine.raw_compare(Relation::UnsignedLess, block, count);
    guard(machine, live);
    let block_offset = with(machine, Binary::Mul, block, OBJECT.mark_block_size as u64);
    let block_address = machine.raw_binary(Binary::Add, blocks, block_offset);
    let within = with(machine, Binary::And, record_id, (OBJECT.mark_records_per_block - 1) as u64);
    let word_index = with(machine, Binary::ShiftRightUnsigned, within, u64::from(OBJECT.mark_word_bits.trailing_zeros()));
    let word_offset = with(machine, Binary::Mul, word_index, size_of::<u64>() as u64);
    let word_address = machine.raw_binary(Binary::Add, block_address, word_offset);
    let marks = machine.raw_load(Lane::I64, mark_flags, word_address, OBJECT.mark_offset as i32);
    let bit_index = with(machine, Binary::And, within, (OBJECT.mark_word_bits - 1) as u64);
    let one = word(machine, 1);
    let mask = machine.raw_binary(Binary::ShiftLeft, one, bit_index);
    let marked = machine.raw_binary(Binary::And, marks, mask);
    let unmarked = machine.raw_compare_imm(Relation::Equal, marked, 0);
    guard(machine, unmarked);
    machine.goto(write, &[]);
    machine.enter(write);
}
