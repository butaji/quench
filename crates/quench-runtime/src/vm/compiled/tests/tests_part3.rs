use super::*;

#[cfg(feature = "aot")]
#[test]
fn loop_constants_share_conditional_entries_and_preserve_exact_exits() {
    use std::sync::atomic::AtomicU8;
    const TEST_NAMESPACE: u32 = u32::from_be_bytes(*b"QLCP");
    const FEEDBACK: u64 = 0xD008;
    const UNTOUCHED: u64 = 0x55;
    let isa = super::super::jit_backend::target_isa().unwrap();
    for ty in [types::I32, types::I64] {
        let mut signature = ir::Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 5]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut function = ir::Function::with_name_signature(UserFuncName::user(TEST_NAMESPACE, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block();
        let choose = builder.create_block();
        let headers = [builder.create_block(), builder.create_block()];
        let bodies = [builder.create_block(), builder.create_block()];
        let exit = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        for header in headers {
            builder.append_block_param(header, ty);
            builder.append_block_param(header, types::I64);
        }
        builder.append_block_param(exit, ty);
        builder.switch_to_block(entry);
        let args = builder.block_params(entry).to_vec();
        let seed = if ty == types::I32 { builder.ins().ireduce(ty, args[0]) } else { args[0] };
        let empty = builder.ins().icmp_imm_s(IntCC::SignedLessThanOrEqual, args[1], 0);
        builder.ins().brif(empty, exit, &[seed.into()], choose, &[]);
        builder.switch_to_block(choose);
        builder.ins().brif(args[2], headers[0], &[seed.into(), args[1].into()], headers[1], &[seed.into(), args[1].into()]);
        for arm in 0..headers.len() {
            builder.switch_to_block(headers[arm]);
            let previous = builder.block_params(headers[arm])[0];
            let count = builder.block_params(headers[arm])[1];
            let stop = builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), args[3]);
            builder.ins().brif(stop, exit, &[previous.into()], bodies[arm], &[]);
            builder.switch_to_block(bodies[arm]);
            let feedback = builder.ins().iconst(ty, FEEDBACK as i64);
            let next = if arm == 0 { builder.ins().bxor(previous, feedback) }
                else { builder.ins().iadd(previous, feedback) };
            let observed = if ty == types::I32 { builder.ins().uextend(types::I64, next) } else { next };
            builder.ins().store(MemFlagsData::trusted(), observed, args[4], 0);
            let remaining = builder.ins().iadd_imm_s(count, -1);
            builder.ins().brif(remaining, headers[arm], &[next.into(), remaining.into()], exit, &[next.into()]);
        }
        builder.switch_to_block(exit);
        let result = builder.block_params(exit)[0];
        let result = if ty == types::I32 { builder.ins().uextend(types::I64, result) } else { result };
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let mut compiler = cranelift_codegen::Context::for_function(function);
        let compiled = compiler.compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default()).unwrap();
        let memory = CodeMemory::new(compiled.buffer.data()).unwrap();
        cranelift_codegen::verify_function(&compiler.func, &*isa).unwrap();
        compiler.flowgraph();
        compiler.compute_loop_analysis();
        let native = &compiler.func;
        assert_eq!(compiler.loop_analysis.loops().count(), 2, "distinct recurrences retain both versions: {native}");
        let live_literals: Vec<_> = native.layout.blocks().flat_map(|block| native.layout.block_insts(block).map(move |inst| (block, inst)))
            .filter_map(|(block, inst)| {
                if !matches!(native.dfg.insts[inst], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm }
                    if imm.bits() as u64 == FEEDBACK) { return None; }
                let value = native.dfg.first_result(inst);
                let uses: Vec<_> = native.layout.blocks().flat_map(|block| native.layout.block_insts(block).map(move |inst| (block, inst)))
                    .filter(|(_, user)| native.dfg.inst_values(*user).any(|arg| native.dfg.resolve_aliases(arg) == value))
                    .map(|(block, _)| block).collect();
                (!uses.is_empty()).then_some((block, uses))
            }).collect();
        assert_eq!(live_literals.len(), 1, "one literal serves both loop versions: {native}");
        let (parent, uses) = &live_literals[0];
        assert!(compiler.loop_analysis.innermost_loop(*parent).is_none());
        for lp in compiler.loop_analysis.loops() {
            let header = compiler.loop_analysis.loop_header(lp);
            assert_eq!(compiler.domtree.idom(header), Some(*parent));
            assert!(uses.iter().any(|block| compiler.loop_analysis.is_in_loop(*block, lp)), "shared constant reaches {lp}");
        }
        // SAFETY: the verified ABI receives three scalar words and live
        // pointers to an atomic byte and writable word; it returns one word.
        let run = unsafe { std::mem::transmute::<*mut u8,
            unsafe extern "C" fn(u64, i64, u64, *const AtomicU8, *mut u64) -> u64>(memory.pointer()) };
        let mask = u64::MAX >> (u64::BITS - ty.bits());
        for seed in [0_u64, 1, i32::MAX as u64, i64::MAX as u64, u64::MAX] {
            for trips in [-1_i64, 0, 1, 2, 17, 257] { for arm in [0, 1] { for requested in [0, 1] {
                let flag = AtomicU8::new(requested);
                let mut observed = UNTOUCHED;
                let actual_trips = if requested == 0 { trips.max(0) } else { 0 };
                let expected = (0..actual_trips).fold(seed & mask, |value, _|
                    (if arm != 0 { value ^ FEEDBACK } else { value.wrapping_add(FEEDBACK) }) & mask);
                assert_eq!(unsafe { run(seed, trips, arm, &flag, &mut observed) }, expected,
                    "{ty}, seed={seed}, trips={trips}, arm={arm}, stop={requested}");
                assert_eq!(observed, if actual_trips == 0 { UNTOUCHED } else { expected });
            } } }
        }
    }
}

