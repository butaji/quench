//! Predicate small diamonds of total integer operations and bit representations.
//! Memory, calls, traps and conversions with observable failure remain control flow.

use cranelift_codegen::{bitset::CompoundBitSet, entity::EntityRef, flowgraph::ControlFlowGraph, ir::{self, InstBuilder}, cursor::{Cursor, FuncCursor}};
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

/// Resolve aliases before recording uses, including successor arguments. Rewrites
/// only reuse existing live operands; newly created results are charged below.
struct Uses {
    live: CompoundBitSet,
    original_values: usize,
}

fn used_values(function: &ir::Function) -> Uses {
    let original_values = function.dfg.num_values();
    let mut live: CompoundBitSet = CompoundBitSet::with_capacity(original_values);
    for block in function.layout.blocks() {
        for inst in function.layout.block_insts(block) {
            for value in function.dfg.inst_values(inst) {
                live.insert(function.dfg.resolve_aliases(value).index());
            }
        }
    }
    if let Some(labels) = &function.dfg.values_labels {
        for value in labels.keys() { live.insert(function.dfg.resolve_aliases(*value).index()); }
    }
    Uses { live, original_values }
}

fn arm(function: &ir::Function, cfg: &ControlFlowGraph, used: &Uses, mut block: ir::Block) -> Option<Arm> {
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
            let representation = matches!(opcode, ir::Opcode::Ireduce | ir::Opcode::Uextend | ir::Opcode::Sextend | ir::Opcode::Bitcast);
            if !representation && !matches!(opcode, ir::Opcode::Iconst | ir::Opcode::Iadd | ir::Opcode::Isub
                | ir::Opcode::Band | ir::Opcode::Bor | ir::Opcode::Bxor | ir::Opcode::Ishl | ir::Opcode::Ushr | ir::Opcode::Sshr
                | ir::Opcode::Icmp | ir::Opcode::Select) {
                return None;
            }
            if !function.dfg.inst_results(inst).iter().all(|value| {
                let ty = function.dfg.value_type(*value);
                (ty.is_int() || (opcode == ir::Opcode::Bitcast && ty.is_float())) && ty.bits() <= ir::types::I64.bits()
            }) { return None; }
            // Icmp's I8 result alone does not bound its operand width. Keep
            // wide/vector comparisons and select conditions out of this cost model.
            if matches!(opcode, ir::Opcode::Icmp | ir::Opcode::Select)
                && !function.dfg.inst_args(inst).iter().all(|value| {
                    let ty = function.dfg.value_type(*value);
                    ty.is_int() && ty.bits() <= ir::types::I64.bits()
                }) { return None; }
            // Pure unused boxing can precede a predicate before egraph cleanup.
            // Keep every instruction and all effect exclusions; only discount
            // genuinely unused total work. New SSA results are conservatively live.
            let live = function.dfg.inst_results(inst).iter().any(|value| {
                let index = function.dfg.resolve_aliases(*value).index();
                index >= used.original_values || used.live.contains(index)
            });
            work += usize::from(live && opcode != ir::Opcode::Iconst && !representation
                && !function.dfg.inst_args(inst).iter().all(|value| constant(function, *value)));
            if work > BRANCH_WORK { return None; }
            instructions.push(inst);
        }
    }
}

