//! Frames in a lane: pushing a call's frame, a wasm export frame, constructing a frame, and returning a script or plain frame.
use super::*;

/// The facts that keep function `function` of `program` out of a frameless activation, for
/// the census of frame calls and constructions.
#[cfg(feature = "profile-lane-exits")]
pub(in super::super) fn frame_entry_census(vm: &Vm, program: ProgramId, function: u32) -> Option<String> {
    vm.programs.residual(program).and_then(|callee_code| {
        let body = callee_code.function_get(function as usize)?;
        let plan = body.call_plan(callee_code);
        let closures = body.code.iter().any(|packed| body.widened(*packed).op().makes_closure());
        Some(format!(
            "{} @{:?} registers_only={} env_activation={} arrow={} rest={} closures={} arguments={} handlers={} class_ctor={} async={} generator={} promoted={}/{} simple_params={} selective={} self={} plain={} in_loop={} legacy={} live={} addresses={} initial={:?} name_ops={:?} unpromoted={:?}",
            body.name.map(|atom| callee_code.atoms[atom as usize].to_string()).unwrap_or_default(),
            body.source_span.map(|span| span.start), plan.registers_only, plan.environment_activation, body.is_arrow, body.rest,
            closures, body.arguments_slot.is_some(), !body.handlers.is_empty(), body.is_class_constructor, body.is_async,
            body.is_generator, body.local_registers.len(), body.locals, body.simple_parameters, body.selective_capture_slots.is_some(),
            body.self_binding_slot.is_some(), plan.plain, plan.in_loop, plan.legacy_reflection, plan.live_parameters,
            plan.addresses_locals, body.initial_register,
            body.code.iter().map(|packed| body.widened(*packed)).filter(|instruction|
                quench_runtime::staged::names_binding(instruction.op()))
                .map(|instruction| format!("{:?}:{}", instruction.op(), { let atom = instruction.atom_index() as usize; if atom < callee_code.atoms.len() { callee_code.atoms[atom].replace('\0', "~") } else { "?".into() } }))
                .collect::<Vec<_>>(),
            body.local_atoms.iter().enumerate()
                .filter(|(slot, _)| !body.local_registers.iter().any(|entry| usize::from(entry.local) == *slot))
                .map(|(_, atom)| callee_code.atoms[*atom as usize].replace('\0', "~"))
                .collect::<Vec<_>>(),
        ))
    })
}

