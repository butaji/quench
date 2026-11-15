//! Words an ahead-of-time image builds from a relocated symbol at run time, where a live
//! compile folds the symbol's value into the code.
use crate::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation};
use cranelift_codegen::ir::{self, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// `(atom << 16) | site`: the word the field helpers take, from the atom symbol's value.
pub(in crate::vm) fn atom_site_word(builder: &mut FunctionBuilder<'_>, atom: ir::Value, site: u16) -> ir::Value {
    let shifted = builder.raw_binary_imm(Binary::ShiftLeft, atom, i64::from(u16::BITS));
    builder.raw_binary_imm(Binary::Or, shifted, i64::from(site))
}

/// Whether no method cache names `atom` (a run-time value): the bit of its word in `words`, a
/// bitset of `words_len` words; an atom past them is unnamed.
pub(in crate::vm) fn atom_unnamed(builder: &mut FunctionBuilder<'_>, (words, words_len): (ir::Value, ir::Value), atom: ir::Value) -> ir::Value {
    let word_bits = u64::BITS.trailing_zeros() as i64;
    let index = builder.raw_binary_imm(Binary::ShiftRightUnsigned, atom, word_bits);
    let covered = builder.raw_compare(Relation::UnsignedGreater, words_len, index);
    let (present, join) = (builder.new_block(), builder.new_block());
    let free = builder.block_parameter(join, Lane::I8);
    let one = builder.raw_const(Lane::I8, 1);
    builder.branch_if(covered, (present, &[]), (join, &[one]));
    builder.enter(present);
    let offset = builder.raw_binary_imm(Binary::ShiftLeft, index, size_of::<u64>().trailing_zeros() as i64);
    let address = builder.raw_binary(Binary::Add, words, offset);
    let word = builder.raw_load(Lane::I64, MemFlagsData::trusted(), address, 0);
    let position = builder.raw_binary_imm(Binary::And, atom, i64::from(u64::BITS - 1));
    let one_bit = builder.raw_const(Lane::I64, 1);
    let bit = builder.raw_binary(Binary::ShiftLeft, one_bit, position);
    let named = builder.raw_binary(Binary::And, word, bit);
    let unnamed = builder.raw_compare_imm(Relation::Equal, named, 0);
    builder.goto(join, &[unnamed]);
    builder.enter(join);
    free
}