pub(super) fn convert(function: &mut ir::Function) -> usize {
    // Replacing two arm edges by one head edge only reduces join indegrees.
    // The original CFG therefore remains conservative for single-entry tests.
    let used = used_values(function);
    let cfg = ControlFlowGraph::with_function(function);
    let heads: Vec<_> = function.layout.blocks().collect();
    let mut converted = 0;
    for head in heads {
        if !function.layout.is_block_inserted(head) { continue; }
        let Some(branch) = function.layout.last_inst(head) else { continue };
        let ir::InstructionData::Brif { arg, blocks, .. } = function.dfg.insts[branch] else { continue };
        if blocks.iter().any(|edge| edge.args(&function.dfg.value_lists).len() != 0) { continue; }
        let Some(left) = arm(function, &cfg, &used, blocks[0].block(&function.dfg.value_lists)) else { continue };
        let Some(right) = arm(function, &cfg, &used, blocks[1].block(&function.dfg.value_lists)) else { continue };
        let join = left.exit.block(&function.dfg.value_lists);
        if join != right.exit.block(&function.dfg.value_lists) || join == head { continue; }
        // A branch whose two edges reach one block gives that block one predecessor
        // branch, so both arms name the same blocks; removing them twice would unlink the
        // whole layout. The CFG counts no edge into the entry block either.
        let entry = function.layout.entry_block();
        if left.blocks.iter().any(|block| right.blocks.contains(block))
            || left.blocks.iter().chain(&right.blocks).any(|block| Some(*block) == entry)
        { continue; }
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

    enum Operation {
        Bitwise, RepresentedBitwise, CompareSelect, DeadCompareSelect, LiveCompareSelect,
        AliasLiveCompareSelect, NumericConversion, Division, Load, DeadLoad, DeadAtomic, DeadConversion, DeadCall,
    }

    #[test]
    fn diamonds_require_total_integer_operations_and_preserve_verified_ssa() {
        #[cfg(feature = "jit")]
        let publication = super::super::CodePublication::ExecutableMemory;
        #[cfg(not(feature = "jit"))]
        let publication = super::super::CodePublication::RelocatableObject;
        let isa = super::super::target_isa_for("speed", publication).unwrap();
        for operation in [Operation::Bitwise, Operation::RepresentedBitwise, Operation::CompareSelect,
            Operation::DeadCompareSelect, Operation::LiveCompareSelect, Operation::AliasLiveCompareSelect,
            Operation::NumericConversion, Operation::Division, Operation::Load, Operation::DeadLoad,
            Operation::DeadAtomic, Operation::DeadConversion, Operation::DeadCall] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.push(ir::AbiParam::new(ir::types::I32));
            signature.returns.push(ir::AbiParam::new(ir::types::I32));
            let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
            let mut alias = None;
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
                Operation::RepresentedBitwise => {
                    let bits = builder.ins().bxor_imm_u(input, 1);
                    let float = builder.ins().bitcast(ir::types::F32, ir::MemFlagsData::new(), bits);
                    let bits = builder.ins().bitcast(ir::types::I32, ir::MemFlagsData::new(), float);
                    let wide = builder.ins().sextend(ir::types::I64, bits);
                    builder.ins().ireduce(ir::types::I32, wide)
                }
                Operation::CompareSelect | Operation::DeadCompareSelect
                    | Operation::LiveCompareSelect | Operation::AliasLiveCompareSelect => {
                    if matches!(operation, Operation::DeadCompareSelect) {
                        builder.ins().bor_imm_u(input, 42);
                    }
                    let value = if matches!(operation, Operation::LiveCompareSelect | Operation::AliasLiveCompareSelect) {
                        builder.ins().iadd_imm_s(input, 1)
                    } else { input };
                    let holds = builder.ins().icmp_imm_s(ir::condcodes::IntCC::SignedLessThan, value, 0);
                    let yes = builder.ins().iconst(ir::types::I32, 17);
                    let no = builder.ins().iconst(ir::types::I32, -5);
                    let selected = builder.ins().select(holds, yes, no);
                    if matches!(operation, Operation::AliasLiveCompareSelect) {
                        let copy = builder.ins().iconst(ir::types::I32, 0);
                        let ir::ValueDef::Result(source, _) = builder.func.dfg.value_def(selected) else { unreachable!() };
                        let ir::ValueDef::Result(destination, _) = builder.func.dfg.value_def(copy) else { unreachable!() };
                        alias = Some((destination, source));
                        copy
                    } else { selected }
                }
                Operation::DeadLoad | Operation::DeadAtomic | Operation::DeadConversion | Operation::DeadCall => {
                    match operation {
                        Operation::DeadLoad => {
                            let address = builder.ins().iconst(ir::types::I64, 0);
                            builder.ins().load(ir::types::I32, ir::MemFlagsData::new(), address, 0);
                        }
                        Operation::DeadAtomic => {
                            let address = builder.ins().iconst(ir::types::I64, 0);
                            builder.ins().atomic_load(ir::types::I32, ir::MemFlagsData::trusted(), address);
                        }
                        Operation::DeadConversion => {
                            let float = builder.ins().bitcast(ir::types::F32, ir::MemFlagsData::new(), input);
                            builder.ins().fcvt_to_sint(ir::types::I32, float);
                        }
                        Operation::DeadCall => {
                            let mut signature = ir::Signature::new(isa.default_call_conv());
                            signature.returns.push(ir::AbiParam::new(ir::types::I32));
                            let signature = builder.import_signature(signature);
                            let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(0, 1));
                            let target = builder.import_function(ir::ExtFuncData {
                                name: ir::ExternalName::user(name), signature, colocated: false, patchable: false,
                            });
                            builder.ins().call(target, &[]);
                        }
                        _ => unreachable!(),
                    }
                    builder.ins().iconst(ir::types::I32, 1)
                }
                Operation::NumericConversion => {
                    let float = builder.ins().bitcast(ir::types::F32, ir::MemFlagsData::new(), input);
                    builder.ins().fcvt_to_sint(ir::types::I32, float)
                }
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
            if let Some((destination, source)) = alias {
                function.dfg.replace_with_aliases(destination, source);
                function.layout.remove_inst(destination);
            }
            assert_eq!(convert(&mut function), usize::from(matches!(operation,
                Operation::Bitwise | Operation::RepresentedBitwise | Operation::CompareSelect | Operation::DeadCompareSelect)));
            cranelift_codegen::verify_function(&function, &*isa).unwrap();
        }
    }

    #[test]
    fn a_branch_whose_edges_share_one_arm_keeps_its_blocks() {
        #[cfg(feature = "jit")]
        let publication = super::super::CodePublication::ExecutableMemory;
        #[cfg(not(feature = "jit"))]
        let publication = super::super::CodePublication::RelocatableObject;
        let isa = super::super::target_isa_for("speed", publication).unwrap();
        let mut signature = ir::Signature::new(isa.default_call_conv());
        signature.params.push(ir::AbiParam::new(ir::types::I32));
        signature.returns.push(ir::AbiParam::new(ir::types::I32));
        let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let [entry, arm, join] = [(); 3].map(|_| builder.create_block());
        builder.append_block_params_for_function_params(entry);
        builder.append_block_param(join, ir::types::I32);
        builder.switch_to_block(entry);
        let input = builder.block_params(entry)[0];
        builder.ins().brif(input, arm, &[], arm, &[]);
        builder.switch_to_block(arm);
        let value = builder.ins().bxor_imm_u(input, 1);
        builder.ins().jump(join, &[value.into()]);
        builder.switch_to_block(join);
        let result = builder.block_params(join)[0];
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        assert_eq!(convert(&mut function), 0);
        assert_eq!(function.layout.entry_block(), Some(entry));
        cranelift_codegen::verify_function(&function, &*isa).unwrap();
    }

    #[test]
    fn integer_compare_width_is_checked_even_when_its_result_is_i8() {
        #[cfg(feature = "jit")]
        let publication = super::super::CodePublication::ExecutableMemory;
        #[cfg(not(feature = "jit"))]
        let publication = super::super::CodePublication::RelocatableObject;
        let isa = super::super::target_isa_for("speed", publication).unwrap();
        for ty in [ir::types::I8, ir::types::I16, ir::types::I32, ir::types::I64, ir::types::I128, ir::types::I32X4] {
            let vector = ty.is_vector();
            let result_type = if vector { ty } else { ir::types::I64 };
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.extend([ir::AbiParam::new(ir::types::I64), ir::AbiParam::new(ty), ir::AbiParam::new(ty)]);
            signature.returns.push(ir::AbiParam::new(result_type));
            let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let [entry, left, right, join] = [(); 4].map(|_| builder.create_block());
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(join, result_type);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let yes = if vector { args[1] } else { builder.ins().iconst(result_type, 1) };
            builder.ins().brif(args[0], left, &[], right, &[]);
            builder.switch_to_block(left);
            builder.ins().jump(join, &[yes.into()]);
            builder.switch_to_block(right);
            let predicate = if vector {
                builder.ins().icmp_imm_s(ir::condcodes::IntCC::SignedLessThan, args[0], 0)
            } else { builder.ins().icmp(ir::condcodes::IntCC::SignedLessThan, args[1], args[2]) };
            let no = if vector { args[2] } else { builder.ins().iconst(result_type, 0) };
            let selected = builder.ins().select(predicate, yes, no);
            builder.ins().jump(join, &[selected.into()]);
            builder.switch_to_block(join);
            let result = builder.block_params(join)[0];
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            cranelift_codegen::verify_function(&function, &*isa).unwrap();
            assert_eq!(convert(&mut function), usize::from(ty.is_int() && ty.bits() <= ir::types::I64.bits()), "{ty}");
            cranelift_codegen::verify_function(&function, &*isa).unwrap();
        }
    }

    #[test]
    #[cfg(feature = "jit")]
    fn predicated_integer_comparisons_preserve_canonical_results() {
        let isa = super::super::target_isa("speed").unwrap();
        for cc in [ir::condcodes::IntCC::SignedLessThan, ir::condcodes::IntCC::UnsignedLessThan] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.extend([ir::AbiParam::new(ir::types::I64), ir::AbiParam::new(ir::types::I32), ir::AbiParam::new(ir::types::I32)]);
            signature.returns.push(ir::AbiParam::new(ir::types::I64));
            let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let [entry, left, right, join] = [(); 4].map(|_| builder.create_block());
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(join, ir::types::I64);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let yes = builder.ins().iconst(ir::types::I64, crate::Value::TRUE.to_bits() as i64);
            let no = builder.ins().iconst(ir::types::I64, crate::Value::FALSE.to_bits() as i64);
            builder.ins().brif(args[0], left, &[], right, &[]);
            builder.switch_to_block(left);
            builder.ins().jump(join, &[yes.into()]);
            builder.switch_to_block(right);
            let bits = builder.ins().uextend(ir::types::I64, args[1]);
            builder.ins().bor_imm_u(bits, crate::value::ValueLayout::INT_TAG as i64);
            let predicate = builder.ins().icmp(cc, args[1], args[2]);
            let selected = builder.ins().select(predicate, yes, no);
            builder.ins().jump(join, &[selected.into()]);
            builder.switch_to_block(join);
            let result = builder.block_params(join)[0];
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            assert_eq!(convert(&mut function), 1);
            cranelift_codegen::verify_function(&function, &*isa).unwrap();
            let memory = super::super::compile_function(function, &*isa, None, |compiled, _|
                Ok(crate::vm::compiled::CodeMemory::new(compiled.code_buffer()).expect("executable mapping"))).unwrap();
            // SAFETY: the owned code is pure, bounded and has exactly this C signature.
            let run: unsafe extern "C" fn(u64, i32, i32) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
            for condition in [0, 1, 2, u64::MAX] {
                for left in [i32::MIN, -1, 0, 1, i32::MAX] {
                    for right in [i32::MIN, -1, 0, 1, i32::MAX] {
                        let relation = match cc {
                            ir::condcodes::IntCC::SignedLessThan => left < right,
                            ir::condcodes::IntCC::UnsignedLessThan => (left as u32) < (right as u32),
                            _ => unreachable!(),
                        };
                        let expected = if condition != 0 || relation { crate::Value::TRUE } else { crate::Value::FALSE };
                        assert_eq!(unsafe { run(condition, left, right) }, expected.to_bits());
                    }
                }
            }
        }
    }

    fn nested_select_fixture(isa: &dyn cranelift_codegen::isa::TargetIsa,
        x_type: ir::Type, y_type: ir::Type, decode: bool) -> ir::Function {
        let mut signature = ir::Signature::new(isa.default_call_conv());
        signature.params.extend([ir::AbiParam::new(ir::types::I64); 5]);
        signature.returns.push(ir::AbiParam::new(ir::types::I64));
        let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let args = builder.block_params(entry).to_vec();
        let x = if x_type == ir::types::I64 { args[1] } else { builder.ins().ireduce(x_type, args[1]) };
        let y = if y_type == ir::types::I64 { args[2] } else { builder.ins().ireduce(y_type, args[2]) };
        let yes = if decode { builder.ins().iconst(ir::types::I64, crate::Value::TRUE.to_bits() as i64) } else { args[3] };
        let no = if decode { builder.ins().iconst(ir::types::I64, crate::Value::FALSE.to_bits() as i64) } else { args[4] };
        let left = builder.ins().select(x, yes, no);
        let right = builder.ins().select(y, yes, no);
        let selected = builder.ins().select(args[0], left, right);
        let result = if decode {
            let predicate = builder.ins().icmp(ir::condcodes::IntCC::Equal, selected, yes);
            builder.ins().uextend(ir::types::I64, predicate)
        } else { selected };
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        function
    }

    #[test]
    fn nested_select_boolean_use_eliminates_full_word_constants() {
        #[cfg(feature = "jit")]
        let publication = super::super::CodePublication::ExecutableMemory;
        #[cfg(not(feature = "jit"))]
        let publication = super::super::CodePublication::RelocatableObject;
        let isa = super::super::target_isa_for("speed", publication).unwrap();
        for ty in [ir::types::I8, ir::types::I32, ir::types::I64] {
            let function = nested_select_fixture(&*isa, ty, ty, true);
            cranelift_codegen::verify_function(&function, &*isa).unwrap();
            super::super::compile_function(function, &*isa, None, |_, function| {
                cranelift_codegen::verify_function(function, &*isa).unwrap();
                assert!(!function.layout.blocks().flat_map(|block| function.layout.block_insts(block)).any(|inst|
                    matches!(function.dfg.insts[inst], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                        if [crate::Value::TRUE.to_bits(), crate::Value::FALSE.to_bits()].contains(&(imm.bits() as u64)))),
                    "Boolean use must consume predicates instead of tagged words: {function}");
                Ok(())
            }).unwrap();
        }
    }

    #[test]
    #[cfg(feature = "jit")]
    fn nested_select_factoring_preserves_nonzero_conditions_and_exact_bits() {
        let isa = super::super::target_isa("speed").unwrap();
        let rows = [
            (0, 2, 0, 0x7ff8_0000_0000_0042, 0x8000_0000_0000_0000),
            (2, 2, 0, 0x7ff8_0000_0000_0042, 0x8000_0000_0000_0000),
            (0, 0, 2, u64::MAX, 0),
            (1, 0, 2, 0, u64::MAX),
            (u64::MAX, 1_u64 << ir::types::I32.bits(), 1, 0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210),
            (0, 1, 1_u64 << (ir::types::I64.bits() - 1), crate::Value::TRUE.to_bits(), crate::Value::FALSE.to_bits()),
            (1, 1_u64 << (ir::types::I8.bits() - 1), 0, 0, u64::MAX),
        ];
        for (x_type, y_type) in [(ir::types::I8, ir::types::I8), (ir::types::I32, ir::types::I32),
            (ir::types::I64, ir::types::I64), (ir::types::I8, ir::types::I64), (ir::types::I64, ir::types::I32)] {
            let function = nested_select_fixture(&*isa, x_type, y_type, false);
            cranelift_codegen::verify_function(&function, &*isa).unwrap();
            let memory = super::super::compile_function(function, &*isa, None, |compiled, function| {
                cranelift_codegen::verify_function(function, &*isa).unwrap();
                Ok(crate::vm::compiled::CodeMemory::new(compiled.code_buffer()).expect("executable mapping"))
            }).unwrap();
            // SAFETY: the owned, pure, bounded function has exactly this C signature.
            let run: unsafe extern "C" fn(u64, u64, u64, u64, u64) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
            for (condition, x, y, yes, no) in rows {
                let (chosen, ty) = if condition != 0 { (x, x_type) } else { (y, y_type) };
                let mask = u64::MAX >> (ir::types::I64.bits() - ty.bits());
                let expected = if chosen & mask != 0 { yes } else { no };
                assert_eq!(unsafe { run(condition, x, y, yes, no) }, expected,
                    "{x_type}/{y_type}, c={condition}, x={x}, y={y}");
            }
        }
    }

    // Pack both operand orders into independent bytes so each comparison's
    // canonical result and the arbitrary unselected byte remain observable.
    fn complement_select_fixture(isa: &dyn cranelift_codegen::isa::TargetIsa,
        ty: ir::Type, cc: ir::condcodes::IntCC, mismatched: bool) -> ir::Function {
        use ir::condcodes::CondCode;
        let mut signature = ir::Signature::new(isa.default_call_conv());
        signature.params.extend([ir::AbiParam::new(ir::types::I64); 4]);
        signature.returns.push(ir::AbiParam::new(ir::types::I64));
        let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let args = builder.block_params(entry).to_vec();
        let mut values = [args[0], args[1], args[2]];
        if ty != ir::types::I64 {
            for value in &mut values { *value = builder.ins().ireduce(ty, *value); }
        }
        let other = builder.ins().ireduce(ir::types::I8, args[3]);
        let condition = builder.ins().icmp(cc, values[0], values[1]);
        let opposite = builder.ins().icmp(cc.complement(), values[0], values[if mismatched { 2 } else { 1 }]);
        let left = builder.ins().select(condition, other, opposite);
        let right = builder.ins().select(condition, opposite, other);
        let left = builder.ins().uextend(ir::types::I64, left);
        let right = builder.ins().uextend(ir::types::I64, right);
        let right = builder.ins().ishl_imm_u(right, ir::types::I8.bits() as i64);
        let packed = builder.ins().bor(left, right);
        builder.ins().return_(&[packed]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        function
    }

    #[test]
    fn complementary_comparisons_are_removed_only_for_identical_operands() {
        #[cfg(feature = "jit")]
        let publication = super::super::CodePublication::ExecutableMemory;
        #[cfg(not(feature = "jit"))]
        let publication = super::super::CodePublication::RelocatableObject;
        let isa = super::super::target_isa_for("speed", publication).unwrap();
        for ty in [ir::types::I8, ir::types::I32, ir::types::I64] {
            for &cc in ir::condcodes::IntCC::all() {
                for mismatched in [false, true] {
                    let function = complement_select_fixture(&*isa, ty, cc, mismatched);
                    cranelift_codegen::verify_function(&function, &*isa).unwrap();
                    super::super::compile_function(function, &*isa, None, |_, function| {
                        cranelift_codegen::verify_function(function, &*isa).unwrap();
                        let comparisons = function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
                            .filter(|inst| function.dfg.insts[*inst].opcode() == ir::Opcode::Icmp).count();
                        assert_eq!(comparisons, if mismatched { 2 } else { 1 },
                            "{ty}/{cc:?}, mismatched={mismatched}: {function}");
                        Ok(())
                    }).unwrap();
                }
            }
        }
    }

    #[test]
    #[cfg(feature = "jit")]
    fn complementary_selects_preserve_arbitrary_bytes_and_integer_boundaries() {
        use ir::condcodes::IntCC;
        let isa = super::super::target_isa("speed").unwrap();
        for ty in [ir::types::I8, ir::types::I32, ir::types::I64] {
            let shift = ir::types::I64.bits() - ty.bits();
            let mask = u64::MAX >> shift;
            let sign = 1_u64 << (ty.bits() - 1);
            let values = [0, 1, sign - 1, sign, mask - 1, mask];
            let relation = |cc, x: u64, y: u64| {
                let sx = ((x << shift) as i64) >> shift;
                let sy = ((y << shift) as i64) >> shift;
                match cc {
                    IntCC::Equal => x == y, IntCC::NotEqual => x != y,
                    IntCC::SignedLessThan => sx < sy, IntCC::SignedGreaterThanOrEqual => sx >= sy,
                    IntCC::SignedGreaterThan => sx > sy, IntCC::SignedLessThanOrEqual => sx <= sy,
                    IntCC::UnsignedLessThan => x < y, IntCC::UnsignedGreaterThanOrEqual => x >= y,
                    IntCC::UnsignedGreaterThan => x > y, IntCC::UnsignedLessThanOrEqual => x <= y,
                }
            };
            for &cc in IntCC::all() {
                for mismatched in [false, true] {
                    let function = complement_select_fixture(&*isa, ty, cc, mismatched);
                    let memory = super::super::compile_function(function, &*isa, None, |compiled, function| {
                        cranelift_codegen::verify_function(function, &*isa).unwrap();
                        Ok(crate::vm::compiled::CodeMemory::new(compiled.code_buffer()).expect("executable mapping"))
                    }).unwrap();
                    // SAFETY: the owned pure, bounded function has exactly this C signature.
                    let run: unsafe extern "C" fn(u64, u64, u64, u64) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
                    for x in values { for y in values { for z in [y, x ^ mask] {
                        for other in [0, 1, 2, 0x7f, 0x80, 0xff] {
                            let condition = relation(cc, x, y);
                            let opposite = u64::from(!relation(cc, x, if mismatched { z } else { y }));
                            let left = if condition { other } else { opposite };
                            let right = if condition { opposite } else { other };
                            let expected = left | (right << ir::types::I8.bits());
                            assert_eq!(unsafe { run(x, y, z, other) }, expected,
                                "{ty}/{cc:?}, mismatch={mismatched}, x={x:#x}, y={y:#x}, z={z:#x}, other={other:#x}");
                        }
                    } } }
                }
            }
        }
    }


    #[test]
    #[cfg(all(feature = "jit", target_arch = "aarch64"))]
    fn conditional_increments_preserve_width_wrap_and_nonzero_conditions() {
        // A64 conditional-select class: exclude sf and operand fields; retain
        // op=0 and o2=1, matching CSINC (including its aliases).
        const CSINC_OPCODE_MASK: u32 = 0x7fe0_0c00;
        const CSINC_OPCODE: u32 = 0x1a80_0400;
        #[derive(Clone, Copy, Debug)]
        enum Arm {
            Add { constant_first: bool, other_incremented: bool },
            One,
        }
        let cases = [
            Arm::Add { constant_first: false, other_incremented: false },
            Arm::Add { constant_first: true, other_incremented: false },
            Arm::Add { constant_first: false, other_incremented: true },
            Arm::Add { constant_first: true, other_incremented: true },
            Arm::One,
        ];
        let isa = super::super::target_isa("speed").unwrap();
        for ty in [ir::types::I8, ir::types::I16, ir::types::I32, ir::types::I64] {
            let mask = u64::MAX >> (ir::types::I64.bits() - ty.bits());
            let sign = 1_u64 << (ty.bits() - 1);
            for swapped in [false, true] { for case in cases {
                let mut signature = ir::Signature::new(isa.default_call_conv());
                signature.params.extend([ir::AbiParam::new(ir::types::I64); 3]);
                signature.returns.push(ir::AbiParam::new(ir::types::I64));
                let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let args = builder.block_params(entry).to_vec();
                let mut operands = [args[1], args[2]];
                if ty != ir::types::I64 {
                    for operand in &mut operands { *operand = builder.ins().ireduce(ty, *operand); }
                }
                let one = builder.ins().iconst(ty, 1);
                let (first, alternative) = match case {
                    Arm::Add { constant_first, other_incremented } => {
                        let first = if constant_first { builder.ins().iadd(one, operands[0]) }
                            else { builder.ins().iadd(operands[0], one) };
                        let alternative = if other_incremented { builder.ins().iadd(operands[1], one) }
                            else { operands[1] };
                        (first, alternative)
                    }
                    Arm::One => (one, operands[1]),
                };
                let result = if swapped { builder.ins().select(args[0], alternative, first) }
                    else { builder.ins().select(args[0], first, alternative) };
                let result = if ty == ir::types::I64 { result }
                    else { builder.ins().uextend(ir::types::I64, result) };
                builder.ins().return_(&[result]);
                builder.seal_all_blocks();
                builder.finalize(isa.frontend_config());
                cranelift_codegen::verify_function(&function, &*isa).unwrap();
                let memory = super::super::compile_function(function, &*isa, None, |compiled, function| {
                    cranelift_codegen::verify_function(function, &*isa).unwrap();
                    let increments = compiled.code_buffer().chunks_exact(std::mem::size_of::<u32>())
                        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                        .filter(|word| word & CSINC_OPCODE_MASK == CSINC_OPCODE).count();
                    assert_eq!(increments, 1, "{ty}, swapped={swapped}, {case:?}: {function}");
                    Ok(crate::vm::compiled::CodeMemory::new(compiled.code_buffer()).expect("executable mapping"))
                }).unwrap();
                // SAFETY: this owned, pure, bounded function has exactly this C signature.
                let run: unsafe extern "C" fn(u64, u64, u64) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
                for condition in [0, 1, 2, u64::MAX, 1_u64 << (ir::types::I64.bits() - 1)] {
                    for x in [0, 1, sign - 1, sign, mask - 1, mask, u64::MAX] {
                        for y in [0, sign, mask] {
                            let (first, alternative) = match case {
                                Arm::Add { other_incremented, .. } => (x.wrapping_add(1),
                                    if other_incremented { y.wrapping_add(1) } else { y }),
                                Arm::One => (1, y),
                            };
                            let expected = if (condition != 0) != swapped { first & mask } else { alternative & mask };
                            assert_eq!(unsafe { run(condition, x, y) }, expected,
                                "{ty}, swapped={swapped}, {case:?}, c={condition:#x}, x={x:#x}, y={y:#x}");
                        }
                    }
                }
            } }
        }
    }

}
