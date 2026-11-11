//! One test per hunk of the vendored Cranelift patch (`vendor/cranelift-codegen/QUENCH.patch`,
//! V1-V6 plus the alias-analysis slot): a Cranelift update that drops or breaks a hunk fails here.
//! Each test compiles a small CLIF function for the host and inspects the optimized IR or the
//! lowered code, so no JIT execution is involved.

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    self, AbiParam, AliasRegionData, InstBuilder, MemFlagsData, Signature, UserFuncName, types,
};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

fn isa(unwind_info: bool) -> OwnedTargetIsa {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").unwrap();
    flags.set("unwind_info", if unwind_info { "true" } else { "false" }).unwrap();
    cranelift_native::builder().unwrap().finish(settings::Flags::new(flags)).unwrap()
}

fn build(
    isa: &OwnedTargetIsa,
    call_conv: CallConv,
    params: &[ir::Type],
    returns: &[ir::Type],
    body: impl FnOnce(&mut FunctionBuilder, &[ir::Value]),
) -> ir::Function {
    let mut signature = Signature::new(call_conv);
    signature.params.extend(params.iter().map(|&ty| AbiParam::new(ty)));
    signature.returns.extend(returns.iter().map(|&ty| AbiParam::new(ty)));
    let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let args = builder.block_params(entry).to_vec();
    body(&mut builder, &args);
    builder.finalize(isa.frontend_config());
    function
}

/// The optimized CLIF text.
fn optimized(isa: &OwnedTargetIsa, function: ir::Function) -> String {
    let mut context = Context::for_function(function);
    context.optimize(&**isa, &mut ControlPlane::default()).unwrap();
    context.func.display().to_string()
}

/// The machine code and the vcode text.
fn compiled(isa: &OwnedTargetIsa, function: ir::Function) -> (Vec<u8>, String, Vec<u32>) {
    let mut context = Context::for_function(function);
    context.set_disasm(true);
    context.compile(&**isa, &mut ControlPlane::default()).unwrap();
    let code = context.compiled_code().unwrap();
    (
        code.code_buffer().to_vec(),
        code.vcode.clone().unwrap_or_default(),
        code.bb_starts.clone(),
    )
}

/// `overflow op; brif flag` on two integers of `ty`.
fn overflow_branch(
    isa: &OwnedTargetIsa,
    ty: ir::Type,
    op: impl FnOnce(&mut FunctionBuilder, ir::Value, ir::Value) -> (ir::Value, ir::Value),
) -> String {
    let function = build(isa, CallConv::SystemV, &[ty, ty], &[ty], |b, args| {
        let (result, flag) = op(b, args[0], args[1]);
        let (slow, fast) = (b.create_block(), b.create_block());
        b.ins().brif(flag, slow, &[], fast, &[]);
        b.switch_to_block(slow);
        b.seal_block(slow);
        let zero = b.ins().iconst(ty, 0);
        b.ins().return_(&[zero]);
        b.switch_to_block(fast);
        b.seal_block(fast);
        b.ins().return_(&[result]);
    });
    compiled(isa, function).1
}

/// V1: a signed 32/64-bit overflow op followed by its branch is one arithmetic op and `jo`.
#[test]
fn v1_signed_overflow_branch_fuses_to_jo() {
    let isa = isa(false);
    for ty in [types::I32, types::I64] {
        let cases: [(&str, fn(&mut FunctionBuilder, ir::Value, ir::Value) -> (ir::Value, ir::Value)); 3] = [
            ("sadd", |b, x, y| b.ins().sadd_overflow(x, y)),
            ("ssub", |b, x, y| b.ins().ssub_overflow(x, y)),
            ("smul", |b, x, y| b.ins().smul_overflow(x, y)),
        ];
        for (name, op) in cases {
            let text = overflow_branch(&isa, ty, op);
            assert!(text.contains("jo "), "{name}.{ty}: no jo in\n{text}");
            assert!(!text.contains("seto"), "{name}.{ty}: flag materialized in\n{text}");
        }
    }
}

/// V1: the signed 32/64-bit product is the two-operand `imul` (no `rax`/`rdx` pair).
#[test]
fn v1_smul_overflow_uses_two_operand_imul() {
    let isa = isa(false);
    for ty in [types::I32, types::I64] {
        let text = overflow_branch(&isa, ty, |b, x, y| b.ins().smul_overflow(x, y));
        assert!(text.contains("imul"), "{ty}: {text}");
        assert!(!text.contains("rdx"), "{ty}: one-operand form in\n{text}");
    }
}

