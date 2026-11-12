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

/// Cost of a call through each convention with six values live across it (`cargo test
/// --profile experiment -p quench-runtime --lib tail_vs_system_call -- --ignored --nocapture`):
/// the Tail convention has no callee-saved registers, so the caller spills what the C one keeps.
#[test]
#[ignore = "measurement"]
fn tail_vs_system_call_cost() {
    fn run(conv: CallConv) -> f64 {
        let signature = |conv| {
            let mut s = ir::Signature::new(conv);
            s.params.extend((0..2).map(|_| AbiParam::new(types::I64)));
            s.returns.extend((0..2).map(|_| AbiParam::new(types::I64)));
            s
        };
        let callee = build(signature(conv), &|builder, p| {
            let sum = builder.ins().iadd(p[0], p[1]);
            builder.ins().return_(&[sum, p[1]]);
        });
        let host_conv = super::super::jit_backend::target_isa().unwrap().default_call_conv();
        let mut host = ir::Signature::new(host_conv);
        host.params.extend((0..2).map(|_| AbiParam::new(types::I64)));
        host.returns.push(AbiParam::new(types::I64));
        let driver = build(host, &|builder, p| {
            let header = builder.create_block();
            builder.append_block_param(header, types::I64);
            for _ in 0..6 { builder.append_block_param(header, types::I64); }
            let exit = builder.create_block();
            builder.append_block_param(exit, types::I64);
            let one = builder.ins().iconst(types::I64, 1);
            let zero = builder.ins().iconst(types::I64, 0);
            let start: [ir::Value; 7] = [p[0], p[1], one, zero, p[0], p[1], one];
            builder.ins().jump(header, &start.map(ir::BlockArg::from));
            builder.switch_to_block(header);
            let params = builder.block_params(header).to_vec();
            let target = builder.ins().iconst(types::I64, callee.pointer() as i64);
            let sig = builder.import_signature(signature(conv));
            let call = builder.ins().call_indirect(sig, target, &[params[1], params[2]]);
            let r = builder.inst_results(call).to_vec();
            let live = [0, 3, 4, 5, 6].map(|i| builder.ins().iadd(params[i], r[0]));
            let next = builder.ins().iadd_imm_s(params[0], -1);
            let again = builder.ins().icmp_imm_u(IntCC::NotEqual, next, 0);
            let done = builder.ins().iadd(live[1], live[2]);
            let total = builder.ins().iadd(done, live[3]);
            let total = builder.ins().iadd(total, live[4]);
            let args = [next, r[1], live[0], live[1], live[2], live[3], live[4]].map(ir::BlockArg::from);
            builder.ins().brif(again, header, &args, exit, &[ir::BlockArg::from(total)]);
            builder.switch_to_block(exit);
            let result = builder.block_params(exit)[0];
            builder.ins().return_(&[result]);
        });
        // SAFETY: (iterations, seed) -> word in the host convention.
        let drive: unsafe extern "C" fn(u64, u64) -> u64 = unsafe { std::mem::transmute(driver.pointer()) };
        let calls = 200_000_000_u64;
        let start = std::time::Instant::now();
        std::hint::black_box(unsafe { drive(calls, 3) });
        start.elapsed().as_secs_f64() * 1e9 / calls as f64
    }
    let (system, tail) = (run(CallConv::SystemV), run(CallConv::Tail));
    eprintln!("ns per call with live state: SystemV {system:.3}  Tail {tail:.3}");
}
