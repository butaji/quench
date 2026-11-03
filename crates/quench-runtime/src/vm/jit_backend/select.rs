//! Predicate small diamonds of total integer operations after semantic lowering.
//! Memory, calls, traps and conversions with observable failure remain control flow.

use cranelift_codegen::{flowgraph::ControlFlowGraph, ir::{self, InstBuilder}, cursor::{Cursor, FuncCursor}};
use smallvec::SmallVec;

// A conditional branch and its join jump pay for up to two speculated integer
// operations on the unselected arm. Cranelift folds constants and narrowing.
const BRANCH_WORK: usize = 2;
// Inline capacities cover small diamonds; larger paths and joins grow normally.
const INLINE_ARM_BLOCKS: usize = 4;
const INLINE_ARM_INSTRUCTIONS: usize = 8;

struct Arm {
    blocks: SmallVec<[ir::Block; INLINE_ARM_BLOCKS]>,
    instructions: SmallVec<[ir::Inst; INLINE_ARM_INSTRUCTIONS]>,
    exit: ir::BlockCall,
}

fn constant(function: &ir::Function, value: ir::Value) -> bool {
    let ir::ValueDef::Result(inst, _) = function.dfg.value_def(function.dfg.resolve_aliases(value)) else { return false };
    function.dfg.insts[inst].opcode() == ir::Opcode::Iconst
}

fn arm(function: &ir::Function, cfg: &ControlFlowGraph, mut block: ir::Block) -> Option<Arm> {
    let mut blocks = SmallVec::new();
    let mut instructions = SmallVec::new();
    let mut work = 0;
    loop {
        if cfg.pred_iter(block).take(2).count() != 1 || !function.dfg.block_params(block).is_empty() { return None; }
        let last = function.layout.last_inst(block)?;
        if function.dfg.insts[last].opcode() != ir::Opcode::Jump { return None; }
        blocks.push(block);
        for inst in function.layout.block_insts(block) {
            if let ir::InstructionData::Jump { destination, .. } = function.dfg.insts[inst] {
                let next = destination.block(&function.dfg.value_lists);
                if cfg.pred_iter(next).take(2).count() != 1 || destination.args(&function.dfg.value_lists).len() != 0 {
                    return Some(Arm { blocks, instructions, exit: destination });
                }
                block = next;
                break;
            }
            let opcode = function.dfg.insts[inst].opcode();
            if !matches!(opcode, ir::Opcode::Iconst | ir::Opcode::Ireduce | ir::Opcode::Iadd | ir::Opcode::Isub
                | ir::Opcode::Band | ir::Opcode::Bor | ir::Opcode::Bxor | ir::Opcode::Ishl | ir::Opcode::Ushr | ir::Opcode::Sshr) {
                return None;
            }
            if !function.dfg.inst_results(inst).iter().all(|value| {
                let ty = function.dfg.value_type(*value);
                ty.is_int() && ty.bits() <= ir::types::I64.bits()
            }) { return None; }
            work += usize::from(!matches!(opcode, ir::Opcode::Iconst | ir::Opcode::Ireduce)
                && !function.dfg.inst_args(inst).iter().all(|value| constant(function, *value)));
            if work > BRANCH_WORK { return None; }
            instructions.push(inst);
        }
    }
}

