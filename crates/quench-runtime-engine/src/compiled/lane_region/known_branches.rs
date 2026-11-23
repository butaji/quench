//! Branches a lowering left on conditions it already knows, decided once SSA has resolved every
//! variable to its definition: each takes its one edge, and the compile drops the blocks that
//! lose their last edge before any analysis visits them.

#[allow(unused_imports)]
use crate::ext_traits::*;
use cranelift_codegen::ir::{self, InstBuilder, InstructionData, Opcode};

/// The integer constant `value` names, at its type's width: (zero-extended, sign-extended).
fn known(function: &ir::Function, value: ir::Value) -> Option<(u64, i64)> {
    let value = function.dfg.resolve_aliases(value);
    let InstructionData::UnaryImm { opcode: Opcode::Iconst, imm } = function.dfg.insts[function.dfg.value_def(value).inst()?] else { return None };
    let bits = function.dfg.value_type(value).bits();
    let raw = imm.bits() as u64;
    let unsigned = if bits >= 64 { raw } else { raw & ((1u64 << bits) - 1) };
    let signed = if bits >= 64 { raw as i64 } else { ((unsigned << (64 - bits)) as i64) >> (64 - bits) };
    Some((unsigned, signed))
}

/// `value` cut to `ty`'s width, sign-extended as the frontend writes an immediate.
fn fit(ty: ir::Type, value: u64) -> i64 {
    let bits = ty.bits();
    if bits >= 64 { return value as i64; }
    let unsigned = value & ((1u64 << bits) - 1);
    ((unsigned << (64 - bits)) as i64) >> (64 - bits)
}

/// Decides the comparisons of known constants and takes the one edge of each branch on a
/// decided condition, once SSA has resolved every variable to its definition (while lowering,
/// a variable read in an unsealed block is still a parameter). The edges not taken leave their
/// blocks unreachable, which the compile removes before any analysis visits them. The count of
/// branches it decided.
pub(crate) fn decide(function: &mut ir::Function) -> usize {
    use ir::condcodes::IntCC;
    let mut decided = 0;
    let blocks: Vec<ir::Block> = function.layout.blocks().collect();
    for block in blocks {
        let insts: Vec<ir::Inst> = function.layout.block_insts(block).collect();
        for inst in insts {
            match function.dfg.insts[inst] {
                InstructionData::IntCompare { opcode: Opcode::Icmp, cond, args } => {
                    let (Some((au, as_)), Some((bu, bs))) = (known(function, args[0]), known(function, args[1])) else { continue };
                    let holds = match cond {
                        IntCC::Equal => au == bu, IntCC::NotEqual => au != bu,
                        IntCC::SignedLessThan => as_ < bs, IntCC::SignedLessThanOrEqual => as_ <= bs,
                        IntCC::SignedGreaterThan => as_ > bs, IntCC::SignedGreaterThanOrEqual => as_ >= bs,
                        IntCC::UnsignedLessThan => au < bu, IntCC::UnsignedLessThanOrEqual => au <= bu,
                        IntCC::UnsignedGreaterThan => au > bu, IntCC::UnsignedGreaterThanOrEqual => au >= bu,
                    };
                    let result_ty = function.dfg.value_type(function.dfg.first_result(inst));
                    function.replace(inst).iconst(result_ty, i64::from(holds));
                }
                InstructionData::Binary { opcode, args } => {
                    let (Some((a, _)), Some((b, _))) = (known(function, args[0]), known(function, args[1])) else { continue };
                    let ty = function.dfg.value_type(function.dfg.first_result(inst));
                    if !ty.is_int() || ty.bits() > 64 { continue; }
                    let width = ty.bits();
                    let shift = |b: u64| (b % u64::from(width)) as u32;
                    let value = match opcode {
                        Opcode::Iadd => a.wrapping_add(b), Opcode::Isub => a.wrapping_sub(b), Opcode::Imul => a.wrapping_mul(b),
                        Opcode::Band => a & b, Opcode::Bor => a | b, Opcode::Bxor => a ^ b,
                        Opcode::Ishl => a.wrapping_shl(shift(b)), Opcode::Ushr => a.wrapping_shr(shift(b)),
                        _ => continue,
                    };
                    function.replace(inst).iconst(ty, fit(ty, value));
                }
                InstructionData::Unary { opcode: opcode @ (Opcode::Ireduce | Opcode::Uextend | Opcode::Sextend), arg } => {
                    let Some((unsigned, signed)) = known(function, arg) else { continue };
                    let ty = function.dfg.value_type(function.dfg.first_result(inst));
                    let value = if opcode == Opcode::Sextend { signed as u64 } else { unsigned };
                    function.replace(inst).iconst(ty, fit(ty, value));
                }
                InstructionData::Brif { arg, blocks, .. } => {
                    let Some((flag, _)) = known(function, arg) else { continue };
                    let taken = blocks[usize::from(flag == 0)];
                    let target = taken.block(&function.dfg.value_lists);
                    let arguments: Vec<ir::BlockArg> = taken.args(&function.dfg.value_lists).collect();
                    function.replace(inst).jump(target, &arguments);
                    decided += 1;
                }
                _ => {}
            }
        }
    }
    decided
}