/// V1: a constant factor is the three-operand immediate form.
#[test]
fn v1_smul_overflow_constant_factor_is_an_immediate() {
    let isa = isa(false);
    let text = overflow_branch(&isa, types::I32, |b, x, _| {
        let k = b.ins().iconst(types::I32, 10);
        b.ins().smul_overflow(x, k)
    });
    assert!(text.contains("imul"), "{text}");
    assert!(!text.contains("seto") && text.contains("jo "), "{text}");
}

/// V2: the header of a small loop without calls starts on a 64-byte boundary.
#[test]
fn v2_small_loop_header_is_cache_line_aligned() {
    let isa = isa(false);
    let function = build(&isa, CallConv::SystemV, &[types::I64], &[types::I64], |b, args| {
        let (header, body, exit) = (b.create_block(), b.create_block(), b.create_block());
        b.append_block_param(header, types::I64);
        let zero = b.ins().iconst(types::I64, 0);
        b.ins().jump(header, &[zero.into()]);
        b.switch_to_block(header);
        let i = b.block_params(header)[0];
        let more = b.ins().icmp(IntCC::UnsignedLessThan, i, args[0]);
        b.ins().brif(more, body, &[], exit, &[]);
        b.switch_to_block(body);
        b.seal_block(body);
        let next = b.ins().iadd_imm_s(i, 1);
        b.ins().jump(header, &[next.into()]);
        b.seal_block(header);
        b.switch_to_block(exit);
        b.seal_block(exit);
        b.ins().return_(&[i]);
    });
    let (code, _, _) = compiled(&isa, function);
    // Alignment padding is a multi-byte `nop` (0f 1f /0); the rest of the function has none.
    assert!(code.windows(2).any(|w| w == [0x0f, 0x1f]), "no padding before the loop header: {code:02x?}");
}

/// V3: the operand with the longer pure chain elaborates first, even when it is the second one.
#[test]
fn v3_taller_operand_elaborates_first() {
    let isa = isa(false);
    let function = build(&isa, CallConv::SystemV, &[types::I64; 5], &[types::I64], |b, p| {
        let short = b.ins().imul(p[0], p[4]);
        let mut tall = b.ins().imul(p[1], p[2]);
        for step in [p[3], p[2], p[1], p[3], p[4], p[0]] {
            tall = b.ins().bxor(tall, step);
            tall = b.ins().imul(tall, step);
        }
        let sum = b.ins().isub(short, tall);
        b.ins().return_(&[sum]);
    });
    let text = optimized(&isa, function);
    let line = |needle: &str| text.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} in\n{text}"));
    assert!(line("imul v1, v2") < line("imul v0, v4"), "the short chain led:\n{text}");
}

/// V4: `eq(fcmp, 0)` is the complementary `fcmp`, and an integer constant bitcast to a float is a float constant.
#[test]
fn v4_fcmp_complement_and_bitcast_constant() {
    let isa = isa(false);
    let function = build(&isa, CallConv::SystemV, &[types::F64, types::F64], &[types::I8], |b, p| {
        let lt = b.ins().fcmp(FloatCC::LessThan, p[0], p[1]);
        let zero = b.ins().iconst(types::I8, 0);
        let not = b.ins().icmp(IntCC::Equal, lt, zero);
        b.ins().return_(&[not]);
    });
    let text = optimized(&isa, function);
    assert!(text.contains("fcmp uge"), "{text}");
    assert!(!text.contains("icmp"), "{text}");
    let function = build(&isa, CallConv::SystemV, &[], &[types::F64], |b, _| {
        let bits = b.ins().iconst(types::I64, 0x4008_0000_0000_0000);
        let value = b.ins().bitcast(types::F64, ir::MemFlagsData::new(), bits);
        b.ins().return_(&[value]);
    });
    let text = optimized(&isa, function);
    assert!(text.contains("f64const"), "{text}");
    assert!(!text.contains("bitcast"), "{text}");
}

/// V5: a frame's epilogue restores `rsp` from `rbp`, not with a dead `add rsp, n` first.
#[test]
fn v5_epilogue_has_no_dead_rsp_adjustment() {
    let isa = isa(false);
    let function = build(&isa, CallConv::SystemV, &[types::I64], &[types::I64], |b, p| {
        let slot = b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot, 64, 3));
        b.ins().stack_store(types::I64, p[0], slot, 0);
        let v = b.ins().stack_load(types::I64, types::I64, slot, 0);
        b.ins().return_(&[v]);
    });
    let (code, _, _) = compiled(&isa, function);
    // `add rsp, imm8` and `add rsp, imm32`.
    let adds = code.windows(3).filter(|w| w[0] == 0x48 && (w[1] == 0x83 || w[1] == 0x81) && w[2] == 0xc4).count();
    assert_eq!(adds, 0, "dead add rsp in {code:02x?}");
}

