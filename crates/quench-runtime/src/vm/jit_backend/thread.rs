//! Branch edges that skip blocks which only jump onward.
//!
//! Lane lowering ends a guard or a join in a block holding one `jump` that forwards its
//! parameters; richards' largest body has 605 of them among 2479 blocks. Cranelift's
//! machine-code buffer threads such jumps for free, but every block costs the control-flow,
//! dominator and register-allocation passes (about 13 us per block), so the edge goes
//! straight to the target here and the forwarding block, now unreachable, is dropped.

use cranelift_codegen::ir::{self, BlockArg};
use smallvec::SmallVec;

/// How a forwarding block names one argument of its jump.
#[derive(Clone, Copy)]
enum Forwarded {
    /// The block's own parameter at this index.
    Parameter(usize),
    /// A value that dominates the block, and so every edge into it.
    Value(ir::Value),
}

type Forward = (ir::Block, SmallVec<[Forwarded; 4]>);

/// How many instructions operands and branch arguments name each value.
fn uses(function: &ir::Function) -> Vec<u32> {
    let mut counts = vec![0u32; function.dfg.num_values()];
    for block in function.layout.blocks() {
        for inst in function.layout.block_insts(block) {
            for value in function.dfg.inst_values(inst) {
                counts[function.dfg.resolve_aliases(value).as_u32() as usize] += 1;
            }
        }
    }
    counts
}

/// The blocks that hold only a `jump`, with where it goes. A block whose parameter is named
/// anywhere but its own jump keeps the parameter's dominance over what follows, so it stays.
fn forwards(function: &ir::Function) -> Vec<Option<Forward>> {
    let counts = uses(function);
    let blocks = function.dfg.num_blocks();
    let mut table: Vec<Option<Forward>> = vec![None; blocks];
    let entry = function.layout.entry_block();
    for block in function.layout.blocks() {
        if Some(block) == entry { continue; }
        let Some(first) = function.layout.first_inst(block) else { continue };
        if function.layout.last_inst(block) != Some(first) { continue; }
        let ir::InstructionData::Jump { destination, .. } = function.dfg.insts[first] else { continue };
        let target = destination.block(&function.dfg.value_lists);
        if target == block { continue; }
        let parameters = function.dfg.block_params(block);
        let arguments: Option<SmallVec<[Forwarded; 4]>> = destination.args(&function.dfg.value_lists).map(|argument| {
            let BlockArg::Value(value) = argument else { return None };
            let value = function.dfg.resolve_aliases(value);
            Some(match parameters.iter().position(|parameter| function.dfg.resolve_aliases(*parameter) == value) {
                Some(index) => Forwarded::Parameter(index),
                None => Forwarded::Value(value),
            })
        }).collect();
        let Some(arguments) = arguments else { continue };
        let named_in_jump = |parameter: usize| arguments.iter()
            .filter(|argument| matches!(argument, Forwarded::Parameter(index) if *index == parameter)).count() as u32;
        let confined = parameters.iter().enumerate().all(|(index, parameter)|
            counts[function.dfg.resolve_aliases(*parameter).as_u32() as usize] == named_in_jump(index));
        if confined { table[block.as_u32() as usize] = Some((target, arguments)); }
    }
    table
}

/// Each hop of a chain of forwarding blocks costs one step; a cycle of them stops here.
const MAX_HOPS: usize = 8;

/// Retargets every `jump` and `brif` edge that enters a forwarding block; the number of edges
/// moved.
pub(super) fn thread_jumps(function: &mut ir::Function) -> usize {
    let table = forwards(function);
    if table.iter().all(Option::is_none) { return 0; }
    let insts: Vec<ir::Inst> = function.layout.blocks()
        .filter_map(|block| function.layout.last_inst(block))
        .filter(|inst| matches!(function.dfg.insts[*inst], ir::InstructionData::Jump { .. } | ir::InstructionData::Brif { .. }))
        .collect();
    let dfg = &mut function.dfg;
    let mut moved = 0;
    for inst in insts {
        let edges = dfg.insts[inst].branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables);
        for edge in edges.iter_mut() {
            let mut block = edge.block(&dfg.value_lists);
            let mut arguments: SmallVec<[BlockArg; 4]> = edge.args(&dfg.value_lists).collect();
            let mut hops = 0;
            while hops < MAX_HOPS && let Some((target, forwarded)) = &table[block.as_u32() as usize] {
                arguments = forwarded.iter().map(|argument| match *argument {
                    Forwarded::Parameter(index) => arguments[index],
                    Forwarded::Value(value) => BlockArg::Value(value),
                }).collect();
                block = *target;
                hops += 1;
            }
            if hops > 0 {
                *edge = ir::BlockCall::new(block, arguments, &mut dfg.value_lists);
                moved += 1;
            }
        }
    }
    moved
}

#[cfg(all(test, feature = "jit"))]
mod tests {
    use super::*;
    use cranelift_codegen::ir::{types, AbiParam, InstBuilder, UserFuncName};
    use cranelift_codegen::isa::CallConv;
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    #[test]
    fn edges_skip_a_block_that_only_jumps_and_keep_its_arguments() {
        let mut signature = ir::Signature::new(CallConv::SystemV);
        signature.params.push(AbiParam::new(types::I64));
        signature.returns.push(AbiParam::new(types::I64));
        let mut function = ir::Function::with_name_signature(UserFuncName::default(), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let (entry, hop, join) = (builder.create_block(), builder.create_block(), builder.create_block());
        builder.append_block_params_for_function_params(entry);
        builder.append_block_param(hop, types::I64);
        builder.append_block_param(join, types::I64);
        builder.switch_to_block(entry);
        let argument = builder.block_params(entry)[0];
        let flag = builder.ins().icmp_imm_s(ir::condcodes::IntCC::Equal, argument, 0);
        let one = builder.ins().iconst(types::I64, 1);
        builder.ins().brif(flag, hop, &[one.into()], join, &[argument.into()]);
        builder.switch_to_block(hop);
        let carried = builder.block_params(hop)[0];
        builder.ins().jump(join, &[carried.into()]);
        builder.switch_to_block(join);
        let result = builder.block_params(join)[0];
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        let isa = super::super::target_isa().unwrap();
        builder.finalize(isa.frontend_config());
        assert_eq!(thread_jumps(&mut function), 1);
        let ir::InstructionData::Brif { blocks, .. } = function.dfg.insts[function.layout.last_inst(entry).unwrap()] else { panic!() };
        let pool = &function.dfg.value_lists;
        assert_eq!(blocks[0].block(pool), join);
        assert_eq!(blocks[0].args(pool).collect::<Vec<_>>(), vec![BlockArg::Value(one)]);
        assert_eq!(blocks[1].block(pool), join);
    }
}