#[cfg(feature = "aot")]
#[test]
fn slot_widening_unlinks_compiled_readers_without_releasing_active_code() {
    let program = crate::Engine::specialize(
        "var receiver={edgeValue:1}; function read(){var n=0;for(var i=0;i<5;i++)n=(n+receiver.edgeValue)|0;return n;} read();",
        "slot-reader-retirement.js").unwrap();
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    vm.execute(&program).unwrap();
    vm.programs.enable_js_lane_methods();
    vm.keep_slot_representations();
    let function = program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "read")).unwrap();
    let method = vm.compile_lane_method(&program, function as u32,
        &|_, _| Vec::new(), &|_| (0, 0), Vec::new(), &[]).unwrap();
    assert!(!method.dependencies.slots.is_empty(), "the actual field read must subscribe");
    let published = vm.programs.publish_lane_method(method, &mut vm.heap, vm.method_cache_atoms.readers()).unwrap();
    let active = published.lease().unwrap();
    let weak = Rc::downgrade(&active);
    let receiver_atom = vm.intern_atom("receiver");
        let receiver = vm.own_property(vm.realm.globals, receiver_atom).unwrap();
        let atom = vm.intern_atom("edgeValue");
        let shape = vm.heap.object(receiver).unwrap().shape();
        let slot = vm.shape_slot(shape, atom).unwrap();
        vm.heap.property_set(receiver, slot, Value::integer(2));
        assert!(published.lease().is_some());
        vm.heap.property_set(receiver, slot, Value::number(0.5));
        assert!(published.lease().is_none(), "widening retires future entry at the write");
        assert_eq!(vm.heap.property_get(vm.heap.object(receiver).unwrap(), slot), Some(Value::number(0.5)));
        assert!(weak.upgrade().is_some(), "retirement preserves the active code lease");
        drop(active);
        assert!(weak.upgrade().is_none());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn numeric_result_signature_joins_typed_returns_and_preserves_float_values() {
        let integer = "function choose(n,x){n=n|0;x=x|0;if(n<0)return x|0;for(let i=0;i<n;i=(i+1)|0)x=(x+1)|0;return x|0;}";
        let mixed = "function choose(n,x){n=n|0;if(n<0)return x|0;return x*1;}";
        for typescript in [false, true] {
            for (source, format) in [(integer, NativeNumberResult::Int32), (mixed, NativeNumberResult::Float64)] {
                let source = if typescript { source.replace("(n,x)", "(n:number,x:number):number") } else { source.to_owned() };
                let program = if typescript { crate::Engine::specialize_typescript(&source, "result-signature.ts") }
                    else { crate::Engine::specialize(&source, "result-signature.js") }.unwrap();
                let artifact = compile_js_typed_numeric_region(&program, 1).expect("shared typed return region");
                assert_eq!(artifact.result, format);
                let compiled = CompiledJsFunction::from_artifact(artifact).unwrap();
                let interrupt = crate::ExecutionInterrupt::default();
                for n in [-1, 0, 3] { for x in [i32::MIN, -1, 0, i32::MAX] {
                    let expected = if format == NativeNumberResult::Int32 && n >= 0 { x.wrapping_add(n) } else { x };
                    let result = direct_call(&compiled, &[Value::integer(n), Value::integer(x)], &interrupt);
                    assert!(matches!(result, Some(value) if value.to_bits() == Value::integer(expected).to_bits()),
                        "{source}:n={n},x={x}");
                }}
                if format == NativeNumberResult::Float64 {
                    for x in [-0.0, 1.5, f64::MIN_POSITIVE, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                        let result = direct_call(&compiled, &[Value::integer(1), Value::number(x)], &interrupt);
                        assert!(matches!(result, Some(value) if value.to_bits() == Value::number(x).to_bits()),
                            "mixed result preserves {x:?}");
                    }
                }
                for value in [Value::UNDEFINED, Value::TRUE, Value::NULL] {
                    assert!(direct_call(&compiled, &[value, Value::integer(1)], &interrupt).is_none(), "TS annotations are not input proofs");
                }
                interrupt.request_termination();
                assert!(direct_call(&compiled, &[Value::integer(1), Value::integer(1)], &interrupt).is_none());
            }
        }
    }

    #[test]
    fn native_widening_edges_retire_readers_before_guest_store() {
        #[derive(Clone, Copy)]
        enum Edge { Store, Domain, Pointer }
        for edge in [Edge::Store, Edge::Domain, Edge::Pointer] {
            let program = crate::Engine::specialize(
                "var receiver={edgeValue:1,a:2,b:3}; function read(){var n=0;for(var i=0;i<5;i++)n=(n+receiver.edgeValue)|0;return n;} function write(x){receiver.edgeValue=x;} function make(n){return function store(){this.edgeValue=n;};} var setter=make(0.5);setter.call(receiver);write(1);read();",
                "native-widening-edge.js").unwrap();
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.execute(&program).unwrap();
            vm.programs.enable_js_lane_methods();
            vm.keep_slot_representations();
            let reader = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "read")).unwrap();
            let writer = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "write")).unwrap();
            let store = program.function(writer).instructions().find(|i| i.op().is_set_field()).unwrap();
            let method = vm.compile_lane_method(&program, reader as u32,
                &|_, _| Vec::new(), &|_| (0, 0), Vec::new(), &[]).unwrap();
            assert!(!method.dependencies.slots.is_empty());
            let published = vm.programs.publish_lane_method(method, &mut vm.heap, vm.method_cache_atoms.readers()).unwrap();
            let active = published.lease().unwrap();
            let atom = vm.intern_atom("receiver");
            let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
            let shape = vm.heap.object(receiver).unwrap().shape();
            let slot = vm.shape_slot(shape, store.atom_index()).unwrap();
            let residual = (&program as *const ResidualProgram).addr() as u64;
            let machine = (&mut vm as *mut Vm<crate::test_host::TestHost>).addr() as u64;
            // Each edge uses the same live VM/program and exact slot as a native
            // region; rejection must precede the guest store or raw pointer escape.
            let admitted = unsafe {
                match edge {
                    Edge::Domain => lane_field_write_admission::<crate::test_host::TestHost>(machine, residual,
                        shape as u64, slot as u64, crate::typed_ir::SlotType::from_type(crate::typed_ir::Type::NUMBER) as u64),
                    Edge::Store => lane_field_set::<crate::test_host::TestHost>(machine, residual, receiver.to_bits(),
                        store.atom_index() as u64, store.cache_site_index() as u64, Value::number(0.5).to_bits()),
                    Edge::Pointer => u64::from(vm.cached_field_store_pointer(&program, receiver,
                        store.atom_index(), store.cache_site_index(), crate::typed_ir::Type::NUMBER).is_some()),
                }
            };
            assert_eq!(admitted, 0, "widening must exit before mutation");
            assert!(published.lease().is_none());
            assert_eq!(vm.heap.slot_type(shape, slot), crate::typed_ir::Type::NUMBER);
            assert_eq!(vm.heap.property_get(vm.heap.object(receiver).unwrap(), slot), Some(Value::integer(1)));
            vm.heap.property_set(receiver, slot, Value::number(0.5));
            assert!(vm.heap.admit_slot_writes(shape, slot, crate::typed_ir::Type::NUMBER),
                "the wider domain is admitted on the next entry");
            assert_eq!(vm.heap.unrepresented_slots(), 0);
            drop(active);
        }
    }

    #[test]
    fn jit_loop_slots_preserve_widening_and_reentry() {
        let source = concat!("run({count:4096,value:0},1);\n",
            include_str!("../../tests/fixtures/loop-slot-lifetime.js"), "\nthrow 42;");
        for typescript in [false, true] {
            let program = if typescript { crate::Engine::specialize_typescript(source, "jit-loop-slots.cts") }
                else { crate::Engine::specialize(source, "jit-loop-slots.cjs") }.unwrap();
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::Compiled);
            let error = vm.execute(&program).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::integer(42)), "{error}");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn function_binding_requires_call_budget_and_clear_interrupt() {
        let (native, _, _) = linked_test_program("throw function leaf(){};", false);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let callee = vm.execute_native_program(&native, true).unwrap_err().thrown_value().unwrap();
        let (program, function, _) = vm.same_realm_user_target(callee).unwrap();
        let admitted = |vm: &Vm<crate::test_host::TestHost>, depth| unsafe {
            lane_function_binding::<crate::test_host::TestHost>(vm as *const _ as u64, 0,
                u64::from(program.raw()), callee.to_bits(), u64::from(function), depth)
        };
        assert_eq!(admitted(&vm, 1), 1);
        {
            let _limit = crate::stack::GuestCallLimitGuard::enter(1);
            let _active = crate::stack::GuestCallGuard::enter().unwrap();
            assert_eq!(admitted(&vm, 1), 0, "no store may run after call-depth exhaustion");
        }
        assert_eq!(admitted(&vm, 1), 1);
        {
            let _limit = crate::stack::GuestCallLimitGuard::enter(2);
            let _active = crate::stack::GuestCallGuard::enter().unwrap();
            assert_eq!(admitted(&vm, 1), 1);
            assert_eq!(admitted(&vm, 2), 0, "the complete chain must fit before effects");
        }
        vm.execution_interrupt.request_termination();
        assert_eq!(admitted(&vm, 1), 0, "termination must precede the inlined store");
    }

    #[cfg(feature = "aot")]
    #[test]
    fn bound_field_arena_preserves_slot_bounds() {
        let isa = super::super::jit_backend::target_isa().unwrap();
        for bound in [false, true] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 4]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut native, &mut context);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let p = builder.block_params(entry).to_vec();
            let arena = FieldArena::Unbound { values: p[0], length: p[1] };
            let arena = if bound { arena.bind(&mut builder, p[3]) } else { arena };
            let miss = builder.create_block();
            let address = arena.address(&mut builder, p[2], FieldSlot::Bound(p[3]), miss);
            builder.ins().return_(&[address]);
            builder.switch_to_block(miss);
            let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
            builder.ins().return_(&[deleted]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: this function performs integer address arithmetic only and
            // never dereferences the synthetic arena or returned address.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64, u64, u64) -> u64>(memory.pointer()) };
            let width = crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u64;
            let base = width;
            for (length, storage, slot) in [
                (0, 0, 0), (1, 0, 0), (1, 0, 1), (1, 0, 2),
                (3, 1, 1), (3, 2, 1),
                (u64::from(u32::MAX), u64::from(u32::MAX), 0),
                (u64::from(u32::MAX) + u64::from(u16::MAX) + 1,
                    u64::from(u32::MAX), u64::from(u16::MAX)),
            ] {
                let expected = if storage + slot < length { base + (storage + slot) * width }
                    else { Value::DELETED.to_bits() };
                assert_eq!(unsafe { run(base, length, storage, slot) }, expected,
                    "bound={bound}, length={length}, storage={storage}, slot={slot}");
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn nullable_slot_observations_keep_checks_across_execution_tiers() {
        let source = include_str!("../../tests/fixtures/nullable-slot-writes.js");
        let program = crate::Engine::specialize(source, "nullable-slot-writes.js").unwrap();
        let native = crate::NativeJsProgram::<crate::test_host::TestHost>::compile_eager(program.clone()).unwrap();
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled, ExecutionMode::NativeOnly] {
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.programs.enable_js_lane_methods();
            vm.set_execution_mode(mode);
            if mode == ExecutionMode::NativeOnly { vm.execute_native_program(&native, true).unwrap(); }
            else { vm.execute(&program).unwrap(); }
            let atom = vm.lookup_atom("answer").unwrap();
            // Local Node oracle: 3000 warm writes, 12 widening cases, 64 later writes.
            assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::integer(3076)));
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_holder_read_does_not_require_receiver_arena_storage() {
        for (source, inline_holder) in [
            ("function read(o){return o.method;} function Box(){} Box.prototype.method=42; Box.prototype.pad=0; globalThis.box=new Box(); box.own=1; read(box);", false),
            ("function read(o){return o.method;} function Box(){} Box.prototype.method=42; globalThis.box=new Box(); box.own=1; read(box);", true),
        ] {
        let program = crate::Engine::specialize(source, "packed-holder.js").unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.execute(&program).unwrap();
        let atom = vm.lookup_atom("box").unwrap();
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        assert!(vm.heap.object(receiver).unwrap().inline_properties().is_some());
        let program = vm.programs.get(ProgramId::MAIN).unwrap();
        let function = program.functions.iter().position(|function|
            function.header().name.is_some_and(|name| &program.atoms[name as usize] == "read")).unwrap();
        let site = program.function(function).instructions().find_map(|instruction| match instruction.field_lookup() {
            Some(FieldLookup::Atom { cache_site, .. }) => Some(cache_site), _ => None,
        }).unwrap();
        let cache = vm.field_cache_entry(site);
        assert_ne!(cache.holder, super::super::NO_FIELD_HOLDER);
        assert_eq!(vm.record_field_entry_hit(cache, receiver), Some(Value::number(42.0)));
        let prototype = vm.heap.object(receiver).unwrap().proto();
        let holder = vm.heap.object(prototype).unwrap();
        assert_eq!(holder.exact_shape(), cache.holder);
        assert_eq!(holder.inline_properties().is_some(), inline_holder);
        assert_eq!(holder.extras_storage_start().is_some(), !inline_holder);

        let isa = super::super::jit_backend::target_isa().unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 4]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let mut binding = FieldSlotBinding {
            receiver: builder.ins().iconst(types::I32, i64::from(cache.receiver)),
            holder: builder.ins().iconst(types::I32, i64::from(cache.holder)),
            slot: FieldSlot::Bound(builder.ins().iconst(types::I64, i64::from(cache.slot))),
            records: parameters[0],
            arena: FieldArena::Unbound { values: parameters[1], length: parameters[2] },
            writes: None, admitted: None,
        };
        let slot = binding.slot.read(&mut builder);
        binding.arena = binding.arena.bind(&mut builder, slot);
        let miss = builder.create_block();
        let unused = builder.ins().iconst(types::I64, 0);
        let (address, _) = emit_js_cached_field_slot(&mut builder, unused,
            parameters[3], site, true, miss, Some(binding), None, None);
        let value = builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0);
        builder.ins().return_(&[value]);
        builder.switch_to_block(miss);
        let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
        builder.ins().return_(&[deleted]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the four inputs name this VM's live reservation, arena and
        // receiver. No guest call or collection occurs while the code runs.
        let run = unsafe { std::mem::transmute::<*mut u8,
            unsafe extern "C" fn(usize, *const Value, usize, u64) -> u64>(memory.pointer()) };
        let (values, length) = vm.heap.native_property_values();
        let records = vm.heap.object_record_base().unwrap() as usize;
        assert_eq!(Value::from_bits(unsafe { run(records, values, length, receiver.to_bits()) }), Value::number(42.0));
        // A receiver that has not proved the cached shape cannot read this slot.
        assert_eq!(unsafe { run(records, values, length, Value::NULL.to_bits()) }, Value::DELETED.to_bits());
        assert_eq!(unsafe { run(records, values, length, vm.realm.globals.to_bits()) }, Value::DELETED.to_bits());
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn activation_admits_only_authoritative_this_and_dense_function_captures() {
        let mut vm = semantic_vm("throw 7;");
        let receiver = vm.object();
        vm.frames[0].this = receiver;
        vm.frames[0].fixed_this = false;
        JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, _| {
            assert!(activation.dispatch.fixed_this_slot.is_null());
        }).unwrap();
        vm.frames[0].fixed_this = true;
        let slots = crate::heap::EnvironmentSlots::from(vec![Value::number(7.0)]);
        vm.frames[0].capture_cache = super::super::CaptureCache {
            env: vm.frames[0].env, captured: vm.frames[0].captured,
            epoch: vm.heap.collection_epoch(), slots: &slots, root: None,
        };
        JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, vm| {
            assert_eq!(activation.dispatch.fixed_this_slot, &vm.frames[0].this as *const Value);
            assert!(!activation.dispatch.capture_values.is_null());
        }).unwrap();
        vm.frames[0].capture_cache.root = Some(Some(ProgramId::MAIN.raw()));
        JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, _| {
            assert!(activation.dispatch.capture_values.is_null());
        }).unwrap();
    }

    #[cfg(feature = "aot")]
    #[test]
    fn typed_native_loops_preserve_stores_in_native_only_and_unspecialized_execution() {
        let source = "function tail(node) { while(node.next!==null) { node.mark=node.mark+1; node=node.next; } return node; } const end={next:null,mark:0}; const head={next:end,mark:0}; for(let i=0;i<40;i++) { if(tail(head)!==end) throw 'identity'; } throw head.mark;";
        for specialized in [false, true] {
            let mut program = crate::Engine::specialize(source, "native-loop-store.js").unwrap();
            program.specialized = specialized;
            let native = crate::NativeJsProgram::<crate::test_host::TestHost>::compile_eager(program).unwrap();
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.execute_native_program(&native, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::number(40.0)));
        }
    }