/// `frame_call_in_lane`'s call of `callee`, resolved to the in-loop `target`, with receiver
/// `this` and the call window's values past its first `skip` (a forwarded call's own
/// arguments, `Vm::forwarded_call`): entered directly when the JavaScript tier promoted it,
/// else pushed as the general loop's in-loop call pushes it.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
pub(in super::super) fn push_frame_call_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    i: &LaneInstruction,
    target: (ProgramId, u32, Value),
    callee: Value,
    this: Value,
    passed: FrameCallArguments<'_>,
) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    // A callee of another program runs in the lane only where the lane admits its program.
    if target.0 != unsafe { (*cx).program }
        && !vm.programs.residual(target.0).is_some_and(|callee_code| vm.admits_in_lane(target.0, callee_code))
    {
        return Resumed::Exit(exit(ip));
    }
    // A callee that has not made its deferred runs takes the general path, as a lane call's
    // does (`ProgramStore::call_may_derive_in_lane`): entering its frame here derived its view at
    // its first call, a callee run once included (jiti npm scenario: -2.1% instructions).
    if !vm.call_may_derive_in_lane(target.0, target.1) {
        return Resumed::Exit(exit(ip));
    }
    #[cfg(feature = "profile-lane-exits")]
    if let Some(body) = frame_entry_census(vm, target.0, target.1) {
        quench_runtime::vm::lane_exits::record_detail(format!("frame call {body}"));
    }
    let window = ImmediateLayout::call_window_base(i.imm());
    let argc = ImmediateLayout::argument_count(i.imm());
    // A function the JavaScript tier promoted is entered directly, as the general loop's
    // in-loop call enters it (`Vm::js_jit_compiled_call`); a guard miss pushes its frame. A
    // forwarded call is pushed, as the general loop's (`Vm::try_inline_call_apply`).
    // SAFETY: the call's window is registers of the running function, which the direct
    // call only reads.
    let arguments = match passed {
        FrameCallArguments::Window(skip) => (skip == 0).then(|| unsafe {
            std::slice::from_raw_parts(r.0.add(usize::from(window)), usize::from(argc))
        }),
        FrameCallArguments::List(list) => Some(list),
    };
    if let Some(arguments) = arguments
        && let Some(value) = vm.js_jit_compiled_call(code, (target.0, target.1), returns, arguments)
    {
        r.set(i.a, value);
        return Resumed::At(r, following(ip), Carried::script(cx));
    }
    vm.materialize_lane_activations(cx);
    // The caller is now the innermost frame, its registers the frame's.
    let (frame, r) = unsafe { ((*cx).frame(), (*cx).registers) };
    // SAFETY: the call record lies in the running view.
    let call_pc = unsafe { record_pc(cx, ip) } as usize;
    // SAFETY: the call's window is registers of the caller frame, which the push neither
    // moves nor frees before it copies them; a list outlives the push.
    let arguments: *const [Value] = match passed {
        FrameCallArguments::Window(skip) => unsafe {
            std::ptr::slice_from_raw_parts(r.0.add(usize::from(window) + skip), usize::from(argc) - skip)
        },
        FrameCallArguments::List(list) => list,
    };
    let destination = i.a & quench_runtime::bytecode::REGISTER_MASK;
    let pushed = vm.push_in_loop_call(
        code,
        frame,
        call_pc + 1,
        target,
        callee,
        this,
        unsafe { &*arguments },
        destination,
    );
    match pushed {
        Some(Ok(StepResult::PushFrame {
            destination,
            call_guard,
            completion,
            ..
        })) => {
            // A callee of another program runs in its own, which the push made active
            // and the loop's executing one sets aside as the loop's own push does.
            let caller_program = (target.0 != unsafe { (*cx).program }).then(|| {
                // SAFETY: the general loop's executing program outlives the lane run.
                let executing = unsafe { &mut *(*cx).executing };
                let saved = quench_runtime::vm::dispatch_frame::SavedProgram {
                    id: executing.id,
                    retained: std::mem::replace(&mut executing.retained, vm.programs.get(target.0)),
                };
                executing.id = target.0;
                enter_program_in_lane(cx, target.0);
                saved
            });
            // SAFETY: the general loop's pending calls outlive the lane run.
            unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
                caller: frame,
                caller_cursor: None,
                call_pc: call_pc as u32,
                destination,
                completion,
                returns,
                call_guard: Some(call_guard),
                caller_program,
            });
            // The pushed frame's guard narrows the activations the thread admits.
            vm.lane_stack.refresh_activation_limit();
            // The tier observes the frame's entry as the general loop's would, and a
            // native result continues at the `Return` that completes the frame.
            let callee_frame = frame + 1;
            // SAFETY: the context's program, now the callee's, outlives the lane run.
            if let Some(return_pc) = vm.js_tier_frame_entry(unsafe { &*(*cx).code }, callee_frame) {
                vm.frames[callee_frame].pc = return_pc;
            }
            enter_script_frame_in_lane(cx, callee_frame)
        }
        // A failed push (the guest call depth) throws at the call, as the general path's.
        Some(Err(error)) => {
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(ip))
        }
        _ => Resumed::Exit(exit(ip)),
    }
}

