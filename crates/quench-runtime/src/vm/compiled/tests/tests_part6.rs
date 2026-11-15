use super::*;

#[cfg(feature = "aot")]
#[test]
fn linked_native_terminal_calls_preserve_constructor_results_and_order() {
    for typescript in [false, true] {
        let (native, counts, _) = linked_test_program(
            "let trace = ''; function add(x) { trace += 'a'; return x + 1; } function wrap(x) { trace += 'w'; return add(x); } function Box(x) { trace += 'b'; this.value = wrap(x); } let box = new Box(41); if (Object.getPrototypeOf(box) !== Box.prototype || box.value !== 42 || trace !== 'bwa') throw 'bad construction'; throw box.value;",
            typescript,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected result throw") };
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)));
        assert!(counts.iter().all(|count| count.get() > 0));
    }
}

#[cfg(feature = "aot")]
#[test]
fn linked_native_async_frame_resumes_after_await_and_finishes_jobs() {
    let (native, counts, _) = linked_test_program(
        "globalThis.answer = 0; async function work() { try { globalThis.answer = await Promise.resolve(7); } finally { globalThis.finished = true; } } work();",
        false,
    );
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    vm.execute_native_program(&native, true).unwrap();
    for (name, expected) in [("answer", Value::number(7.0)), ("finished", Value::TRUE)] {
            let atom = vm.lookup_atom(name).unwrap();
            assert_eq!(vm.own_property(vm.realm.globals, atom), Some(expected));
        }
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_typed_call_preserves_an_asserted_strings_actual_value() {
        let (native, counts, _) = linked_test_program(
            "function add(x: number): number { return x + 1; } throw add('x' as unknown as number);", true,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected throw") };
        assert_eq!(vm.heap.string(error.thrown_value().unwrap()).unwrap().to_string(), "x1");
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_generator_preserves_parameter_errors_before_body_entry() {
        let (native, counts, _) = linked_test_program(
            "function* values({x}) { throw 'body ran'; } let caught = false; try { values(null); } catch (e) { caught = e instanceof TypeError; } if (!caught) throw 'missing parameter error'; throw 'parameter';", false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected throw") };
        assert_eq!(vm.heap.string(error.thrown_value().unwrap()).unwrap().to_string(), "parameter");
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_callbacks_collect_and_preserve_object_identity() {
        for typescript in [false, true] {
            let (native, counts, _) = linked_test_program(
                "const keep = {id: 7}; const marker = {}; function callback(x) { $262.gc(); if (x === marker) throw marker; return x; } function outer() { for (let n = 0; n < 16; n++) { try { [marker].map(callback); throw 'missing callback throw'; } catch (error) { if (error !== marker) throw error; } const values = [keep, keep].map(callback); if (values[0] !== keep || values[1] !== keep) throw 'identity changed'; } return keep.id; } throw outer();",
                typescript,
            );
            let mut vm = Vm::new(crate::test_host::TestHost::default().with_test262());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::number(7.0)));
            assert!(counts.iter().all(|count| count.get() > 0));
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_capture_cache_preserves_tdz_mutation_and_collection() {
        let (native, counts, _) = linked_test_program(
            "function outer() { const read = () => { let result; for (let i = 0; i < 32; i++) { result = value; if (i === 7) $262.gc(); } return result; }; let caught = false; try { read(); } catch (e) { caught = e instanceof ReferenceError; } if (!caught) throw 'tdz'; let value = {n: 1}; if (read() !== value) throw 'identity'; value = {n: 2}; return read().n; } throw outer();",
            false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default().with_test262());
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(2.0)));
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_code_owner_survives_the_program_handle_until_vm_reset() {
        let (native, counts, weak) = linked_test_program("globalThis.saved = () => 42;", false);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.execute_native_program(&native, true).unwrap();
        drop(native);
        assert!(weak.upgrade().is_some());
        let atom = vm.lookup_atom("saved").unwrap();
        let saved = vm.own_property(vm.realm.globals, atom).unwrap();
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        assert_eq!(vm.call_value(&program, saved, Value::UNDEFINED, &[]).unwrap(), Value::number(42.0));
        assert!(counts.iter().all(|count| count.get() > 0));
        let replacement = crate::Engine::specialize("0;", "replacement.js").unwrap();
        vm.execute(&replacement).unwrap();
        assert!(weak.upgrade().is_none(), "reset releases code with the old program");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_deferred_jobs_keep_their_code_until_the_host_checkpoint() {
        let (native, counts, weak) = linked_test_program(
            "globalThis.answer = 0; Promise.resolve().then(() => { globalThis.answer = 42; });", false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.execute_native_program(&native, false).unwrap();
        drop(native);
        assert!(weak.upgrade().is_some());
        let atom = vm.lookup_atom("answer").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(0.0)));
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        vm.drain_jobs(&program).unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(42.0)));
        assert!(counts.iter().all(|count| count.get() > 0));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_semantic_graph_rejects_full_width_invalid_resume_pc_before_effects() {
        let mut vm = semantic_vm("globalThis.effect = 1;");
        let (_memory, entry, _operands) = live_native_graph(&vm);
        let (_prepared, partitioned) = partitioned_native_graph(&vm);
        let invalid = (u64::from(u32::MAX) + 1) as usize;
        for entry in [entry, partitioned] {
            let (status, word, outcome) = with_semantic_context(&mut vm, invalid, |context, result| {
                // SAFETY: as above; the out-of-range PC is data, not a pointer.
                unsafe { i64::from(entry(context.cast(), 0, 0, std::ptr::null(), result)) }
            });
            assert_eq!(status, i64::from(NATIVE_JS_INVALID_PC));
            assert_eq!(word, invalid as u64);
            assert!(outcome.is_none());
        }
        let effect = vm.lookup_atom("effect").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, effect), None);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_semantic_graph_does_not_trust_typescript_assertions() {
        let program = crate::Engine::specialize_typescript(
            "(globalThis as any).input = 'x' as unknown as number; throw (globalThis as any).input + 1;",
            "native-assertion.cts",
        ).unwrap();
        let mut vm = semantic_program(program);
        let (_memory, entry, _operands) = live_native_graph(&vm);
        let (_prepared, partitioned) = partitioned_native_graph(&vm);
        for entry in [entry, partitioned] {
            let (status, _, outcome) = with_semantic_context(&mut vm, 0, |context, result| {
                // SAFETY: the rooted frame, entries and operands live through the call.
                unsafe { i64::from(entry(context.cast(), 0, 0, std::ptr::null(), result)) }
            });
            assert_eq!(status, i64::from(NATIVE_JS_STOPPED));
            let Err(error) = outcome.expect("graph stops at throw").result else { panic!("expected throw") };
            let value = error.thrown_value().expect("original thrown value");
            assert_eq!(vm.heap.string(value).expect("JS string concatenation").to_string(), "x1");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_executes_one_store_and_returns_to_native_control() {
        let mut vm = semantic_vm("globalThis.x = 1; globalThis.y = 2;");
        let (pc, instruction) = semantic_instruction(&vm, Op::SetField);
        vm.frames[0].registers[usize::from(instruction.register_b())] = vm.realm.globals;
        let value = Value::number(1.0);
        vm.frames[0].registers[usize::from(instruction.register_a())] = value;
        let (status, next, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::SetField as usize }>);
        assert_eq!(status, 0);
        assert_eq!(next, (pc + 1) as u64);
        assert!(outcome.is_none());
        assert_eq!(vm.own_property(vm.realm.globals, instruction.atom_index()), Some(value));
        let y = vm.lookup_atom("y").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, y), None);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_rejects_mismatched_or_invalid_opcodes_before_effects() {
        let mut vm = semantic_vm("globalThis.x = 1;");
        let (pc, _) = semantic_instruction(&vm, Op::SetField);
        let before = vm.frames[0].registers.clone();
        for entry in [
            native_js_operation_entry::<crate::test_host::TestHost, { Op::LoadConst as usize }>,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::COUNT }>,
            native_js_operation_entry::<crate::test_host::TestHost, { usize::MAX }>,
        ] {
            let (status, _, outcome) = invoke_semantic(&mut vm, pc, entry);
            assert_eq!(status, 1);
            assert!(outcome.unwrap().result.is_err());
            assert_eq!(vm.frames[0].registers, before);
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_admits_wide_opcodes_without_decoding_source_operands() {
        let mut program = crate::Engine::specialize("throw 7;", "wide-semantic-entry.js").unwrap();
        let body = &mut program.functions[0];
        let pc = body.instructions().position(|instruction| instruction.op().is_throw()).unwrap();
        let instruction = super::super::dispatch_fast::decoded_instruction(body, body.code[pc]);
        let wide_index = body.wide.len();
        body.wide.push(instruction);
        body.code[pc] = crate::bytecode::Instr::wide(wide_index).unwrap();
        let mut vm = semantic_program(program);
        vm.frames[0].registers[usize::from(instruction.register_a())] = Value::number(7.0);
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::SetField as usize }>);
        assert_eq!(status, 1);
        assert!(outcome.unwrap().result.is_err());
        assert_eq!(vm.frames[0].registers[usize::from(instruction.register_a())], Value::number(7.0));
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::Throw as usize }>);
        assert_eq!(status, 1);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.instruction_pc, pc);
        let Err(error) = outcome.result else { panic!("wide Throw must stop with its original value") };
        assert_eq!(error.thrown_value(), Some(Value::number(7.0)));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_preserves_thrown_value_and_exact_pc() {
        let mut vm = semantic_vm("throw 7;");
        let (pc, instruction) = semantic_instruction(&vm, Op::Throw);
        let value = Value::number(7.0);
        vm.frames[0].registers[usize::from(instruction.register_a())] = value;
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::Throw as usize }>);
        assert_eq!(status, 1);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.instruction_pc, pc);
        assert_eq!(outcome.next_pc, pc + 1);
        let Err(error) = outcome.result else { panic!("semantic throw must be abrupt") };
        assert_eq!(error.thrown_value(), Some(value));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn static_semantic_entry_checks_termination_before_guest_effects() {
        let mut vm = semantic_vm("globalThis.x = 1;");
        let (pc, instruction) = semantic_instruction(&vm, Op::SetField);
        vm.frames[0].registers[usize::from(instruction.register_b())] = vm.realm.globals;
        vm.frames[0].registers[usize::from(instruction.register_a())] = Value::number(1.0);
        vm.execution_interrupt.request_termination();
        let (status, _, outcome) = invoke_semantic(&mut vm, pc,
            native_js_operation_entry::<crate::test_host::TestHost, { Op::SetField as usize }>);
        assert_eq!(status, 1);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.instruction_pc, pc);
        let Err(error) = outcome.result else { panic!("terminated entry must stop") };
        assert!(error.is_execution_terminated());
        assert_eq!(vm.own_property(vm.realm.globals, instruction.atom_index()), None);
    }

    #[test]
    fn per_import_callout_counts_use_function_indices_and_escape_json_names() {
        let names = vec![
            (0, 2, "env".to_owned(), "copy\"fast".to_owned()),
            (1, 5, "host\\api".to_owned(), "line\nfeed".to_owned()),
        ];
        let counters = [std::cell::Cell::new(0), std::cell::Cell::new(0)];
        // SAFETY: the pointer covers both initialized counters for their
        // entire use below.
        unsafe {
            note_import_callout_target(counters.as_ptr(), counters.len(), 1);
            note_import_callout_target(counters.as_ptr(), counters.len(), 1);
            note_import_callout_target(counters.as_ptr(), counters.len(), 99);
        }
        assert_eq!(counters[0].get(), 0);
        assert_eq!(counters[1].get(), 2);
        assert_eq!(
            format_import_callout_targets(&names, &counters),
            "{\"function\":1,\"import_index\":5,\"module\":\"host\\\\api\",\"name\":\"line\\nfeed\",\"count\":2}"
        );
        assert_eq!(json_string("\u{1}\t"), "\"\\u0001\\t\"");
    }

    #[test]
    #[cfg(all(target_arch = "x86_64", unix))]
    fn native_checked_signed_multiply_preserves_product_and_overflow() {
        let isa = super::super::jit_backend::target_isa().unwrap();
        for ty in [types::I16, types::I32, types::I64] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 3]);
            let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut function, &mut context);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);
            let args = builder.block_params(block).to_vec();
            let left = if ty == types::I64 { args[0] } else { builder.ins().ireduce(ty, args[0]) };
            let right = if ty == types::I64 { args[1] } else { builder.ins().ireduce(ty, args[1]) };
            let (product, overflow) = builder.ins().smul_overflow(left, right);
            let product = if ty == types::I64 { product } else { builder.ins().sextend(types::I64, product) };
            let overflow = builder.ins().uextend(types::I64, overflow);
            builder.ins().store(MemFlagsData::trusted(), product, args[2], 0);
            builder.ins().store(MemFlagsData::trusted(), overflow, args[2], std::mem::size_of::<i64>() as i32);
            builder.ins().return_(&[]);
            builder.finalize(isa.frontend_config());
            let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: the generated signature is (i64, i64, pointer) -> void; its only
            // memory accesses write the two i64 slots supplied below. Code stays live.
            let multiply = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(i64, i64, *mut i64)>(code.pointer()) };
            let bits = ty.bits();
            let min = -(1i128 << (bits - 1));
            let max = (1i128 << (bits - 1)) - 1;
            let cases = [min, min + 1, -46341, -2, -1, 0, 1, 2, 46341, max - 1, max];
            for left in cases.into_iter().filter(|value| (min..=max).contains(value)) {
                for right in cases.into_iter().filter(|value| (min..=max).contains(value)) {
                    let exact = left * right;
                    let wrapped = (exact << (128 - bits)) >> (128 - bits);
                    let mut actual = [0i64; 2];
                    unsafe { multiply(left as i64, right as i64, actual.as_mut_ptr()) };
                    assert_eq!(actual, [wrapped as i64, i64::from(!(min..=max).contains(&exact))], "{ty}: {left} * {right}");
                }
            }
        }
    }

    #[test]
    #[cfg(all(target_arch = "x86_64", unix))]
    fn native_vector_rotations_preserve_near_matches_and_unaligned_accesses() {
        #[derive(Clone, Copy, Debug)]
        enum Shape { Rotate, DistinctInputs, SignedRight, NonComplementaryShift }
        let isa = super::super::jit_backend::target_isa().unwrap();
        // A baseline x86-64 target has no optional SSSE3: it must retain
        // the existing shifts/OR lowering while producing the same values.
        let baseline = cranelift_codegen::isa::lookup(isa.triple().clone())
            .unwrap().finish(isa.flags().clone()).unwrap();
        for isa in [isa, baseline] {
        let cases = [0, 1, 2, u32::MAX as u64, 1 << 32, 1 << 63, i64::MAX as u64, u64::MAX, 0x0123_4567_89ab_cdef, 0xaaaa_5555_aaaa_5555];
        for rotate in 0..u64::BITS {
            for reversed in [false, true] {
                for shape in [Shape::Rotate, Shape::DistinctInputs, Shape::SignedRight, Shape::NonComplementaryShift] {
                    let mut signature = Signature::new(isa.default_call_conv());
                    signature.params.extend([AbiParam::new(types::I64); 3]);
                    let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
                    let mut context = FunctionBuilderContext::new();
                    let mut builder = FunctionBuilder::new(&mut function, &mut context);
                    let block = builder.create_block();
                    builder.append_block_params_for_function_params(block);
                    builder.switch_to_block(block);
                    builder.seal_block(block);
                    let args = builder.block_params(block).to_vec();
                    let memory = MemFlagsData::new().with_notrap();
                    let cast = MemFlagsData::new().with_endianness(ir::Endianness::Little);
                    let first = builder.ins().load(types::I8X16, memory, args[0], 0);
                    let first = builder.ins().bitcast(types::I64X2, cast, first);
                    let second = if matches!(shape, Shape::DistinctInputs) {
                        let value = builder.ins().load(types::I8X16, memory, args[1], 0);
                        builder.ins().bitcast(types::I64X2, cast, value)
                    } else { first };
                    let left_amount = u64::BITS - rotate - u32::from(matches!(shape, Shape::NonComplementaryShift));
                    let left_count = builder.ins().iconst(types::I32, i64::from(left_amount));
                    let right_count = builder.ins().iconst(types::I32, i64::from(rotate));
                    let left = builder.ins().ishl(first, left_count);
                    let right = if matches!(shape, Shape::SignedRight) {
                        builder.ins().sshr(second, right_count)
                    } else { builder.ins().ushr(second, right_count) };
                    let left = builder.ins().bitcast(types::I8X16, cast, left);
                    let right = builder.ins().bitcast(types::I8X16, cast, right);
                    let value = if reversed { builder.ins().bor(right, left) } else { builder.ins().bor(left, right) };
                    builder.ins().store(memory, value, args[2], 0);
                    builder.ins().return_(&[]);
                    builder.finalize(isa.frontend_config());
                    let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                    // SAFETY: generated code accesses exactly sixteen bytes at
                    // each pointer, promises no alignment, and stays live here.
                    let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(*const u8, *const u8, *mut u8)>(code.pointer()) };
                    for offset in [0, 1, 7] {
                        for (index, input) in cases.into_iter().enumerate() {
                            let left = [input, cases[(index + 3) % cases.len()]];
                            let right = if matches!(shape, Shape::DistinctInputs) { [!left[1], !left[0]] } else { left };
                            let mut first = [0u8; 32];
                            let mut second = [0u8; 32];
                            let mut output = [0xa5u8; 32];
                            for lane in 0..2 {
                                let start = offset + lane * std::mem::size_of::<u64>();
                                first[start..start + 8].copy_from_slice(&left[lane].to_le_bytes());
                                second[start..start + 8].copy_from_slice(&right[lane].to_le_bytes());
                            }
                            unsafe { run(first.as_ptr().add(offset), second.as_ptr().add(offset), output.as_mut_ptr().add(offset)) };
                            for lane in 0..2 {
                                let right = if matches!(shape, Shape::SignedRight) { (right[lane] as i64).wrapping_shr(rotate) as u64 } else { right[lane].wrapping_shr(rotate) };
                                let expected = left[lane].wrapping_shl(left_amount) | right;
                                let start = offset + lane * std::mem::size_of::<u64>();
                                assert_eq!(&output[start..start + 8], &expected.to_le_bytes(), "{shape:?}, rotate={rotate}, reversed={reversed}, offset={offset}");
                            }
                            assert!(output[..offset].iter().chain(output[offset + 16..].iter()).all(|byte| *byte == 0xa5));
                        }
                    }
                }
            }
        }
        }
    }

    #[test]
    fn native_alias_regions_preserve_overlapping_untagged_accesses() {
        let isa = super::super::jit_backend::target_isa().unwrap();
        for tagged_load in [false, true] {
            for tagged_store in [false, true] {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.push(AbiParam::new(types::I64));
                signature.returns.push(AbiParam::new(types::I64));
                let mut function = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
                let region = function.dfg.alias_regions.insert(ir::AliasRegionData {
                    user_id: 0,
                    description: "overlapping test accesses".into(),
                });
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut function, &mut context);
                let block = builder.create_block();
                builder.append_block_params_for_function_params(block);
                builder.switch_to_block(block);
                builder.seal_block(block);
                let address = builder.block_params(block)[0];
                let load_flags = MemFlagsData::trusted().with_alias_region(tagged_load.then_some(region));
                let store_flags = MemFlagsData::trusted().with_alias_region(tagged_store.then_some(region));
                let first = builder.ins().load(types::I64, load_flags, address, 0);
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().store(store_flags, zero, address, 0);
                let second = builder.ins().load(types::I64, load_flags, address, 0);
                let result = builder.ins().iadd(first, second);
                builder.ins().return_(&[result]);
                builder.finalize(isa.frontend_config());
                let code = compile_native_function(function, isa.as_ref(), usize::MAX, CodeMemory::new).unwrap();
                // SAFETY: the generated function reads and writes one live u64,
                // and its executable mapping remains alive throughout each call.
                let run = unsafe {
                    std::mem::transmute::<*mut u8, unsafe extern "C" fn(*mut u64) -> u64>(code.pointer())
                };
                for input in [0, 1, 42, i64::MAX as u64, u64::MAX] {
                    let mut memory = input;
                    assert_eq!(unsafe { run(&mut memory) }, input, "load tagged={tagged_load}, store tagged={tagged_store}");
                    assert_eq!(memory, 0);
                }
            }
        }
    }
