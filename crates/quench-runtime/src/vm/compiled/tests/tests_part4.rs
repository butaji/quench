use super::*;

#[cfg(all(feature = "native-execution", not(any(feature = "jit", feature = "aot"))))]
#[test]
fn static_lane_call_helper_declines_without_dynamic_context() {
    // Execution-only images lend no interpreter lane, so even a null context
    // must decline before inspecting it or publishing a dynamic continuation.
    let status = unsafe { super::super::dispatch_fast::lane_body_call::<crate::test_host::TestHost>(0, 0, 0, 0) };
    assert_eq!(status, LANE_CALL_MISSED);
}

#[test]
fn nested_lane_entry_restores_outer_fault_binding() {
    if !FAULT_POLL_SUPPORTED { return; }
    let program = crate::Engine::specialize(
        "function count(n){n=n|0;let sum=0;for(let i=0;i<n;i=(i+1)|0)sum=(sum+1)|0;return sum;}",
        "nested-poll-binding.js").unwrap();
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    let method = vm.compile_lane_method(&program, 1, &|_, _| Vec::new(),
        &|_| (0, 0), Vec::new(), &[]).expect("polling lane body");
    assert!(method.fault_poll);
    let frame = lane_frame();
    // The real outer landing must survive an inner trampoline overwriting it.
    // This sentinel is never entered: the inner entry installs its own landing.
    let outer_landing = (&program as *const ResidualProgram).addr();
    let original_landing = unsafe { std::mem::replace(&mut (*frame).context.entry_stack, outer_landing) };
    // These outer program inputs are never dereferenced by the nested body:
    // entry must replace them, then restore them before returning to the caller.
    let original_program = unsafe { ((*frame).dispatch.program_id, (*frame).dispatch.field_entries) };
    let outer_program = (ProgramId::from_raw(1), (&program as *const ResidualProgram).addr());
    unsafe { ((*frame).dispatch.program_id, (*frame).dispatch.field_entries) = outer_program; }
    let before = unsafe {
        let context = &(*frame).context;
        (context.stack_limit, context.entry_stack, context.interrupt,
            context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner)
    };
    let mut registers = vec![Value::number(7.0); usize::from(method.caller_registers)];
    let result = vm.run_nested_lane_method(&method, ProgramId::MAIN, &program,
        registers.as_mut_ptr(), Value::DELETED, std::ptr::null(), std::ptr::null_mut());
    assert!(matches!(result, LaneExit::Returned { value, .. } if value == Value::number(7.0)));
    let after = unsafe {
        let context = &(*frame).context;
        (context.stack_limit, context.entry_stack, context.interrupt,
            context.adapter_payload, context.poll_start, context.poll_end, context.interrupt_owner)
    };
    assert_eq!(before, after, "outer native recovery binding is borrowed, not replaced");
    assert_eq!(unsafe { ((*frame).dispatch.program_id, (*frame).dispatch.field_entries) }, outer_program,
        "the nested entry must restore the outer program's field table");
    unsafe { ((*frame).dispatch.program_id, (*frame).dispatch.field_entries) = original_program; }
    unsafe { (*frame).context.entry_stack = original_landing; }

    unsafe extern "C" fn outer(context: *mut CallContext, _: u64, _: u64,
        arguments: *const u64, out: *mut u64) -> u32 {
        // Only borrowed raw inputs live across the poll: recovery skips no destructor.
        let inputs = unsafe { std::slice::from_raw_parts(arguments, 4) };
        let vm = unsafe { &mut *(inputs[0] as *mut Vm<crate::test_host::TestHost>) };
        let method = unsafe { &*(inputs[1] as *const LaneMethod) };
        let program = unsafe { &*(inputs[2] as *const ResidualProgram) };
        let registers = inputs[3] as *mut Value;
        let result = vm.run_nested_lane_method(method, ProgramId::MAIN, program,
            registers, Value::DELETED, std::ptr::null(), std::ptr::null_mut());
        let LaneExit::Returned { value, .. } = result else { return NATIVE_JS_STOPPED; };
        unsafe { out.write(value.to_bits()); }
        vm.execution_interrupt.request_termination();
        // The restored outer landing must own this fault, after the inner landing died.
        let poll = unsafe { (*context).poll_start as *const std::sync::atomic::AtomicBool };
        unsafe { (*poll).load(std::sync::atomic::Ordering::Acquire); }
        0
    }
    let inputs = [&mut vm as *mut _ as u64, &method as *const _ as u64,
        &program as *const _ as u64, registers.as_mut_ptr() as u64];
    let page = native_poll_page(&vm.execution_interrupt).expect("owned poll page");
    let context = unsafe { &mut (*frame).context };
    context.bind_native_adapter(context.adapter_payload,
        vm.execution_interrupt.attention_flag(), Some(page));
    let mut out = 0;
    let status = unsafe { enter_wasm(context, 0, 0, inputs.as_ptr(), &mut out, outer) };
    assert_eq!(i64::from(status), TERMINATED_STATUS);
    assert_eq!(Value::from_bits(out), Value::number(7.0));
}

