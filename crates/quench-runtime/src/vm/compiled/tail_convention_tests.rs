//! The Tail calling convention a body-to-body call with hot state in arguments needs: a callee
//! returning two words, entered from the host through a C-ABI trampoline. (`return_call` needs
//! frame pointers on x64, which executable-memory publication does not preserve.)

use super::*;
use cranelift_codegen::isa::CallConv;

fn tail(params: usize, returns: usize) -> ir::Signature {
    let mut signature = ir::Signature::new(CallConv::Tail);
    signature.params.extend((0..params).map(|_| AbiParam::new(types::I64)));
    signature.returns.extend((0..returns).map(|_| AbiParam::new(types::I64)));
    signature
}

fn build(signature: ir::Signature, body: &dyn Fn(&mut FunctionBuilder<'_>, &[ir::Value])) -> CodeMemory {
    let isa = super::super::jit_backend::target_isa().unwrap();
    let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let parameters = builder.block_params(entry).to_vec();
    body(&mut builder, &parameters);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    compile_native_function(function, &*isa, usize::MAX, CodeMemory::new).unwrap()
}

#[test]
fn a_tail_convention_callee_returns_two_words_to_a_host_trampoline() {
    let callee = build(tail(2, 2), &|builder, parameters| {
        let sum = builder.ins().iadd(parameters[0], parameters[1]);
        let difference = builder.ins().isub(parameters[0], parameters[1]);
        builder.ins().return_(&[sum, difference]);
    });
    let mut host = ir::Signature::new(super::super::jit_backend::target_isa().unwrap().default_call_conv());
    host.params.extend((0..4).map(|_| AbiParam::new(types::I64)));
    host.returns.push(AbiParam::new(types::I64));
    let trampoline = build(host, &|builder, parameters| {
        let signature = builder.import_signature(tail(2, 2));
        let call = builder.ins().call_indirect(signature, parameters[0], &[parameters[1], parameters[2]]);
        let results = builder.inst_results(call).to_vec();
        builder.ins().store(MemFlagsData::trusted(), results[1], parameters[3], 0);
        builder.ins().return_(&[results[0]]);
    });
    // SAFETY: the trampoline takes (target, a, b, out) in the host convention.
    let enter: unsafe extern "C" fn(u64, u64, u64, *mut u64) -> u64 = unsafe { std::mem::transmute(trampoline.pointer()) };
    let mut second = 0_u64;
    assert_eq!(unsafe { enter(callee.pointer() as u64, 40, 2, &mut second) }, 42);
    assert_eq!(second, 38);
}
