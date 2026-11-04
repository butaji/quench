//! Lower conditional integer updates through a mask derived from one tested bit.

use crate::{cursor::{Cursor, FuncCursor}, ir::{self, InstBuilder, InstructionData, Opcode, ValueDef}};
use cranelift_entity::EntityRef;
use alloc::vec;
use alloc::vec::Vec;

fn binary(f: &ir::Function, value: ir::Value, opcode: Opcode) -> Option<[ir::Value; 2]> {
    let ValueDef::Result(inst, _) = f.dfg.value_def(f.dfg.resolve_aliases(value)) else { return None };
    let InstructionData::Binary { opcode: actual, args } = f.dfg.insts[inst] else { return None };
    (actual == opcode).then(|| args.map(|v| f.dfg.resolve_aliases(v)))
}

fn constant(f: &ir::Function, value: ir::Value) -> Option<u64> {
    let ValueDef::Result(inst, _) = f.dfg.value_def(f.dfg.resolve_aliases(value)) else { return None };
    let InstructionData::UnaryImm { opcode: Opcode::Iconst, imm } = f.dfg.insts[inst] else { return None };
    Some(imm.bits() as u64)
}

fn tested_bit(f: &ir::Function, mut condition: ir::Value, ty: ir::Type) -> Option<(ir::Value, u32, bool)> {
    let mut inverted = false;
    if let ValueDef::Result(inst, _) = f.dfg.value_def(condition)
        && let InstructionData::IntCompare { cond, args, .. } = f.dfg.insts[inst] {
        use ir::condcodes::IntCC;
        if !matches!(cond, IntCC::Equal | IntCC::NotEqual) { return None; }
        condition = if constant(f, args[1]) == Some(0) { args[0] }
            else if constant(f, args[0]) == Some(0) { args[1] } else { return None };
        inverted = cond == IntCC::Equal;
    }
    if f.dfg.value_type(condition) != ty { return None; }
    let args = binary(f, condition, Opcode::Band)?;
    for (source, mask) in [(args[0], args[1]), (args[1], args[0])] {
        let Some(bits) = constant(f, mask) else { continue };
        let bits = bits & (u64::MAX >> (u64::BITS - ty.bits()));
        if bits.is_power_of_two() { return Some((source, bits.trailing_zeros(), inverted)); }
    }
    None
}

pub(crate) fn run(f: &mut ir::Function) {
    let selects: Vec<_> = f.layout.blocks().flat_map(|block| f.layout.block_insts(block))
        .filter(|&inst| f.dfg.insts[inst].opcode() == Opcode::Select).collect();
    if selects.is_empty() { return; }
    let mut uses = vec![0_usize; f.dfg.num_values()];
    for block in f.layout.blocks() { for inst in f.layout.block_insts(block) {
        for value in f.dfg.inst_values(inst) { uses[f.dfg.resolve_aliases(value).index()] += 1; }
    }}
    for inst in selects {
        let InstructionData::Ternary { args, .. } = f.dfg.insts[inst] else { continue };
        let [condition, yes, no] = args.map(|v| f.dfg.resolve_aliases(v));
        let ty = f.dfg.value_type(yes);
        if !ty.is_int() || ty.bits() > u64::BITS || uses[condition.index()] != 1 { continue; }
        let Some((source, bit, condition_inverted)) = tested_bit(f, condition, ty) else { continue };
        for (changed, base, invert) in [(yes, no, false), (no, yes, true)] {
            let zero_base = constant(f, base) == Some(0);
            let other = if zero_base { changed } else {
                if uses[changed.index()] != 1 { continue; }
                let Some(args) = binary(f, changed, Opcode::Bxor) else { continue };
                if args[0] == base { args[1] } else if args[1] == base { args[0] } else { continue }
            };
            let mut cursor = FuncCursor::new(f);
            cursor.goto_inst(inst);
            let sign = i64::from(ty.bits() - 1);
            let high = cursor.ins().ishl_imm_u(source, sign - i64::from(bit));
            let mask = cursor.ins().sshr_imm_u(high, sign);
            let mask = if invert ^ condition_inverted { cursor.ins().bnot(mask) } else { mask };
            cursor.func.dfg.insts[inst] = if zero_base {
                InstructionData::Binary { opcode: Opcode::Band, args: [mask, other] }
            } else {
                let selected = cursor.ins().band(mask, other);
                InstructionData::Binary { opcode: Opcode::Bxor, args: [base, selected] }
            };
            break;
        }
    }
}