/// A JavaScript `Call` of an exported Wasm function that `Vm::wasm_export_frame_call`
/// admits: its frame, over the instance environment, pushed with the pending call that
/// converts its result (`CallCompletion::WasmExport`) and entered in this lane, as a Wasm
/// call's frame is (`wasm_calls`'s `push_frame`), so the call nests no native frame and no
/// run of the general loop. Lane activations become frames first, as for any frame call.
/// `None` leaves the call to the native: a run that is not a frame's own JavaScript run
/// (`LaneContext::script_calls`), a callee native code may run, a call completing a caller
/// the lane does not complete (`returning_call_admitted`), or another program the lane
/// does not admit.
#[inline(never)]
pub(in super::super) fn lane_wasm_export_frame(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Resumed> {
    quench_runtime::staged::wasm_activation::step_export_call_of(&mut LaneExportCall { cx, r, ip, i: std::ptr::from_ref(i), call: None })
}

/// `new` of a user constructor that needs a VM frame, made in place as the general loop's
/// in-loop construction makes it (`Vm::inline_construct_target`): lane activations become
/// frames, the receiver is allocated, and the constructor's frame is pushed with the pending
/// construction that substitutes the receiver for a non-object result
/// (`return_script_frame_in_lane`). A derived class constructor is pushed the same way without a
/// receiver (`Vm::inline_derived_construct_target`): its frame binds `this` at `super(...)`, and
/// its pending call applies the derived return rules (`CallCompletion::DerivedConstructor`).
/// Any other construction takes the general path.
#[inline(never)]
pub(in super::super) fn frame_construct_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Ip {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let callee = r.get(i.b);
    // The construction's site PC in the running view, read before the activations below become frames.
    // SAFETY: `ip` is a record of the running view.
    let pc = unsafe { record_pc(cx, ip) };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    // The checks `inline_construct_target` makes of the callee alone, before activations
    // become frames for it. A constructor of another program is pushed in its own, where the
    // lane admits it.
    // A derived class constructor is pushed too (`Vm::inline_derived_construct_target`): its
    // frame binds `this` at `super(...)`, and its pending call applies the derived return
    // rules (`CallCompletion::DerivedConstructor`), where it left the lane before.
    let admits = |body: &quench_runtime::bytecode::Function| Vm::inline_constructible(body) || Vm::inline_derived_constructible(body);
    let pushable = match vm.function_record(callee) {
        Some(record @ FunctionRecord { kind: FunctionKind::User(program, id), .. }) => {
            vm.record_realm(record) == vm.realm.globals
                && if *program == unsafe { (*cx).program } {
                    code.function_get(*id as usize).is_some_and(admits)
                } else {
                    vm.programs.residual(*program).is_some_and(|callee_code| {
                        vm.admits_in_lane(*program, callee_code)
                            && callee_code
                                .function_get(*id as usize)
                                .is_some_and(admits)
                    })
                }
        }
        _ => false,
    };
    if !pushable
        || !quench_runtime::staged::call_construct::concrete_admits_construct(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }, vm.construct_target.is_some())
        || returns && !returning_call_admitted(cx)
    {
        return exit(ip);
    }
    vm.materialize_lane_activations(cx);
    // The caller is now the innermost frame, its registers the frame's.
    let (frame, r) = unsafe { ((*cx).frame(), (*cx).registers) };
    let target = match vm.inline_construct_target(code, frame, pc, callee) {
        Some((program, id, env, this)) => Some((program, id, env, this, CallCompletion::Constructor(this))),
        None => vm
            .inline_derived_construct_target(code, frame, callee)
            .map(|(program, id, env)| (program, id, env, Value::UNDEFINED, CallCompletion::DerivedConstructor)),
    };
    let Some((program, id, env, this, completion)) = target else {
        return exit(ip);
    };
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    // SAFETY: the construction's window is registers of the caller frame.
    let arguments = std::ptr::slice_from_raw_parts(unsafe { r.0.add(usize::from(i.c)) }, argc);
    derived_frames::push_construction_frame(cx, ip, i, frame, (program, id, env), (callee, callee), (this, completion), arguments)
}

/// Continue in JavaScript frame `frame` at its resume point, with the lane's per-frame
/// facts for it: `new` activates in place only from an ordinary body
/// (`LaneContext::script_constructs`).
#[inline(always)]
pub(in super::super) fn enter_script_frame_in_lane(cx: *mut LaneContext, frame: usize) -> Resumed {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let function = vm.frames[frame].function;
    unsafe {
        (*cx).script_constructs = code
            .function_get(function as usize)
            .is_some_and(|function| function.dispatch == quench_runtime::bytecode::DispatchClass::General);
    }
    enter_frame_in_lane(cx, frame, vm.frames[frame].pc)
}

