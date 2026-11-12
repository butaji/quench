//! Words an ahead-of-time image builds from a relocated symbol at run time, where a live
//! compile folds the symbol's value into the code.
use cranelift_codegen::ir::{self, InstBuilder, condcodes::IntCC, types};
use cranelift_frontend::FunctionBuilder;

/// `(atom << 16) | site`: the word the field helpers take, from the atom symbol's value.
pub(in crate::vm) fn atom_site_word(builder: &mut FunctionBuilder<'_>, atom: ir::Value, site: u16) -> ir::Value {
    let shifted = builder.ins().ishl_imm_u(atom, i64::from(u16::BITS));
    builder.ins().bor_imm_u(shifted, i64::from(site))
}

/// Whether no method cache names `atom` (a run-time value): the bit of its word in `words`, a
/// bitset of `words_len` words; an atom past them is unnamed.
pub(in crate::vm) fn atom_unnamed(builder: &mut FunctionBuilder<'_>, (words, words_len): (ir::Value, ir::Value), atom: ir::Value) -> ir::Value {
    let word_bits = u64::BITS.trailing_zeros() as i64;
    let index = builder.ins().ushr_imm_u(atom, word_bits);
    let covered = builder.ins().icmp(IntCC::UnsignedGreaterThan, words_len, index);
    let (present, join) = (builder.create_block(), builder.create_block());
    let free = builder.append_block_param(join, types::I8);
    let one = builder.ins().iconst(types::I8, 1);
    builder.ins().brif(covered, present, &[], join, &[ir::BlockArg::Value(one)]);
    builder.switch_to_block(present);
    let offset = builder.ins().ishl_imm_u(index, size_of::<u64>().trailing_zeros() as i64);
    let address = builder.ins().iadd(words, offset);
    let word = builder.ins().load(types::I64, ir::MemFlagsData::trusted(), address, 0);
    let position = builder.ins().band_imm_u(atom, i64::from(u64::BITS - 1));
    let one_bit = builder.ins().iconst(types::I64, 1);
    let bit = builder.ins().ishl(one_bit, position);
    let named = builder.ins().band(word, bit);
    let unnamed = builder.ins().icmp_imm_u(IntCC::Equal, named, 0);
    builder.ins().jump(join, &[ir::BlockArg::Value(unnamed)]);
    builder.switch_to_block(join);
    free
}
