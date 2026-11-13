//! The branch shapes the staged code emits that the backend turns into straight-line code.

use super::select::convert;
use cranelift_codegen::ir::{self, InstBuilder, condcodes::IntCC, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

/// `Stage::choose` of two words under a signed comparison, as `Emit` emits it: a diamond whose
/// arms only name their values.
fn minimum_diamond(isa: &dyn cranelift_codegen::isa::TargetIsa) -> ir::Function {
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.extend([ir::AbiParam::new(types::I32); 2]);
    signature.returns.push(ir::AbiParam::new(types::I32));
    let mut function = ir::Function::with_name_signature(ir::UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let (entry, left, right, join) =
        (builder.create_block(), builder.create_block(), builder.create_block(), builder.create_block());
    builder.append_block_params_for_function_params(entry);
    let chosen = builder.append_block_param(join, types::I32);
    builder.switch_to_block(entry);
    let [a, b] = builder.block_params(entry).try_into().unwrap();
    let below = builder.ins().icmp(IntCC::SignedLessThan, a, b);
    builder.ins().brif(below, left, &[], right, &[]);
    builder.switch_to_block(left);
    builder.ins().jump(join, &[a.into()]);
    builder.switch_to_block(right);
    builder.ins().jump(join, &[b.into()]);
    builder.switch_to_block(join);
    builder.ins().return_(&[chosen]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    function
}

fn opcodes(function: &ir::Function) -> Vec<ir::Opcode> {
    function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
        .map(|inst| function.dfg.insts[inst].opcode()).collect()
}

#[test]
fn a_two_arm_minimum_is_one_select_and_no_branch() {
    let isa = super::target_isa().unwrap();
    let mut function = minimum_diamond(&*isa);
    assert_eq!(convert(&mut function), 1);
    cranelift_codegen::verify_function(&function, &*isa).unwrap();
    let opcodes = opcodes(&function);
    assert_eq!(opcodes.iter().filter(|opcode| **opcode == ir::Opcode::Select).count(), 1);
    assert!(!opcodes.contains(&ir::Opcode::Brif));
}
