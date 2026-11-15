use super::*;

#[test]
fn native_narrow_store_encoding_matches_shared_values() {
    use crate::value::NarrowLayout as N;
    let isa = super::super::jit_backend::target_isa().unwrap();
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let value = builder.block_params(entry)[0];
    let mut stage = object_slots::Emit { builder: &mut builder, property_values: None, gate: None };
    let encoded = crate::staged::store::narrow_form(&mut stage, value, typed_ir::Type::ANY);
    builder.ins().return_(&[encoded]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    let memory = compile_native_function(function, &*isa, usize::MAX, CodeMemory::new).unwrap();
    // SAFETY: the generated function accepts and returns a single u64.
    let encode: unsafe extern "C" fn(u64) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
    let limit = 1_i32 << (N::INTEGER_BITS - 1);
    let mut values = vec![Value::NULL, Value::UNDEFINED, Value::TRUE, Value::DELETED,
        Value::heap(0), Value::direct_string(0), Value::number(-0.0), Value::number(1.5), Value::number(f64::NAN)];
    values.extend([i32::MIN, -limit - 1, -limit, -1, 0, 1, limit - 1, limit, i32::MAX].map(Value::integer));
    values.extend([0, 1, (1 << N::INDEX_BITS) - 1, 1 << N::INDEX_BITS, u32::MAX].map(Value::direct_object));
    for value in values {
        let expected = value.to_narrow().map_or(u64::MAX, u64::from);
        assert_eq!(unsafe { encode(value.to_bits()) }, expected, "{value:?}");
    }
}

#[test]
fn native_narrow_decoder_matches_value_bits() {
    use crate::value::NarrowLayout as N;
    let isa = super::super::jit_backend::target_isa().unwrap();
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(types::I32));
    signature.returns.push(AbiParam::new(types::I64));
    let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let narrow = builder.block_params(entry)[0];
    let done = builder.create_block();
    let result = builder.append_block_param(done, types::I64);
    emit_narrow_read(&mut builder, narrow, NarrowTable::Process, done);
    builder.switch_to_block(done);
    builder.ins().return_(&[result]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    let memory = compile_native_function(function, &*isa, usize::MAX, CodeMemory::new).unwrap();
    // SAFETY: the function above accepts one u32 and returns its decoded u64.
    let decode: unsafe extern "C" fn(u32) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
    let indices = [0, 1, (1 << N::INTEGER_BITS) - 1, u32::MAX];
    for code in 0..=N::CODE_MASK {
        for index in indices {
            let narrow = (index << N::INDEX_SHIFT) | (code << N::CODE_SHIFT);
            assert_eq!(unsafe { decode(narrow) }, Value::from_narrow(narrow).to_bits());
        }
    }
    let half = 1_i32 << (N::INTEGER_BITS - 1);
    for integer in [i32::MIN, -half, -half + 1, -1, 0, 1, half - 1, half, i32::MAX] {
        let narrow = ((integer as u32) << N::CODE_SHIFT) | N::INTEGER_FLAG;
        assert_eq!(unsafe { decode(narrow) }, Value::from_narrow(narrow).to_bits());
    }
}

#[cfg(all(target_arch = "aarch64", any(feature = "jit", feature = "aot")))]
#[test]
fn aarch64_logical_right_shift_selection_preserves_all_integer_widths() {
    // A64 Logical (shifted register): fixed class bits, N=0; shift kind
    // occupies bits23:22. The test checks actual emitted instructions.
    const LOGICAL_CLASS_MASK: u32 = 0x1f20_0000;
    const LOGICAL_CLASS: u32 = 0x0a00_0000;
    const SHIFT_KIND_OFFSET: u32 = 22;
    const SHIFT_KIND_MASK: u32 = 0b11 << SHIFT_KIND_OFFSET;
    const LSR_KIND: u32 = 0b01 << SHIFT_KIND_OFFSET;
    const ASR_KIND: u32 = 0b10 << SHIFT_KIND_OFFSET;
    const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLSH");
    use crate::vm::operations::NumberBitwise;
    let isa = super::super::jit_backend::target_isa().unwrap();
    for ty in [types::I8, types::I16, types::I32, types::I64] {
        let width = ty.bits();
        let mask = u64::MAX >> (u64::BITS - width);
        for operation in [NumberBitwise::And, NumberBitwise::Or, NumberBitwise::Xor] { for signed in [false, true] { for reversed in [false, true] {
            for count in [0_u64, 1, 17, 31, 32, 63, 64, u64::MAX] {
                let mut signature = ir::Signature::new(isa.default_call_conv());
                signature.params.extend([AbiParam::new(types::I64), AbiParam::new(types::I64)]);
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let block = builder.create_block();
                builder.append_block_params_for_function_params(block);
                builder.switch_to_block(block);
                let args = builder.block_params(block).to_vec();
                let left = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                let right = if ty == types::I64 { args[1] } else { builder.ins().ireduce(ty, args[1]) };
                let shifted = if signed { builder.ins().sshr_imm_u(right, count as i64) }
                    else { builder.ins().ushr_imm_u(right, count as i64) };
                let (left, right) = if reversed { (shifted, left) } else { (left, shifted) };
                let result = match operation {
                    NumberBitwise::And => builder.ins().band(left, right),
                    NumberBitwise::Or => builder.ins().bor(left, right),
                    NumberBitwise::Xor => builder.ins().bxor(left, right),
                    _ => unreachable!("logical operation matrix"),
                };
                let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                builder.ins().return_(&[result]);
                builder.seal_all_blocks();
                builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
                let bytes = compiled.buffer.data();
                if width >= u32::BITS && count == 17 {
                    let kind = if signed { ASR_KIND } else { LSR_KIND };
                    assert!(bytes.as_chunks::<{ std::mem::size_of::<u32>() }>().0.iter().any(|bytes| {
                        let word = u32::from_le_bytes(*bytes);
                        word & LOGICAL_CLASS_MASK == LOGICAL_CLASS && word & SHIFT_KIND_MASK == kind
                    }), "right shift must fold: {ty} operation{operation:?} signed{signed} reversed{reversed}");
                }
                let memory = CodeMemory::new(bytes).unwrap();
                // SAFETY: this verified native function has exactly two
                // u64 inputs and one u64 result under the host C ABI.
                let entry = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
                for left in [0, 1, u64::MAX, 1_u64 << (width - 1)] {
                    for right in [0, 1, u64::MAX, 1_u64 << (width - 1)] {
                        let shift = count as u32 & (width - 1);
                        let right = right & mask;
                        let shifted = if signed {
                            let widened = ((right << (u64::BITS - width)) as i64) >> (u64::BITS - width);
                            (widened >> shift) as u64 & mask
                        } else { right >> shift };
                        let expected = match operation { NumberBitwise::And => left & shifted, NumberBitwise::Or => left | shifted,
                            NumberBitwise::Xor => left ^ shifted, _ => unreachable!("logical operation matrix") } & mask;
                        assert_eq!(unsafe { entry(left, right) }, expected,
                            "{ty} operation{operation:?} signed{signed} reversed{reversed} shift{count}");
                    }
                }
            }
        }}}
    }
}

#[cfg(all(feature = "aot", target_arch = "aarch64"))]
fn assert_aarch64_cfg_edges(code: &cranelift_codegen::CompiledCode) {
    // A64 direct branch encodings; this fixture has no calls, jump tables
    // or literal islands. Compare the metadata with actual branch targets.
    const B_MASK: u32 = 0xfc00_0000;
    const B: u32 = 0x1400_0000;
    const B_COND_MASK: u32 = 0xff00_0010;
    const B_COND: u32 = 0x5400_0000;
    const CB_MASK: u32 = 0x7e00_0000;
    const CB: u32 = 0x3400_0000;
    const RET_MASK: u32 = 0xffff_fc1f;
    const RET: u32 = 0xd65f_0000;
    const RETAA: u32 = 0xd65f_0bff;
    const RETAB: u32 = 0xd65f_0fff;
    const BRANCH_BITS: u32 = 26;
    const CONDITIONAL_BITS: u32 = 19;
    const CONDITIONAL_SHIFT: u32 = 5;
    let bytes = code.buffer.data();
    let mut actual = Vec::new();
    for (index, &start) in code.bb_starts.iter().enumerate() {
        let end = code.bb_starts.get(index + 1).copied().unwrap_or(bytes.len() as u32);
        let mut terminal = false;
        for pc in (start..end).step_by(std::mem::size_of::<u32>()) {
            let word = u32::from_le_bytes(bytes[pc as usize..pc as usize + std::mem::size_of::<u32>()].try_into().unwrap());
            let branch = if word & B_MASK == B { terminal = true; Some((word, BRANCH_BITS)) }
                else if word & B_COND_MASK == B_COND || word & CB_MASK == CB {
                    Some((word >> CONDITIONAL_SHIFT, CONDITIONAL_BITS))
                } else { None };
            if let Some((immediate, bits)) = branch {
                let signed = ((immediate << (u32::BITS - bits)) as i32) >> (u32::BITS - bits);
                actual.push((start, (i64::from(pc) + i64::from(signed) * std::mem::size_of::<u32>() as i64) as u32));
            }
            terminal |= word & RET_MASK == RET || word == RETAA || word == RETAB;
            if terminal { break; }
        }
        if !terminal && end < bytes.len() as u32 { actual.push((start, end)); }
    }
    actual.sort_unstable(); actual.dedup();
    let mut recorded = code.bb_edges.clone();
    recorded.sort_unstable(); recorded.dedup();
    assert_eq!(recorded, actual, "machine CFG must follow physical blocks: starts{:?}, bytes{:x?}, {:?}",
        code.bb_starts, bytes.as_chunks::<{ std::mem::size_of::<u32>() }>().0.iter().map(|word| u32::from_le_bytes(*word)).collect::<Vec<_>>(), code.vcode);
}

#[cfg(feature = "aot")]
#[test]
fn private_store_commits_preserve_overlapping_bytes_and_zero_trips() {
    let isa = super::super::jit_backend::target_isa().unwrap();
    let mut signature = Signature::new(isa.default_call_conv());
    signature.params.extend([AbiParam::new(types::I64); 2]);
    let mut function = ir::Function::with_name_signature(UserFuncName::testcase("private_store_commit"), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block(); let body = builder.create_block(); let exit = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    let count = builder.append_block_param(body, types::I64);
    builder.switch_to_block(entry);
    let args = builder.block_params(entry).to_vec();
    builder.ins().brif(args[1], body, &[args[1].into()], exit, &[]);
    builder.switch_to_block(body);
    let word = builder.ins().ireduce(types::I32, count);
    let first = builder.ins().store(MemFlagsData::trusted(), word, args[0], 0);
    const BYTE_XOR: u8 = 7;
    const BYTE_OFFSET: usize = 1;
    let byte = builder.ins().ireduce(types::I8, count);
    let byte = builder.ins().bxor_imm_u(byte, i64::from(BYTE_XOR));
    let second = builder.ins().store(MemFlagsData::trusted(), byte, args[0], BYTE_OFFSET as i32);
    builder.func.private_stores.extend([first, second]);
    let next = builder.ins().iadd_imm_s(count, -1);
    builder.ins().brif(next, body, &[next.into()], exit, &[]);
    builder.switch_to_block(exit); builder.ins().return_(&[]);
    builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
    let mut compiler = cranelift_codegen::Context::for_function(function);
    let code = compiler.compile(&*isa, &mut Default::default()).unwrap();
    let memory = CodeMemory::new(code.buffer.data()).unwrap();
    compiler.compute_cfg(); compiler.compute_domtree(); compiler.compute_loop_analysis();
    assert!(!compiler.func.layout.blocks().filter(|block| compiler.loop_analysis.innermost_loop(*block).is_some())
        .flat_map(|block| compiler.func.layout.block_insts(block))
        .any(|inst| compiler.func.dfg.insts[inst].opcode().can_store()), "{}", compiler.func);
    // SAFETY: the verified signature receives an aligned, live word and a count.
    let run: unsafe extern "C" fn(*mut u64, u64) = unsafe { std::mem::transmute(memory.pointer()) };
    const SENTINEL: u64 = 0x9182_7364_5546_3728;
    for count in [0, 1, 2, 17, 257] {
        let mut actual = SENTINEL; let mut expected = SENTINEL.to_ne_bytes();
        if count != 0 {
            expected[..size_of::<u32>()].copy_from_slice(&1_u32.to_ne_bytes());
            expected[BYTE_OFFSET] = 1 ^ BYTE_XOR;
        }
        unsafe { run(&mut actual, count); }
        assert_eq!(actual.to_ne_bytes(), expected, "count={count}");
    }
}

#[cfg(feature = "aot")]
#[test]
fn conditional_xor_masks_preserve_integer_words_and_bit_positions() {
    let isa = super::super::jit_backend::target_isa().unwrap();
    for ty in [types::I8, types::I16, types::I32, types::I64] {
        let word = u64::MAX >> (u64::BITS - ty.bits());
        let sign = 1_u64 << (ty.bits() - 1);
        for tested in (0..ty.bits()).map(|bit| 1_u64 << bit).chain([0, word]) { for invert in [false, true] {
          for comparison in [None, Some(IntCC::Equal), Some(IntCC::NotEqual)] { for factored in [false, true] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::testcase("conditional_xor"), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let args = builder.block_params(entry).to_vec();
                let values: Vec<_> = args.into_iter().map(|value| if ty == types::I64 { value }
                    else { builder.ins().ireduce(ty, value) }).collect();
                let condition = builder.ins().band_imm_u(values[0], tested as i64);
                let condition = comparison.map_or(condition, |cc| builder.ins().icmp_imm_u(cc, condition, 0));
                let result = if factored {
                    let zero = builder.ins().iconst(ty, 0);
                    let selected = if invert { builder.ins().select(condition, zero, values[2]) }
                        else { builder.ins().select(condition, values[2], zero) };
                    builder.ins().bxor(values[1], selected)
                } else {
                    let changed = builder.ins().bxor(values[1], values[2]);
                    if invert { builder.ins().select(condition, values[1], changed) }
                    else { builder.ins().select(condition, changed, values[1]) }
                };
                let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                builder.ins().return_(&[result]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                if tested.is_power_of_two() {
                    assert!(!compiler.func.layout.blocks().flat_map(|block| compiler.func.layout.block_insts(block))
                        .any(|inst| compiler.func.dfg.insts[inst].opcode() == ir::Opcode::Select), "{ty} mask{tested} invert{invert}: {}", compiler.func);
                }
                let memory = CodeMemory::new(compiler.compiled_code().unwrap().buffer.data()).unwrap();
                // SAFETY: verified native ABI accepts and returns machine words; mapping stays live.
                let run: unsafe extern "C" fn(u64, u64, u64) -> u64 = unsafe { std::mem::transmute(memory.pointer()) };
                let sources: Vec<_> = if ty == types::I8 { (0..=word).collect() }
                    else { vec![0, 1, sign, word, tested, word ^ tested] };
                let deltas: Vec<_> = if ty == types::I8 { (0..=word).collect() }
                    else { vec![0, 1, sign, word, tested, word ^ tested] };
                for source in sources { for base in [0, 1, sign, word] { for &delta in &deltas {
                    let expected = base ^ if (source & tested != 0) ^ invert ^ (comparison == Some(IntCC::Equal)) { delta } else { 0 };
                    assert_eq!(unsafe { run(source, base, delta) }, expected, "{ty} mask{tested} invert{invert} factored{factored} source{source} base{base} delta{delta}");
                }}}
            }}}}
        }
    }

    #[test]
    fn signed_shift_pairs_preserve_width_masking_and_shared_inputs() {
        use cranelift_codegen::settings::Configurable;
        let mut flags = cranelift_codegen::settings::builder();
        // Exercise instruction selection directly, including widths for which
        // the middle-end can otherwise replace the pair with an IR extension.
        flags.set("opt_level", "none").unwrap();
        flags.set("enable_verifier", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let width = ty.bits();
            let mask = u64::MAX >> (u64::BITS - width);
            for amount in 0..width { for delta in [0, 1] { for shared in [false, true] { for masked in [false, true] {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.push(AbiParam::new(types::I64));
                signature.params.push(AbiParam::new(types::I64));
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::testcase("signed_shift_pair"), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let input = builder.block_params(entry)[0];
                let filter = builder.block_params(entry)[1];
                let filter = if ty == types::I64 { filter } else { builder.ins().ireduce(ty, filter) };
                let input = if ty == types::I64 { input } else { builder.ins().ireduce(ty, input) };
                let left = builder.ins().ishl_imm_u(input, i64::from(amount) - i64::from(width));
                let right = builder.ins().sshr_imm_u(left, i64::from(amount + width + delta));
                let result = if shared { builder.ins().bxor(left, right) } else { right };
                let result = if masked { builder.ins().band(result, filter) } else { result };
                let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
                builder.ins().return_(&[result]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                let code = compiler.compiled_code().unwrap().buffer.data();
                #[cfg(target_arch = "aarch64")]
                if ty == types::I64 && amount != 0 && delta == 0 && !shared {
                    // A64 Bitfield class, ignoring sf/opc and operand fields.
                    const BITFIELD_CLASS_MASK: u32 = 0x1f80_0000;
                    const BITFIELD_CLASS: u32 = 0x1300_0000;
                    const SIGNED_BITFIELD_MASK: u32 = 0x7f80_0000;
                    let bitfields = code.chunks_exact(std::mem::size_of::<u32>())
                        .filter(|word| u32::from_le_bytes((*word).try_into().unwrap()) & BITFIELD_CLASS_MASK == BITFIELD_CLASS).count();
                    assert_eq!(bitfields, 1, "one signed extract replaces both shifts, amount={amount}");
                    assert!(code.chunks_exact(std::mem::size_of::<u32>()).any(|word|
                        u32::from_le_bytes(word.try_into().unwrap()) & SIGNED_BITFIELD_MASK == BITFIELD_CLASS),
                        "logical consumers must retain the signed extract, amount={amount} masked={masked}");
                }
                let memory = CodeMemory::new(code).unwrap();
                // SAFETY: verified two-word argument/one-word result ABI and live code.
                let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
                let retained_sign = 1_u64 << (width - amount - 1);
                for value in (0_u64..=255).chain([retained_sign, retained_sign - 1, mask, mask >> 1, !mask, u64::MAX]) { for filter in [0, 1, mask, !mask] {
                    let left = value.wrapping_shl(amount) & mask;
                    let signed = (left << (u64::BITS - width)) as i64 >> (u64::BITS - width);
                    let right = (signed >> ((amount + delta) & (width - 1))) as u64 & mask;
                    let expected = if shared { left ^ right } else { right };
                    let expected = if masked { expected & filter } else { expected };
                    assert_eq!(unsafe { run(value, filter) }, expected, "{ty} shift={amount} delta={delta} shared={shared} masked={masked} value={value}");
                }}
            }}}}
        }
    }

    #[test]
    fn masked_integer_conditions_preserve_width_polarity_and_other_uses() {
        let isa = super::super::jit_backend::target_isa().unwrap();
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let word_mask = u64::MAX >> (u64::BITS - ty.bits());
            let sign_bit = 1_u64 << (ty.bits() - 1);
            for mask in [0, 1, sign_bit, sign_bit | 1, word_mask, 0x55 & word_mask] {
                for condition in [None, Some(IntCC::Equal), Some(IntCC::NotEqual)] {
                    let mut signature = Signature::new(isa.default_call_conv());
                    signature.params.extend([AbiParam::new(types::I64); 3]);
                    signature.returns.push(AbiParam::new(types::I64));
                    let mut function = ir::Function::with_name_signature(UserFuncName::testcase("masked_condition"), signature);
                    let mut context = FunctionBuilderContext::new();
                    let mut builder = FunctionBuilder::new(&mut function, &mut context);
                    let entry = builder.create_block();
                    builder.append_block_params_for_function_params(entry);
                    builder.switch_to_block(entry);
                    let args = builder.block_params(entry).to_vec();
                    let source = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                    let masked = builder.ins().band_imm_u(source, mask as i64);
                    let predicate = match condition {
                        None => masked,
                        Some(condition) => builder.ins().icmp_imm_u(condition, masked, 0),
                    };
                    let selected = builder.ins().select(predicate, args[1], args[2]);
                    // Keep a second consumer: folding the predicate must not remove its value.
                    let masked = if ty == types::I64 { masked } else { builder.ins().uextend(types::I64, masked) };
                    let result = builder.ins().bxor(selected, masked);
                    builder.ins().return_(&[result]);
                    builder.seal_all_blocks();
                    builder.finalize(isa.frontend_config());
                    let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                    let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64, u64) -> u64>(code.pointer()) };
                    for source in (0..=255).chain([sign_bit, sign_bit - 1, word_mask, !word_mask, u64::MAX]) {
                        let masked = source & mask;
                        let choose_first = if condition == Some(IntCC::Equal) { masked == 0 } else { masked != 0 };
                        for (first, second) in [(0, u64::MAX), (sign_bit, 1)] {
                            let expected = (if choose_first { first } else { second }) ^ masked;
                            assert_eq!(unsafe { run(source, first, second) }, expected,
                                "{ty} source={source:x} mask={mask:x} condition={condition:?}");
                        }
                    }
                }
            }
        }
    }
