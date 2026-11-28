//! Select lowering on the jit backend, run as executable code (moved from the core's
//! `vm/jit_backend/select` tests, whose binary does not link the engine).
use crate::ext_traits::*;
use quench_runtime::vm::jit_backend::convert;
use cranelift_codegen::ir::{self, InstBuilder};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

#[test]
#[cfg(feature = "jit")]
fn predicated_integer_comparisons_preserve_canonical_results() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
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
        let yes = builder.ins().iconst(ir::types::I64, quench_runtime::Value::TRUE.to_bits() as i64);
        let no = builder.ins().iconst(ir::types::I64, quench_runtime::Value::FALSE.to_bits() as i64);
        builder.ins().brif(args[0], left, &[], right, &[]);
        builder.switch_to_block(left);
        builder.ins().jump(join, &[yes.into()]);
        builder.switch_to_block(right);
        let bits = builder.ins().uextend(ir::types::I64, args[1]);
        builder.ins().bor_imm_u(bits, quench_runtime::value::ValueLayout::INT_TAG as i64);
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
        let memory = quench_runtime::vm::jit_backend::compile_function_reading(function, &*isa, None, false, |compiled, _|
            Ok(quench_runtime::vm::engine_types::CodeMemory::new(compiled.code_buffer()).expect("executable mapping"))).unwrap();
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
                    let expected = if condition != 0 || relation { quench_runtime::Value::TRUE } else { quench_runtime::Value::FALSE };
                    assert_eq!(unsafe { run(condition, left, right) }, expected.to_bits());
                }
            }
        }
    }
}

#[test]
#[cfg(feature = "jit")]
fn nested_select_factoring_preserves_nonzero_conditions_and_exact_bits() {
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let rows = [
        (0, 2, 0, 0x7ff8_0000_0000_0042, 0x8000_0000_0000_0000),
        (2, 2, 0, 0x7ff8_0000_0000_0042, 0x8000_0000_0000_0000),
        (0, 0, 2, u64::MAX, 0),
        (1, 0, 2, 0, u64::MAX),
        (u64::MAX, 1_u64 << ir::types::I32.bits(), 1, 0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210),
        (0, 1, 1_u64 << (ir::types::I64.bits() - 1), quench_runtime::Value::TRUE.to_bits(), quench_runtime::Value::FALSE.to_bits()),
        (1, 1_u64 << (ir::types::I8.bits() - 1), 0, 0, u64::MAX),
    ];
    for (x_type, y_type) in [(ir::types::I8, ir::types::I8), (ir::types::I32, ir::types::I32),
        (ir::types::I64, ir::types::I64), (ir::types::I8, ir::types::I64), (ir::types::I64, ir::types::I32)] {
        let function = nested_select_fixture(&*isa, x_type, y_type, false);
        cranelift_codegen::verify_function(&function, &*isa).unwrap();
        let memory = quench_runtime::vm::jit_backend::compile_function_reading(function, &*isa, None, false, |compiled, function| {
            cranelift_codegen::verify_function(function, &*isa).unwrap();
            Ok(quench_runtime::vm::engine_types::CodeMemory::new(compiled.code_buffer()).expect("executable mapping"))
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

#[test]
#[cfg(feature = "jit")]
fn complementary_selects_preserve_arbitrary_bytes_and_integer_boundaries() {
    use ir::condcodes::IntCC;
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
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
                let memory = quench_runtime::vm::jit_backend::compile_function_reading(function, &*isa, None, false, |compiled, function| {
                    cranelift_codegen::verify_function(function, &*isa).unwrap();
                    Ok(quench_runtime::vm::engine_types::CodeMemory::new(compiled.code_buffer()).expect("executable mapping"))
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
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
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
            let memory = quench_runtime::vm::jit_backend::compile_function_reading(function, &*isa, None, false, |compiled, function| {
                cranelift_codegen::verify_function(function, &*isa).unwrap();
                let increments = compiled.code_buffer().chunks_exact(std::mem::size_of::<u32>())
                    .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                    .filter(|word| word & CSINC_OPCODE_MASK == CSINC_OPCODE).count();
                assert_eq!(increments, 1, "{ty}, swapped={swapped}, {case:?}: {function}");
                Ok(quench_runtime::vm::engine_types::CodeMemory::new(compiled.code_buffer()).expect("executable mapping"))
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
    let yes = if decode { builder.ins().iconst(ir::types::I64, quench_runtime::Value::TRUE.to_bits() as i64) } else { args[3] };
    let no = if decode { builder.ins().iconst(ir::types::I64, quench_runtime::Value::FALSE.to_bits() as i64) } else { args[4] };
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
    let publication = quench_runtime::vm::jit_backend::CodePublication::ExecutableMemory;
    #[cfg(not(feature = "jit"))]
    let publication = quench_runtime::vm::jit_backend::CodePublication::RelocatableObject;
    let isa = quench_runtime::vm::jit_backend::target_isa_for("speed", publication).unwrap();
    for ty in [ir::types::I8, ir::types::I32, ir::types::I64] {
        let function = nested_select_fixture(&*isa, ty, ty, true);
        cranelift_codegen::verify_function(&function, &*isa).unwrap();
        quench_runtime::vm::jit_backend::compile_function_reading(function, &*isa, None, false, |_, function| {
            cranelift_codegen::verify_function(function, &*isa).unwrap();
            assert!(!function.layout.blocks().flat_map(|block| function.layout.block_insts(block)).any(|inst|
                matches!(function.dfg.insts[inst], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                    if [quench_runtime::Value::TRUE.to_bits(), quench_runtime::Value::FALSE.to_bits()].contains(&(imm.bits() as u64)))),
                "Boolean use must consume predicates instead of tagged words: {function}");
            Ok(())
        }).unwrap();
    }
}

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
    let publication = quench_runtime::vm::jit_backend::CodePublication::ExecutableMemory;
    #[cfg(not(feature = "jit"))]
    let publication = quench_runtime::vm::jit_backend::CodePublication::RelocatableObject;
    let isa = quench_runtime::vm::jit_backend::target_isa_for("speed", publication).unwrap();
    for ty in [ir::types::I8, ir::types::I32, ir::types::I64] {
        for &cc in ir::condcodes::IntCC::all() {
            for mismatched in [false, true] {
                let function = complement_select_fixture(&*isa, ty, cc, mismatched);
                cranelift_codegen::verify_function(&function, &*isa).unwrap();
                quench_runtime::vm::jit_backend::compile_function_reading(function, &*isa, None, false, |_, function| {
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

