use super::*;

#[test]
fn rotated_countdown_preserves_empty_loops_and_early_exit_values() {
    let isa = super::super::jit_backend::target_isa().unwrap();
    for ty in [types::I8, types::I16, types::I32, types::I64] {
      for observe_counter in [false, true] {
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 3]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut function = ir::Function::with_name_signature(UserFuncName::testcase("rotated_counter"), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let body = builder.create_block();
            let advance = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, ty);
            builder.append_block_param(header, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let mut words = Vec::new();
            for value in args { words.push(if ty == types::I64 { value } else { builder.ins().ireduce(ty, value) }); }
            let zero = builder.ins().iconst(ty, 0);
            builder.ins().jump(header, &[words[0].into(), zero.into()]);
            builder.switch_to_block(header);
            let counter = builder.block_params(header)[0];
            let checksum = builder.block_params(header)[1];
            let more = builder.ins().icmp(IntCC::SignedLessThan, counter, words[1]);
            builder.ins().brif(more, body, &[], exit, &[]);
            builder.switch_to_block(body);
            let stopped = builder.ins().icmp(IntCC::Equal, if observe_counter { counter } else { checksum }, words[2]);
            builder.ins().brif(stopped, exit, &[], advance, &[]);
            builder.switch_to_block(advance);
            let updated = if observe_counter { builder.ins().bxor(checksum, counter) }
                else { builder.ins().iadd_imm_u(checksum, 1) };
            let next = builder.ins().iadd_imm_u(counter, 1);
            builder.ins().jump(header, &[next.into(), updated.into()]);
            builder.switch_to_block(exit);
            let result = builder.ins().iadd(counter, checksum);
            let result = if ty == types::I64 { result } else { builder.ins().uextend(types::I64, result) };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            if !observe_counter {
                let converted = super::super::jit_backend::compile_function(function.clone(), &*isa, None, |_, native| {
                    let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
                    let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
                    let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
                    loops.compute(native, &cfg, &dom);
                    Ok(native.layout.blocks().filter(|&block| loops.innermost_loop(block).is_some())
                        .flat_map(|block| native.layout.block_insts(block))
                        .any(|inst| native.dfg.insts[inst].opcode() == ir::Opcode::Isub))
                }).unwrap();
                assert!(converted, "{ty}: rotated loop must use shared countdown lowering");
            }
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64, u64) -> u64>(code.pointer()) };
            let sign_bit = 1_u64 << (ty.bits() - 1);
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            let signed = |value:u64| ((value << (u64::BITS - ty.bits())) as i64) >> (u64::BITS - ty.bits());
            for start in [0, 1, sign_bit - 1, sign_bit, sign_bit + 1, mask] {
                for bound in [0, 1, sign_bit - 1, sign_bit, mask] {
                    for distance in [0, 1, 17, mask.min(257)] {
                        let stop = if observe_counter { start.wrapping_add(distance) & mask } else { distance & mask };
                        let (mut counter, mut checksum) = (start, 0);
                        while signed(counter) < signed(bound) && (if observe_counter { counter } else { checksum }) != stop {
                            checksum = if observe_counter { checksum ^ counter } else { checksum.wrapping_add(1) & mask };
                            counter = counter.wrapping_add(1) & mask;
                        }
                        assert_eq!(unsafe { run(start, bound, stop) }, counter.wrapping_add(checksum) & mask,
                            "{ty}: start={start:x} bound={bound:x} stop={stop:x}");
                    }
                }
            }
          }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn countdown_induction_preserves_words_checkpoints_and_labels() {
        use cranelift_codegen::settings::Configurable;
        use std::sync::atomic::AtomicU8;
        const MAX_EXECUTED_TRIPS: u128 = 257;
        const PREVIOUS_LABEL: u32 = 0;
        const NEXT_LABEL: u32 = 1;
        const PREVIOUS_USE: u32 = 101;
        const NEXT_USE: u32 = 102;
        let mut flags = cranelift_codegen::settings::builder();
        flags.set("opt_level", "speed").unwrap();
        flags.set("enable_verifier", "true").unwrap();
        flags.set("machine_code_cfg_info", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        let mut executions = 0;
        let mut optimized = 0;
        for ty in [types::I8, types::I16, types::I32, types::I64] {
            let width = ty.bits();
            let mask = u64::MAX >> (u64::BITS - width);
            let min = if width == i64::BITS { i64::MIN } else { -(1_i64 << (width - 1)) };
            let max = if width == i64::BITS { i64::MAX } else { (1_i64 << (width - 1)) - 1 };
            let starts: Vec<_> = if ty == types::I8 { (min..=max).collect() }
                else { vec![min, min + 1, -7, -1, 0, 1, max - 17, max - 1] };
            for step in [1_i64, -1] { for &start in &starts {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.extend([AbiParam::new(types::I64); 3]);
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::testcase("countdown_words"), signature);
                function.collect_debug_info();
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let entry = builder.create_block();
                let header = builder.create_block();
                let comparison = builder.create_block();
                let backedge = builder.create_block();
                let empty = builder.create_block();
                let done = builder.create_block();
                let interrupted = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.append_block_param(header, ty);
                builder.switch_to_block(entry);
                builder.set_srcloc(ir::SourceLoc::new(0));
                let args = builder.block_params(entry).to_vec();
                let bound = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
                let initial = builder.ins().iconst(ty, start);
                let condition = if step > 0 { IntCC::SignedLessThan } else { IntCC::SignedGreaterThan };
                let enters = builder.ins().icmp(condition, initial, bound);
                builder.ins().brif(enters, header, &[initial.into()], empty, &[]);
                builder.switch_to_block(header);
                let previous = builder.block_params(header)[0];
                builder.set_val_label(previous, ir::ValueLabel::from_u32(PREVIOUS_LABEL));
                let next = builder.ins().iadd_imm_u(previous, step);
                builder.set_val_label(next, ir::ValueLabel::from_u32(NEXT_LABEL));
                let requested = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[1]);
                builder.ins().brif(requested, interrupted, &[], comparison, &[]);
                builder.switch_to_block(comparison);
                let continues = builder.ins().icmp(condition, next, bound);
                builder.ins().brif(continues, backedge, &[], done, &[]);
                builder.switch_to_block(backedge);
                builder.ins().jump(header, &[next.into()]);
                let widen = |builder: &mut FunctionBuilder<'_>, value| if ty == types::I64 { value }
                    else { builder.ins().uextend(types::I64, value) };
                for block in [interrupted, done] {
                    builder.switch_to_block(block);
                    builder.set_srcloc(ir::SourceLoc::new(PREVIOUS_USE));
                    let old = widen(&mut builder, previous);
                    builder.ins().store(MemFlagsData::trusted(), old, args[2], 0);
                    builder.set_srcloc(ir::SourceLoc::new(NEXT_USE));
                    let new = widen(&mut builder, next);
                    builder.ins().store(MemFlagsData::trusted(), new, args[2], std::mem::size_of::<u64>() as i32);
                    builder.set_srcloc(ir::SourceLoc::new(0));
                    builder.ins().return_(&[new]);
                }
                builder.switch_to_block(empty);
                let initial = widen(&mut builder, initial);
                builder.ins().store(MemFlagsData::trusted(), initial, args[2], 0);
                builder.ins().store(MemFlagsData::trusted(), initial, args[2], std::mem::size_of::<u64>() as i32);
                builder.ins().return_(&[initial]);
                builder.seal_all_blocks(); builder.finalize(isa.frontend_config());
                let mut compiler = cranelift_codegen::Context::for_function(function);
                compiler.compile(&*isa, &mut Default::default()).unwrap();
                let code = compiler.compiled_code().unwrap();
                let memory = CodeMemory::new(code.buffer.data()).unwrap();
                for (label, location) in [(PREVIOUS_LABEL, PREVIOUS_USE), (NEXT_LABEL, NEXT_USE)] {
                    let uses: Vec<_> = code.buffer.get_srclocs_sorted().iter().filter(|source|
                        source.loc == ir::MaybeRelSourceLoc::abs(ir::SourceLoc::new(location)) && source.start < source.end).collect();
                    if uses.is_empty() { continue; }
                    let ranges = code.value_labels_ranges.get(&ir::ValueLabel::from_u32(label)).unwrap_or_else(||
                        panic!("{ty} start{start} missing label{label}: {:?}\n{}", compiler.func.dfg.values_labels, compiler.func));
                    for source in uses {
                        assert!(ranges.iter().any(|range| range.start <= source.start && source.start < range.end),
                            "{ty} start{start} label{label} at{}: {ranges:?}", source.start);
                    }
                }
                let native = &compiler.func;
                let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
                let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
                let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
                loops.compute(native, &cfg, &dom);
                let hot: Vec<_> = native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some())
                    .flat_map(|block| native.layout.block_insts(block)).collect();
                let countdown = hot.iter().filter(|inst| native.dfg.insts[**inst].opcode() == ir::Opcode::Isub).count();
                if start == 0 || (step > 0 && start == min) { assert_eq!(countdown, 1, "{native}"); }
                if countdown != 0 {
                    assert_eq!(countdown, 1, "{native}");
                    assert!(!hot.iter().any(|inst| matches!(native.dfg.insts[*inst], ir::InstructionData::IntCompare {
                        cond: IntCC::SignedLessThan | IntCC::SignedGreaterThanOrEqual
                            | IntCC::SignedGreaterThan | IntCC::SignedLessThanOrEqual, .. })), "{native}");
                    optimized += 1;
                }
                // SAFETY: the verified ABI takes a word, a live atomic byte,
                // and two writable word cells, returning one word.
                let run = unsafe { std::mem::transmute::<*mut u8,
                    unsafe extern "C" fn(u64, *const AtomicU8, *mut u64) -> u64>(memory.pointer()) };
                let bounds: Vec<_> = if ty == types::I8 { (min..=max).collect() } else {
                    [min, start.saturating_sub(17), start.saturating_sub(2), start.saturating_sub(1), start, start.saturating_add(1), start.saturating_add(2),
                        start.saturating_add(17), max].into_iter().filter(|bound| *bound >= min && *bound <= max).collect()
                };
                for bound in bounds { for requested in [0_u8, 1] {
                    let distance = ((i128::from(bound) - i128::from(start)) * i128::from(step)).max(0) as u128;
                    let enters = distance != 0;
                    if requested == 0 && distance > MAX_EXECUTED_TRIPS { continue; }
                    let expected = if !enters { start } else if requested != 0 { start + step } else { bound };
                    let previous = if !enters || requested != 0 { start } else { bound - step };
                    let flag = AtomicU8::new(requested);
                    let mut cells = [0_u64; 2];
                    assert_eq!(unsafe { run(bound as u64 & mask, &flag, cells.as_mut_ptr()) }, expected as u64 & mask,
                        "{ty} start{start} bound{bound} step{step} request{requested}");
                    assert_eq!(cells, [previous as u64 & mask, expected as u64 & mask]);
                    executions += 1;
                }}
            }}
        }
        assert!(executions >= (1_usize << u8::BITS) * (1_usize << u8::BITS) * 4);
        assert!(optimized >= 3 * 4, "each width must optimize both directions from zero and increasing from signed minimum");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn loop_carried_updates_reuse_the_comparison_value() {
        use std::sync::atomic::AtomicU8;
        use cranelift_codegen::settings::Configurable;
        const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLCU");
        const BOUND_LABEL_INDEX: u32 = 0;
        let bound_label = ir::ValueLabel::from_u32(BOUND_LABEL_INDEX);
        const PROBE_SOURCE_OFFSET: u32 = 42;
        let probe_location = ir::SourceLoc::new(PROBE_SOURCE_OFFSET);
        let mut flags = cranelift_codegen::settings::builder();
        flags.set("opt_level", "speed").unwrap();
        flags.set("machine_code_cfg_info", "true").unwrap();
        let isa = cranelift_native::builder().unwrap().finish(cranelift_codegen::settings::Flags::new(flags)).unwrap();
        for ty in [types::I32, types::I64] { for step in [1_i64, -1, 7, -7] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 4]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
            function.collect_debug_info();
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let comparison = builder.create_block();
            let backedge = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, ty);
            builder.append_block_param(exit, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let narrow = |builder: &mut FunctionBuilder<'_>, value| if ty == types::I32 {
                builder.ins().ireduce(ty, value)
            } else { value };
            let start = narrow(&mut builder, args[0]);
            let bound = narrow(&mut builder, args[1]);
            builder.set_srcloc(ir::SourceLoc::new(0));
            builder.set_val_label(bound, bound_label);
            let empty = builder.ins().icmp(IntCC::Equal, start, bound);
            builder.ins().brif(empty, exit, &[start.into()], header, &[start.into()]);
            builder.switch_to_block(header);
            let previous = builder.block_params(header)[0];
            let next = builder.ins().iadd_imm_u(previous, step);
            let stop = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[2]);
            builder.ins().brif(stop, exit, &[next.into()], comparison, &[]);
            builder.switch_to_block(comparison);
            let done = builder.ins().icmp(IntCC::Equal, next, bound);
            builder.ins().brif(done, exit, &[next.into()], backedge, &[]);
            builder.switch_to_block(backedge);
            builder.set_srcloc(probe_location);
            let observed = if ty == types::I32 { builder.ins().uextend(types::I64, bound) } else { bound };
            builder.ins().store(MemFlagsData::trusted(), observed, args[3], 0);
            builder.set_srcloc(ir::SourceLoc::new(0));
            builder.ins().jump(header, &[next.into()]);
            builder.switch_to_block(exit);
            let result = builder.block_params(exit)[0];
            let result = if ty == types::I32 { builder.ins().uextend(types::I64, result) } else { result };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let mut compiler = cranelift_codegen::Context::for_function(function);
            compiler.set_disasm(true);
            let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
            let locations = compiled.value_labels_ranges.get(&bound_label).expect("live bound label");
            assert!(!locations.is_empty());
            assert!(locations.windows(2).all(|pair| pair[0].end <= pair[1].start));
            let probes: Vec<_> = compiled.buffer.get_srclocs_sorted().iter()
                .filter(|source| source.loc == ir::MaybeRelSourceLoc::abs(probe_location) && source.start < source.end).collect();
            assert!(!probes.is_empty(), "the relocated backedge must retain its probe");
            for probe in probes {
                assert!(locations.iter().any(|range| range.start <= probe.start && probe.end <= range.end),
                    "bound debug location must cover its relocated use: {}..{}, {locations:?}", probe.start, probe.end);
            }
            assert!(compiled.bb_starts.windows(2).all(|pair| pair[0] < pair[1]));
            #[cfg(target_arch = "aarch64")]
            assert_aarch64_cfg_edges(compiled);
            let memory = CodeMemory::new(compiled.buffer.data()).unwrap();
            let native = &compiler.func;
            let cfg = cranelift_codegen::flowgraph::ControlFlowGraph::with_function(native);
            let dom = cranelift_codegen::dominator_tree::DominatorTree::with_function(native, &cfg);
            let mut loops = cranelift_codegen::loop_analysis::LoopAnalysis::new();
            loops.compute(native, &cfg, &dom);
            assert_eq!(loops.loops().count(), 1);
            let updates = native.layout.blocks().filter(|block| loops.innermost_loop(*block).is_some())
                .flat_map(|block| native.layout.block_insts(block))
                .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Iadd).count();
            assert_eq!(updates, 1, "one update serves comparison and backedge: {ty} step{step}\n{native}");
            // SAFETY: the verified function has four word inputs and one word
            // output; the pointers name a live atomic byte and writable probe.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64, *const AtomicU8, *mut u64) -> u64>(memory.pointer()) };
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            for seed in [0_u64, 1, u64::MAX, i32::MAX as u64, i64::MAX as u64] {
                for trips in [0_u64, 1, 2, 17, 257] { for requested in [0_u8, 1] {
                    let seed = seed & mask;
                    let bound = seed.wrapping_add(trips.wrapping_mul(step as u64)) & mask;
                    let flag = AtomicU8::new(requested);
                    let expected = if trips == 0 { seed } else if requested != 0 {
                        seed.wrapping_add(step as u64) & mask
                    } else { bound };
                    let mut probe = 0;
                    assert_eq!(unsafe { run(seed, bound, &flag, &mut probe) }, expected,
                        "{ty} step{step} seed{seed} trips{trips} stop{requested}");
                    assert_eq!(probe, if trips > 1 && requested == 0 { bound } else { 0 });
                }}
            }
        }}
    }

    #[cfg(feature = "aot")]
    #[test]
    fn loop_carried_updates_share_branch_store_and_backedge_placement() {
        use std::sync::atomic::AtomicU8;
        const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLSP");
        const FEEDBACK: u64 = 0xD008;
        const STORED_MASK: u64 = 0x55;
        const UPDATE_LABEL: u32 = 0;
        const STORE_SOURCE_OFFSET: u32 = 42;
        let update_label = ir::ValueLabel::from_u32(UPDATE_LABEL);
        let store_location = ir::SourceLoc::new(STORE_SOURCE_OFFSET);
        let isa = super::super::jit_backend::target_isa().unwrap();
        for ty in [types::I32, types::I64] {
            let mut signature = ir::Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 5]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
            function.collect_debug_info();
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let entry = builder.create_block();
            let header = builder.create_block();
            let body = builder.create_block();
            let odd = builder.create_block();
            let even = builder.create_block();
            let join = builder.create_block();
            let left = builder.create_block();
            let right = builder.create_block();
            let latch = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.append_block_param(header, ty);
            builder.append_block_param(header, types::I64);
            builder.append_block_param(join, ty);
            builder.append_block_param(join, ty);
            builder.append_block_param(exit, ty);
            builder.switch_to_block(entry);
            let args = builder.block_params(entry).to_vec();
            let seed = if ty == types::I32 { builder.ins().ireduce(ty, args[0]) } else { args[0] };
            builder.ins().brif(args[1], header, &[seed.into(), args[1].into()], exit, &[seed.into()]);
            builder.switch_to_block(header);
            let previous = builder.block_params(header)[0];
            let count = builder.block_params(header)[1];
            let stop = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[4]);
            builder.ins().brif(stop, exit, &[previous.into()], body, &[]);
            builder.switch_to_block(body);
            let shifted = builder.ins().ushr_imm_u(previous, 1);
            let low = builder.ins().band_imm_u(previous, 1);
            builder.ins().brif(low, odd, &[], even, &[]);
            builder.switch_to_block(odd);
            let feedback = builder.ins().iconst(ty, FEEDBACK as i64);
            builder.ins().jump(join, &[shifted.into(), feedback.into()]);
            builder.switch_to_block(even);
            let zero = builder.ins().iconst(ty, 0);
            builder.ins().jump(join, &[shifted.into(), zero.into()]);
            builder.switch_to_block(join);
            let operands = [builder.block_params(join)[0], builder.block_params(join)[1]];
            let next = builder.ins().bxor(operands[0], operands[1]);
            builder.set_val_label(next, update_label);
            let wide = if ty == types::I32 { builder.ins().uextend(types::I64, next) } else { next };
            builder.ins().brif(args[2], left, &[], right, &[]);
            builder.switch_to_block(left);
            builder.set_srcloc(store_location);
            builder.ins().store(MemFlagsData::trusted(), wide, args[3], 0);
            builder.ins().jump(latch, &[]);
            builder.switch_to_block(right);
            let encoded = builder.ins().bxor_imm_u(wide, STORED_MASK as i64);
            builder.ins().store(MemFlagsData::trusted(), encoded, args[3], core::mem::size_of::<u64>() as i32);
            builder.ins().jump(latch, &[]);
            builder.switch_to_block(latch);
            builder.set_srcloc(ir::SourceLoc::new(0));
            let remaining = builder.ins().iadd_imm_s(count, -1);
            builder.ins().brif(remaining, header, &[next.into(), remaining.into()], exit, &[next.into()]);
            builder.switch_to_block(exit);
            let result = builder.block_params(exit)[0];
            let result = if ty == types::I32 { builder.ins().uextend(types::I64, result) } else { result };
            builder.ins().return_(&[result]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let mut compiler = cranelift_codegen::Context::for_function(function);
            let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
            let locations = compiled.value_labels_ranges.get(&update_label).expect("live update label");
            let probes: Vec<_> = compiled.buffer.get_srclocs_sorted().iter()
                .filter(|source| source.loc == ir::MaybeRelSourceLoc::abs(store_location) && source.start < source.end).collect();
            assert!(!probes.is_empty());
            assert!(probes.iter().all(|probe| locations.iter().any(|range|
                range.start <= probe.start && probe.end <= range.end)), "update label covers both storage arms");
            let memory = CodeMemory::new(compiled.buffer.data()).unwrap();
            let native = &compiler.func;
            let shifts = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
                .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Ushr).count();
            assert_eq!(shifts, 1, "one producer serves both stores and the next iteration: {ty}\n{native}");
            // SAFETY: the verified ABI takes three words and live pointers to
            // two writable words and an atomic stop byte, returning a word.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64, u64, *mut u64, *const AtomicU8) -> u64>(memory.pointer()) };
            let mask = u64::MAX >> (u64::BITS - ty.bits());
            for seed in [0, 1, 123456789, u64::MAX, i64::MIN as u64] {
                for trips in [0, 1, 2, 17, 257] { for arm in [0, 1] { for requested in [0, 1] {
                    let seed = seed & mask;
                    let flag = AtomicU8::new(requested);
                    let mut cells = [0_u64; 2];
                    let expected = (0..if requested == 0 { trips } else { 0 }).fold(seed, |value, _|
                        (value >> 1) ^ if value & 1 != 0 { FEEDBACK } else { 0 });
                    assert_eq!(unsafe { run(seed, trips, arm, cells.as_mut_ptr(), &flag) }, expected);
                    let expected_cells = if requested != 0 || trips == 0 { [0, 0] }
                        else if arm != 0 { [expected, 0] } else { [0, expected ^ STORED_MASK] };
                    assert_eq!(cells, expected_cells, "{ty} seed{seed} trips{trips} arm{arm} stop{requested}");
                }}}
            }
        }
    }