#[test]
fn numeric_jit_shared_entry_observes_live_termination() {
    use std::{sync::mpsc, thread, time::Duration};
    const TIMEOUT: Duration = Duration::from_secs(5);
    fn started(context: &mut crate::NativeContext<'_, crate::test_host::TestHost>,
        _: crate::RootId, _: &[crate::RootId]) -> Result<crate::RootId, crate::RootedError> {
        context.host_mut().state::<mpsc::Sender<()>>().send(()).unwrap();
        Ok(context.undefined())
    }
    let (ready, entered) = mpsc::channel::<()>();
    let (send_handle, receive_handle) = mpsc::channel();
    let (complete, finished) = mpsc::channel();
    let worker = thread::Builder::new().stack_size(crate::WORKER_STACK_SIZE).spawn(move || {
        let program = crate::Engine::specialize(r#"
                function spin(n){let x=1;for(let i=0;i<n;i=(i+1)|0)x=x^(x<<13);return x;}
                for(let warm=0;warm<64;warm++)spin(1);
                started();spin(Infinity);
            "#, "numeric-jit-interrupt.js").unwrap();
        let host = crate::test_host::TestHost::default()
            .with_natives(crate::host_functions![global "started" (0) => started]).with_state(Rc::new(ready));
        let mut vm = Vm::new(host);
        vm.set_execution_mode(ExecutionMode::Compiled);
        send_handle.send(vm.execution_interrupt.clone()).unwrap();
        let budget = crate::stack::GuestCallGuard::remaining();
        let error = vm.execute(&program).unwrap_err();
        assert!(error.is_execution_terminated(), "{error}");
        assert_eq!(crate::stack::GuestCallGuard::remaining(), budget);
        let Some(super::super::program_store::JsTierState::Compiled(native)) =
            vm.programs.js_tier(ProgramId::MAIN, 1) else { panic!("numeric function must compile"); };
        #[cfg(not(quench_profiled))]
        assert!(native.native_entries() > 0);
        complete.send(()).unwrap();
    }).unwrap();
    let handle = receive_handle.recv_timeout(TIMEOUT).unwrap();
    entered.recv_timeout(TIMEOUT).unwrap();
    thread::sleep(Duration::from_millis(20));
    handle.request_termination();
    finished.recv_timeout(TIMEOUT).expect("shared numeric JIT loop must observe live cancellation");
    worker.join().unwrap();
}

#[test]
fn scalar_core_binds_poll_pointer_before_loop_but_keeps_atomic_reads() {
    let isa = super::super::jit_backend::target_isa().unwrap();
    let signature = native_entry_signature(isa.pointer_type(), isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(UserFuncName::testcase("bound_poll"), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    let header = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let activation = builder.block_params(entry)[0];
    // The old address establishes fault-poll availability, not executable identity.
    let interrupt = crate::ExecutionInterrupt::default();
    let Some(address) = poll_address(&interrupt) else { return };
    let mut core = RegionCore::new(builder, activation, Some(address));
    core.ins().jump(header, &[]);
    core.switch_to_block(header);
    core.interrupt_safepoint();
    core.interrupt_safepoint();
    core.ins().jump(header, &[]);
    core.seal_all_blocks();
    core.into_builder().finalize(isa.frontend_config());
    cranelift_codegen::verify_function(&native, &*isa).unwrap();
    let owners: Vec<_> = native.layout.block_insts(entry).filter(|&inst| matches!(native.dfg.insts[inst], ir::InstructionData::Load { arg, offset, .. }
            if arg == activation && i32::from(offset) == std::mem::offset_of!(CallContext, poll_start) as i32)).map(|inst| native.dfg.inst_results(inst)[0]).collect();
    assert_eq!(owners.len(), 1, "bind the activation's pointer once before the loop");
    let polls: Vec<_> = native.layout.block_insts(header).filter(|&inst|
        native.dfg.insts[inst].opcode() == ir::Opcode::AtomicLoad).collect();
    assert_eq!(polls.len(), 2, "poll reads must remain separate observable operations");
    for inst in polls { assert_eq!(native.dfg.inst_args(inst), &[owners[0]]); }
}

#[cfg(feature = "aot")]
pub(super) fn semantic_vm(source: &str) -> Vm<crate::test_host::TestHost> {
    let program = crate::Engine::specialize(source, "semantic-entry.js").unwrap();
    semantic_program(program)
}

#[cfg(feature = "aot")]
fn semantic_program(program: ResidualProgram) -> Vm<crate::test_host::TestHost> {
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    vm.initialize_for_snapshot(&program).unwrap();
    let program = vm.programs.get(ProgramId::MAIN).unwrap();
    let body = program.function(0);
    let mut frame = Box::new(Frame::empty(ProgramId::MAIN, 0));
    frame.registers.resize(usize::from(body.registers), Value::UNDEFINED);
    frame.locals.resize(usize::from(body.locals), Value::UNDEFINED);
    frame.initialized_registers = frame.registers.len();
    vm.frames.push(frame);
    vm.active_registers = vm.frames[0].registers.as_mut_ptr();
    vm
}

#[cfg(feature = "aot")]
fn semantic_instruction(vm: &Vm<crate::test_host::TestHost>, opcode: Op) -> (usize, WideInstruction) {
    let program = vm.programs.get(ProgramId::MAIN).unwrap();
    let body = program.function(0);
    body.code.iter().copied().enumerate().find_map(|(pc, packed)| {
        let instruction = super::super::dispatch_fast::decoded_instruction(body, packed);
        (instruction.op() == opcode).then_some((pc, instruction))
    }).expect("test operation exists")
}

#[cfg(feature = "aot")]
fn invoke_semantic(
    vm: &mut Vm<crate::test_host::TestHost>, pc: usize,
    entry: unsafe extern "C" fn(*mut std::ffi::c_void, u64, *const u64, *mut u64) -> i64,
) -> (i64, u64, Option<JsNativeStep>) {
    let program = vm.programs.get(ProgramId::MAIN).unwrap();
    let body = program.function(0);
    let instruction = super::super::dispatch_fast::decoded_instruction(body, body.code[pc]);
    let operands = super::super::NativeJsOperands::new(
        instruction.a(), instruction.b(), instruction.c(), instruction.imm(),
    );
    with_semantic_context(vm, pc, |context, result| {
        // SAFETY: the context owner roots the frame; operands and result live through this call.
        unsafe { entry(context.cast(), pc as u64, operands.as_ptr(), result) }
    })
}

#[cfg(feature = "aot")]
fn with_semantic_context(
    vm: &mut Vm<crate::test_host::TestHost>, pc: usize,
    run: impl FnOnce(*mut CallContext, *mut u64) -> i64,
) -> (i64, u64, Option<JsNativeStep>) {
    JsNativeActivation::with(vm, ProgramId::MAIN, 0, pc, false, true, |activation, vm| {
        let dispatch = &mut activation.dispatch;
        let mut context = CallContext::native_adapter(
            (dispatch as *mut JsStepDispatch).cast(),
            vm.execution_interrupt.attention_flag(), None,
        );
        let vm = vm as *mut Vm<crate::test_host::TestHost>;
        context.vm = vm.cast();
        dispatch.bind_native_storage(vm);
        context.program = ProgramId::MAIN;
        let mut result = 0;
        let status = run(&mut context, &mut result);
        (status, result, dispatch.outcome.take())
    }).unwrap()
}

/// Publishes compiled `code` of a function lowered with `native_entry_signature` to
/// executable memory, and its entry.
#[cfg(feature = "aot")]
fn publish_native_entry(code: &[u8]) -> Option<(CodeMemory, crate::NativeJsFunctionEntry)> {
    let memory = publish_code(code)?;
    // SAFETY: callers lower the function with `native_entry_signature`.
    let entry = unsafe { std::mem::transmute::<*mut u8, crate::NativeJsFunctionEntry>(memory.pointer()) };
    Some((memory, entry))
}

#[cfg(feature = "aot")]
fn live_native_graph(vm: &Vm<crate::test_host::TestHost>) -> (CodeMemory, crate::NativeJsFunctionEntry, Vec<super::super::specialize::InstructionSite>) {
    let program = vm.programs.get(ProgramId::MAIN).unwrap();
    live_native_function(&program, 0, None)
}

#[cfg(feature = "aot")]
fn partitioned_native_graph(vm: &Vm<crate::test_host::TestHost>) -> (crate::PreparedScript, crate::NativeJsFunctionEntry) {
    let program = vm.programs.get(ProgramId::MAIN).unwrap();
    let options = crate::NativeJsCodegenOptions { region_instruction_budget: std::num::NonZeroUsize::new(1).unwrap(), backend: crate::NativeJsBackend::Cranelift };
    let native = crate::NativeJsProgram::compile_eager_with_options((*program).clone(), options).unwrap();
    let prepared = vm.prepare_native_script(native).unwrap();
    let entry = prepared.native.as_ref().unwrap().entries()[0];
    (prepared, entry)
}

#[cfg(feature = "aot")]
pub(super) fn live_native_function(program: &ResidualProgram, function_id: u32, entries: Option<*mut u64>)
    -> (CodeMemory, crate::NativeJsFunctionEntry, Vec<super::super::specialize::InstructionSite>)
{
    let sites = super::super::specialize::instruction_sites(super::super::specialize::Division::Offline, program.function(function_id as usize));
    let isa = super::super::jit_backend::target_isa().unwrap();
    let mut signature = None;
    let native = super::super::native_graph::lower_body(&sites, function_id, &*isa, None,
        |builder, site, context, result| {
            if site.pc == 0 && let Some(entries) = entries {
                let pointer = builder.ins().iconst(isa.pointer_type(), entries as i64);
                let previous = builder.ins().load(types::I64, MemFlagsData::trusted(), pointer, 0);
                let count = builder.ins().iadd_imm_s(previous, 1);
                builder.ins().store(MemFlagsData::trusted(), count, pointer, 0);
            }
            let signature = *signature.get_or_insert_with(|| builder.import_signature(native_js_operation_signature(&*isa)));
            let target = site.op.native_semantic_entry::<crate::test_host::TestHost>() as usize;
            let target = builder.ins().iconst(isa.pointer_type(), target as i64);
            let arguments = builder.ins().iconst(isa.pointer_type(), site.operands.as_ptr() as i64);
            let pc = builder.ins().iconst(types::I64, site.pc as i64);
            let call = builder.ins().call_indirect(signature, target, &[context, pc, arguments, result]);
            builder.inst_results(call)[0]
        }).unwrap();
    // No code size budget is needed by this focused publication check; the
    // production object backend handles relocations instead of live addresses.
    let (memory, entry) = compile_native_function(native, &*isa, usize::MAX, publish_native_entry)
        .expect("native semantic graph compiles");
    (memory, entry, sites)
}

#[cfg(feature = "aot")]
#[test]
fn native_object_table_binding_tracks_heap_lifetime() {
    let source = "globalThis.tableProbe=0; throw 7;";
    let mut vm = semantic_vm(source);
    let original = vm.heap.object_record_base().unwrap() as usize;
    let probe = vm.lookup_atom("tableProbe").unwrap();
    let program = vm.programs.get(ProgramId::MAIN).unwrap();
    let assert_binding = |vm: &mut Vm<crate::test_host::TestHost>, expected| {
        JsNativeActivation::with(vm, ProgramId::MAIN, 0, 0, false, true, |activation, vm| {
            assert_ne!(expected, 0);
            assert_eq!(activation.dispatch.object_record_base, expected);
            assert_eq!(vm.heap.object_record_base().unwrap() as usize, expected);
        }).unwrap();
    };
    assert_binding(&mut vm, original);
    // A normal semantic store may allocate/widen property storage; neither
    // it nor a full collection replaces the record-table reservation.
    vm.set_property(vm.realm.globals, probe, Value::number(1.5)).unwrap();
    vm.collect_now(&program);
    assert_binding(&mut vm, original);
    assert_eq!(vm.own_property(vm.realm.globals, probe), Some(Value::number(1.5)));

    let mut other = semantic_vm(source);
    let other_base = other.heap.object_record_base().unwrap() as usize;
    // Reentry in another VM admits that VM's table, without changing the
    // outer activation's admitted base or retaining it for later entries.
    JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, _| {
        assert_binding(&mut other, other_base);
        assert_eq!(activation.dispatch.object_record_base, original);
    }).unwrap();

    // Realm initialization destroys the old heap reservation. Rebuild the
    // caller frame as semantic_program does, then admit the current table;
    // allocators may reuse the address, so pointer inequality is no proof.
    vm.initialize_for_snapshot(&program).unwrap();
    let current = vm.programs.get(ProgramId::MAIN).unwrap();
    let body = current.function(0);
    let mut frame = Box::new(Frame::empty(ProgramId::MAIN, 0));
    frame.registers.resize(usize::from(body.registers), Value::UNDEFINED);
    frame.locals.resize(usize::from(body.locals), Value::UNDEFINED);
    frame.initialized_registers = frame.registers.len();
    vm.frames.push(frame);
    vm.active_registers = vm.frames[0].registers.as_mut_ptr();
    let reset_base = vm.heap.object_record_base().unwrap() as usize;
    assert_binding(&mut vm, reset_base);
    assert_binding(&mut other, other_base);
}

#[cfg(feature = "aot")]
#[test]
fn native_activation_owns_metadata_without_retaining_it_between_entries() {
    let vm_source = "throw 7;";
    let mut first_vm = semantic_vm(vm_source);
    let mut second_vm = semantic_vm(vm_source);
    let weak = JsNativeActivation::with(&mut first_vm, ProgramId::MAIN, 0, 0, false, true, |first, _| {
        JsNativeActivation::with(&mut second_vm, ProgramId::MAIN, 0, 0, false, true, |second, _| {
            assert_eq!(first.dispatch.program_id, second.dispatch.program_id);
            assert!(!Rc::ptr_eq(&first._program, &second._program));
            assert_eq!(first.dispatch.residual, Rc::as_ptr(&first._program) as usize);
            assert_eq!(first.dispatch.body, first._program.function(0) as *const _);
            assert_ne!(first.dispatch.body, second.dispatch.body);
            let weak = Rc::downgrade(&first._program);
            let mut program = first._program.clone();
            assert!(Rc::get_mut(&mut program).is_none(), "admitted records cannot be uniquely released");
            weak
        }).unwrap()
    }).unwrap();
    // Only the VM owns the program after the native scope returns.
    assert_eq!(weak.strong_count(), 1);
    drop(first_vm);
    assert!(weak.upgrade().is_none());
}

#[cfg(feature = "aot")]
#[test]
fn native_entry_panic_transport_does_not_poison_the_next_entry() {
    unsafe extern "C" fn report_panic(
        context: *mut std::ffi::c_void, _: u64, _: u64, _: *const u64, _: *mut u64,
    ) -> u32 {
        // Model a panic already caught by the semantic callback's FFI boundary.
        unsafe { (*context.cast::<CallContext>()).raised = Some(Raised::Panic(Box::new("native panic"))) };
        u32::try_from(RAISED_STATUS).unwrap()
    }
    let mut vm = semantic_vm("throw 7;");
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, vm| {
            let dispatch = &mut activation.dispatch;
            let interrupt = vm.execution_interrupt.attention_flag() as *const _;
            run_js_native_entry(report_panic, vm, ProgramId::MAIN,
                (dispatch as *mut JsStepDispatch).cast(), interrupt, None)
        })
    })).unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"native panic"));

    let (_memory, entry, _operands) = live_native_graph(&vm);
    JsNativeActivation::with(&mut vm, ProgramId::MAIN, 0, 0, false, true, |activation, vm| {
        let dispatch = &mut activation.dispatch;
        let interrupt = vm.execution_interrupt.attention_flag() as *const _;
        let (status, _) = run_js_native_entry(entry, vm, ProgramId::MAIN,
            (dispatch as *mut JsStepDispatch).cast(), interrupt, None).unwrap();
        assert_eq!(status, NATIVE_JS_STOPPED);
        let Err(error) = dispatch.outcome.take().unwrap().result else { panic!("expected guest throw") };
        assert_eq!(error.thrown_value(), Some(Value::number(7.0)));
    }).unwrap();
}