pub(super) fn convert(function: &mut ir::Function) -> usize {
    // Replacing two arm edges by one head edge only reduces join indegrees.
    // The original CFG therefore remains conservative for single-entry tests.
    let cfg = ControlFlowGraph::with_function(function);
    let heads: Vec<_> = function.layout.blocks().collect();
    let mut converted = 0;
    for head in heads {
        if !function.layout.is_block_inserted(head) { continue; }
        let Some(branch) = function.layout.last_inst(head) else { continue };
        let ir::InstructionData::Brif { arg, blocks, .. } = function.dfg.insts[branch] else { continue };
        if blocks.iter().any(|edge| edge.args(&function.dfg.value_lists).len() != 0) { continue; }
        let Some(left) = arm(function, &cfg, blocks[0].block(&function.dfg.value_lists)) else { continue };
        let Some(right) = arm(function, &cfg, blocks[1].block(&function.dfg.value_lists)) else { continue };
        let join = left.exit.block(&function.dfg.value_lists);
        if join != right.exit.block(&function.dfg.value_lists) || join == head { continue; }
        let pairs: Vec<_> = left.exit.args(&function.dfg.value_lists).zip(right.exit.args(&function.dfg.value_lists)).collect();
        if pairs.iter().any(|(a,b)| !matches!((a,b), (ir::BlockArg::Value(_), ir::BlockArg::Value(_)))) { continue; }
        let selects = pairs.iter().filter(|(a,b)| match (a,b) {
            (ir::BlockArg::Value(a), ir::BlockArg::Value(b)) => function.dfg.resolve_aliases(*a) != function.dfg.resolve_aliases(*b),
            _ => false,
        }).count();
        if selects > BRANCH_WORK { continue; }
        for inst in left.instructions.iter().chain(&right.instructions) {
            function.layout.remove_inst(*inst);
            function.layout.insert_inst(*inst, branch);
        }
        let mut cursor = FuncCursor::new(function);
        cursor.goto_inst(branch);
        let values: Vec<_> = pairs.into_iter().map(|(a,b)| {
            let (ir::BlockArg::Value(a), ir::BlockArg::Value(b)) = (a,b) else { unreachable!() };
            let a = cursor.func.dfg.resolve_aliases(a);
            let b = cursor.func.dfg.resolve_aliases(b);
            ir::BlockArg::Value(if a == b { a } else { cursor.ins().select(arg, a, b) })
        }).collect();
        let destination = cursor.func.dfg.block_call(join, &values);
        cursor.func.dfg.insts[branch] = ir::InstructionData::Jump { opcode: ir::Opcode::Jump, destination };
        for block in left.blocks.iter().chain(&right.blocks) { cursor.func.layout.remove_block_and_insts(*block); }
        converted += 1;
    }
    converted
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    enum Operation { Bitwise, Division, Load }

    #[test]
    fn diamonds_require_total_integer_operations_and_preserve_verified_ssa() {
        #[cfg(feature = "jit")]
        let publication = super::super::CodePublication::ExecutableMemory;
        #[cfg(not(feature = "jit"))]
        let publication = super::super::CodePublication::RelocatableObject;
        let isa = super::super::target_isa_for("speed", publication).unwrap();
        for operation in [Operation::Bitwise, Operation::Division, Operation::Load] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.push(ir::AbiParam::new(ir::types::I32));
            signature.returns.push(ir::AbiParam::new(ir::types::I32));
            let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let left = builder.create_block();
            let right = builder.create_block();
            let join = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(join, ir::types::I32);
            builder.switch_to_block(entry);
            let input = builder.block_params(entry)[0];
            builder.ins().brif(input, left, &[], right, &[]);
            builder.switch_to_block(left);
            let left_value = builder.ins().sshr_imm_u(input, 1);
            builder.ins().jump(join, &[left_value.into()]);
            builder.switch_to_block(right);
            let right_value = match operation {
                Operation::Bitwise => builder.ins().bxor_imm_u(input, 1),
                Operation::Division => {
                    let zero = builder.ins().iconst(ir::types::I32, 0);
                    builder.ins().sdiv(input, zero)
                }
                Operation::Load => {
                    let address = builder.ins().iconst(ir::types::I64, 0);
                    builder.ins().load(ir::types::I32, ir::MemFlagsData::new(), address, 0)
                }
            };
            builder.ins().jump(join, &[right_value.into()]);
            builder.switch_to_block(join);
            let result = builder.block_params(join)[0];
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            assert_eq!(convert(&mut function), usize::from(matches!(operation, Operation::Bitwise)));
            cranelift_codegen::verify_function(&function, &*isa).unwrap();
        }
    }
}