/// The return of the running JavaScript frame with `value` to the caller frame the general
/// loop's pending call names, made in place as that loop makes it: the frame retires, an
/// in-loop `new` substitutes its receiver for a non-object result, and the caller continues
/// with the value in the call's register. A call that returns its callee's completion
/// completes its caller in turn: another pending caller here, or the loop's own frame,
/// which the general loop returns from at that call (`frame_return`). A call into
/// another program returns to the caller's, as the loop's return restores it.
#[inline(never)]
pub(in super::super) fn return_script_frame_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    mut value: Value,
    mut retired: bool,
) -> Ip {
    let vm = unsafe { vm(cx) };
    let pending = unsafe { &mut *(*cx).pending };
    let mut frame = unsafe { (*cx).frame() };
    let mut exit_ip = ip;
    loop {
        // An async callee's promise settles on the general path (`PendingGeneralCall::completion`);
        // a generator whose activation became a frame finishes here as there (`PendingGeneralCall::completion`).
        // A derived constructor's construction completes here only where it throws nothing
        // (`Vm::derived_construct_completes`); its errors are raised at the call on the general path.
        let returns_here = pending.last().is_some_and(|call| {
            lane_completes_return(cx, call, frame, retired)
                && (!matches!(call.completion, CallCompletion::DerivedConstructor)
                    || vm.derived_construct_completes(value, vm.frames[frame].this))
        });
        if !returns_here {
            // A record that has not run yet runs again on the general path; one that ran (a
            // call made, `retired`) completes the loop's own frame with the value there.
            if !retired {
                return exit(exit_ip);
            }
            return frame_return(vm, exit_ip, value);
        }
        let mut call = pending.pop().expect("a pending call names the returning frame");
        let completed = vm.frames.pop().expect("the returning frame is the innermost");
        // Checked above: the returned object, else the `this` binding `super(...)` made.
        if matches!(call.completion, CallCompletion::DerivedConstructor) && !vm.is_object_like(value) {
            value = completed.this;
        }
        // Retired as the general loop's return retires it (`Vm::retire_returned_frame`).
        let (retired_value, generator) = vm.retire_returned_frame(completed, &mut call.completion, value, None);
        value = retired_value;
        if generator {
            note_allocation(vm);
        }
        retired = true;
        // A call into another program returns to the caller's, which the loop executes again.
        if let Some(saved) = call.caller_program.take() {
            // SAFETY: the general loop's executing program outlives the lane run.
            let executing = unsafe { &mut *(*cx).executing };
            executing.id = saved.id;
            executing.retained = saved.retained;
            enter_program_in_lane(cx, saved.id);
        }
        drop(call.call_guard);
        frame = call.caller;
        // The completion may read the caller's register (`CallCompletion::Assigned`), which
        // names the register file `active_registers` holds.
        vm.active_registers = vm.frames[frame].registers.as_mut_ptr();
        value = vm.completed_call_value(&call.completion, frame, call.destination, value);
        if !call.returns {
            vm.write(frame, call.destination, value);
            return enter_script_frame_in_lane(cx, frame).into_loop();
        }
        // The caller completes too, at its call record.
        let (_, call_ip, _) = frame_state_in_lane(cx, frame, call.call_pc as usize);
        exit_ip = call_ip;
    }
}

/// Return from a frame the general loop or a frame push made. Out of line
/// so the activation return saves no registers.
#[cold]
#[inline(never)]
pub(in super::super) fn return_frame_in_lane(r: Registers, ip: Ip) -> Resumed {
    let cx = context_in_lane();
    // SAFETY: called only while a lane runs, from its return handler.
    let i = unsafe { &*ip };
    let vm = unsafe { vm(cx) };
    if unsafe { (*cx).returns_script_frame } {
        return Resumed::Exit(return_script_frame_in_lane(cx, ip, r.get(i.a), false));
    }
    let pending = unsafe { &mut *(*cx).pending };
    #[cfg(feature = "wasm")]
    if unsafe { (*cx).inline_calls }
        && pending.last().is_some_and(|call| {
            matches!(call.completion, CallCompletion::WasmExport(_)) && call.caller + 2 == vm.frames.len()
        })
    {
        return lane_return_wasm_export(cx, r.get(i.a));
    }
    // An in-loop `new` substitutes its receiver for a non-object result;
    // the general path owns that [[Construct]] step.
    if !unsafe { (*cx).inline_calls }
        || pending.last().is_none_or(|call| {
            !matches!(call.completion, CallCompletion::Value)
                || call.caller_program.is_some()
        })
    {
        return Resumed::Exit(exit(ip));
    }
    let value = r.get(i.a);
    let Some(call) = pending.pop() else {
        return Resumed::Exit(exit(ip));
    };
    vm.retire_wasm_frame();
    vm.active_registers = vm.frames[call.caller].registers.as_mut_ptr();
    vm.write(call.caller, call.destination, value);
    drop(call.call_guard);
    let pc = vm.frames[call.caller].pc;
    enter_frame_in_lane(cx, call.caller, pc)
}

/// The return of an exported function's frame a JavaScript call pushed
/// (`lane_wasm_export_frame`) with its encoded `value`: the frame retires, its caller's
/// program runs again as the run's JavaScript (`LaneContext::script_calls`), and the call's
/// register takes the value's JavaScript form (`CallCompletion::WasmExport`), or, for a call
/// that completes its caller, the caller returns it.
#[cold]
#[inline(never)]
pub(in super::super) fn lane_return_wasm_export(cx: *mut LaneContext, value: Value) -> Resumed {
    // The record is unused: a frame's return reads only its pending call.
    quench_runtime::staged::wasm_activation::step_export_frame_return_of(&mut LaneExportCall { cx, r: Registers(std::ptr::null_mut()), ip: std::ptr::null(), i: std::ptr::null(), call: None }, value)
}