/// V5: an i32 load already zeroes the upper half, so zero-extending it adds no move.
#[test]
fn v5_zero_extended_i32_load_adds_no_move() {
    let isa = isa(false);
    let function = build(&isa, CallConv::SystemV, &[types::I64], &[types::I64], |b, p| {
        let v = b.ins().load(types::I32, MemFlagsData::new(), p[0], 0);
        let r = b.ins().uextend(types::I64, v);
        b.ins().return_(&[r]);
    });
    let text = compiled(&isa, function).1;
    let moves = text.lines().filter(|l| l.trim_start().starts_with("movl")).count();
    assert_eq!(moves, 1, "the load only, no zero-extending move:\n{text}");
}

/// V6: with `unwind_info=false`, a Tail-convention function under register pressure saves no
/// callee-saved register; with unwind info the stock convention saves `rbx`.
#[test]
fn v6_tail_convention_saves_no_callee_saved_without_unwind_info() {
    // `mov [rsp+disp8], rbx`: how the prologue saves the first callee-saved register.
    const SAVE_RBX: [u8; 4] = [0x48, 0x89, 0x5c, 0x24];
    let pressure = |isa: &OwnedTargetIsa| {
        let function = build(isa, CallConv::Tail, &[types::I64], &[], |b, p| {
            let values: Vec<_> =
                (0..14).map(|k| b.ins().load(types::I64, MemFlagsData::new(), p[0], k * 8)).collect();
            let sum = values.iter().fold(values[0], |sum, value| b.ins().iadd(sum, *value));
            b.ins().store(MemFlagsData::new(), sum, p[0], 0);
            for (k, value) in values.iter().enumerate() {
                b.ins().store(MemFlagsData::new(), *value, p[0], (k as i32 + 1) * 8);
            }
            b.ins().return_(&[]);
        });
        compiled(isa, function).0
    };
    let saves = |code: &[u8]| code[..code.len().min(40)].windows(4).any(|w| w == SAVE_RBX);
    assert!(saves(&pressure(&isa(true))), "the stock tail convention saves rbx under pressure");
    assert!(!saves(&pressure(&isa(false))), "no callee-saved save without unwind info");
}

/// The alias-analysis slot (`LastStores::other`): a load of a frame slot after a store and a call
/// stays a load, and a region-less load is not forwarded across a store to another region.
#[test]
fn alias_untagged_load_sees_tagged_store_and_calls_keep_loads() {
    let isa = isa(false);
    let loads = |text: &str| text.lines().filter(|l| l.contains(" = load.")).count();

    // store slot; call; load slot: the call may write the slot, so the load stays.
    let function = build(&isa, CallConv::SystemV, &[types::I64, types::I64], &[types::I64], |b, p| {
        let region = b.func.dfg.alias_regions.insert(AliasRegionData { user_id: 1, description: "slot".into() });
        let tagged = MemFlagsData::new().with_alias_region(Some(region));
        b.ins().store(tagged, p[1], p[0], 8);
        let signature = b.import_signature(Signature::new(CallConv::SystemV));
        let name = b.func.declare_imported_user_function(ir::UserExternalName::new(0, 1));
        let callee = b.import_function(ir::ExtFuncData {
            name: ir::ExternalName::user(name),
            signature,
            colocated: false,
            patchable: false,
        });
        b.ins().call(callee, &[]);
        let v = b.ins().load(types::I64, tagged, p[0], 8);
        b.ins().return_(&[v]);
    });
    let text = optimized(&isa, function);
    assert_eq!(loads(&text), 1, "the load after the call was forwarded:\n{text}");

    // untagged load; tagged store to another region; untagged load: both loads stay.
    let function = build(&isa, CallConv::SystemV, &[types::I64, types::I64, types::I64], &[types::I64], |b, p| {
        let region = b.func.dfg.alias_regions.insert(AliasRegionData { user_id: 2, description: "other".into() });
        let first = b.ins().load(types::I64, MemFlagsData::new(), p[0], 0);
        b.ins().store(MemFlagsData::new().with_alias_region(Some(region)), p[1], p[2], 0);
        let second = b.ins().load(types::I64, MemFlagsData::new(), p[0], 0);
        let sum = b.ins().iadd(first, second);
        b.ins().return_(&[sum]);
    });
    let text = optimized(&isa, function);
    assert_eq!(loads(&text), 2, "an untagged load was reused across a tagged store:\n{text}");
}