#[cfg(feature = "aot")]
#[test]
fn native_semantic_graph_runs_root_branches_and_loops() {
    let mut vm = semantic_vm("globalThis.count = 0; while (globalThis.count < 4) { globalThis.count++; } throw globalThis.count;");
    let (_memory, entry, _operands) = live_native_graph(&vm);
    let (status, _, outcome) = with_semantic_context(&mut vm, 0, |context, result| {
        // SAFETY: the shared context, native code and operand data outlive this entry.
        unsafe { i64::from(entry(context.cast(), 0, 0, std::ptr::null(), result)) }
    });
    assert_eq!(status, i64::from(NATIVE_JS_STOPPED));
    let Err(error) = outcome.expect("graph stops at throw").result else { panic!("expected throw") };
    assert_eq!(error.thrown_value(), Some(Value::number(4.0)));
}

#[cfg(feature = "aot")]
fn linked_test_program(source: &str, typescript: bool) -> (
    crate::NativeJsProgram<crate::test_host::TestHost>, Rc<[std::cell::Cell<u64>]>,
    std::rc::Weak<dyn std::any::Any>,
) {
    let program = if typescript {
        crate::Engine::specialize_typescript(source, "linked.cts")
    } else {
        crate::Engine::specialize(source, "linked.cjs")
    }.unwrap();
    linked_test_residual(program)
}

#[cfg(feature = "aot")]
fn linked_test_residual(program: crate::ResidualProgram) -> (
    crate::NativeJsProgram<crate::test_host::TestHost>, Rc<[std::cell::Cell<u64>]>,
    std::rc::Weak<dyn std::any::Any>,
) {
    let counts: Rc<[std::cell::Cell<u64>]> = (0..program.function_count())
        .map(|_| std::cell::Cell::new(0)).collect();
    let mut owners = Vec::new();
    let mut entries = Vec::new();
    for id in 0..program.function_count() {
        let (memory, entry, operands) = live_native_function(&program, id as u32, Some(counts[id].as_ptr()));
        entries.push(entry);
        owners.push((memory, operands));
    }
    let owner: Rc<dyn std::any::Any> = Rc::new((owners, counts.clone()));
    let weak = Rc::downgrade(&owner);
    // SAFETY: every function was lowered from this program for TestHost;
    // owner retains the code, operands and counter storage it addresses.
    let native = unsafe { crate::NativeJsProgram::from_entries(program, entries, owner) }.unwrap();
    (native, counts, weak)
}
