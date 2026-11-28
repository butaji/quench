//! Activation of script calls in a lane: the plain activation of a rest body that keeps nothing, the script target a call resolves to, and the activation and reuse of a frame for it.
use super::*;

/// The plain activation (`push_passed_in_lane`) of a rest body of the running program that keeps
/// nothing (`TARGET_GATHERS_KEEPING` clear), passed the `argc` values at `passed` (its window
/// of `r`, or the elements of the array a spread call passes): the fixed parameters copied as
/// any call's are, and the rest parameter's register the array of the values past them, read
/// in place, or `undefined` for a rest view that keeps the values after its registers.
#[inline(always)]
pub(in super::super) fn activate_gathering_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if (returns && !returning_call_admitted(cx)) || vm.with_scope_entered {
        return Err(exit(ip));
    }
    let view = target.body.forms & TARGET_REST_VIEW != 0;
    let room = usize::from(target.body.room) + if view { usize::from(argc) } else { 0 };
    if !quench_runtime::staged::call_keeping::concrete_admits_keeping(
        vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), room,
    ) {
        return Err(script_call_frame_in_lane(ip, room));
    }
    let fixed = target.body.params - 1;
    if view {
        // A rest view keeps every value passed after its registers, as a keeping activation
        // does, and its rest register holds `undefined` (`LaneFrame::rest_view`).
        let callee_registers = fill_window_in_lane(vm, target, passed, argc.min(fixed));
        callee_registers.set(fixed, Value::UNDEFINED);
        let count = usize::from(target.body.registers);
        // SAFETY: the room checked above holds the kept values past the window.
        unsafe {
            quench_runtime::staged::call_keeping::keep_passed(
                &mut quench_runtime::staged::Concrete::pure(), callee_registers.0.add(count).expose_provenance() as u64,
                passed.expose_provenance() as u64, argc,
            );
            vm.lane_stack.values.set_top(callee_registers.0.add(count + usize::from(argc) + lane_values::CONTEXT));
        }
        return Ok(enter_window_in_lane::<false, false>(cx, r, ip, target, callee_registers, argc, this, false, Some(target.body.registers)));
    }
    // The entry, which callers read in place, is copied before the rest array allocates.
    let target = &{ *target };
    let rest = vm.rest_parameter_array(unsafe {
        std::slice::from_raw_parts(passed.add(usize::from(fixed.min(argc))), usize::from(argc.saturating_sub(fixed)))
    });
    let callee_registers = fill_window_in_lane(vm, target, passed, argc.min(fixed));
    callee_registers.set(fixed, rest);
    Ok(enter_window_in_lane::<false, false>(cx, r, ip, target, callee_registers, argc.min(target.body.params), this, false, None))
}

#[inline(always)]
pub(in super::super) fn script_target_in_lane(cx: *mut LaneContext, callee: Value) -> Option<ScriptCallTarget> {
    script_target_in_in_lane(cx, callee, &mut None).copied()
}

/// `script_target_in_lane`'s target, read in place where the cache holds it, else resolved
/// into `resolved`. The cache's entry holds until the next resolution or collection.
#[inline(always)]
pub(in super::super) fn script_target_in_in_lane(
    cx: *mut LaneContext,
    callee: Value,
    resolved: &mut Option<ScriptCallTarget>,
) -> Option<&ScriptCallTarget> {
    let vm = unsafe { vm(cx) };
    // The paths besides a plain call (constructions, getters, forwarded and applied calls)
    // activate callees of the running program alone. Another program's callee, once
    // resolved, is held apart (`cross_call_entry`) for the callers that activate across
    // programs; resolving it again would only find it so.
    // The cached entry is tested and used in place; `script_call_holds` admitted it for the
    // active program already.
    if let Some(entry) = vm.script_call_entry(callee) {
        // SAFETY: the entry is read before anything else touches the cache.
        let target = unsafe { &*entry };
        return admits_activation_in_lane(target).then_some(target);
    }
    // A cached entry or cross entry is admitted by its link's compare, which refuses under
    // any condition (no script calls, a construction pending); a resolve makes no compare, so
    // the conditions refuse it here, once, for every unlinked caller.
    if vm.lane_stack.refuses_any() || vm.cross_call_entry(callee).is_some() {
        return None;
    }
    let active = vm.active_program;
    *resolved = vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee)
        .filter(|target| target.body.program() == active && admits_activation_in_lane(target));
    resolved.as_ref()
}

/// The instance environment the running Wasm function reads its bindings from: the
/// export's (`LaneContext::instance`) above a JavaScript call of it, else its frame's.
#[inline(always)]
pub(in super::super) fn instance_env_in_lane(cx: *mut LaneContext) -> Value {
    // SAFETY: the context outlives the lane run.
    let instance = unsafe { (*cx).instance };
    if !instance.is_deleted() {
        return instance;
    }
    unsafe { vm(cx).frames[(*cx).frame()].env }
}

/// Make `program` the lane's running program, as a call into or a return to one of its
/// functions makes it the general loop's executing one: the residual, lane views and
/// constants its records read, and the VM's active program, whose cache layout its sites
/// index.
#[inline(always)]
pub(in super::super) fn enter_program_in_lane(cx: *mut LaneContext, program: ProgramId) {
    let vm = unsafe { vm(cx) };
    let context = vm.program_context_in_lane(program);
    vm.activate_context(program, context);
    // The innermost plain activation's caller program (`PlainFrames::program`) is the program
    // the next plain push runs under. An emitted direct call bumps the plain depth without
    // writing it, so after a crossing push (whose side record keeps its own caller program)
    // the word must name the entered program, or a plain frame pushed above it by emitted
    // code reads its caller as the crossing frame's caller program.
    vm.lane_stack.plain.program.set(program);
    // SAFETY: the context outlives the lane run; the store keeps the program's residual,
    // views and constants for it, and its context names them.
    unsafe {
        (*cx).code = (*context).code;
        (*cx).program = program;
        (*cx).views = (*context).views;
        (*cx).constants = (*context).constants;
    }
}

/// Activate `target`, which keeps no passed values (callers route those to
/// `activate_keeping_in_lane`), for the call record at `ip` with the arguments from register
/// `arguments` on: the activation's registers and the callee's
/// first record, or the record to continue at when the call takes the general path.
#[inline(always)]
pub(in super::super) fn activate_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
    constructs: bool,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    let argc = ImmediateLayout::argument_count(i.imm());
    // A call that returns its callee's completion completes its caller too, which only an
    // activation's return does in the lane. A proper tail call of an ordinary callee reuses the
    // running frame (`tail_reuse_admitted_in_lane`); any other strict caller's terminal call exits
    // to the general dispatch path, which takes it as a jump.
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && tail_reuse_admitted_in_lane(cx, r, ip, target, argc, constructs) {
        let passed = unsafe { r.0.add(usize::from(arguments)).cast_const() };
        return Ok(reuse_frame_in_lane(cx, r, target, passed, argc, this));
    }
    if returns && !returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    if vm.with_scope_entered {
        return Err(exit(ip));
    }
    // Values past the parameters stay observable (legacy `f.arguments`, a frame made of the
    // activation): the keeping activation holds them after the registers. A callee no legacy
    // reflection observes is passed its parameters alone, as its frame would keep them.
    if argc > target.body.params && (!target.body.drops_extra_arguments() || target.body.forwards()) {
        // A call keeping every value it was passed, a rest body's array aside, takes the plain
        // push with the values after its registers.
        if !constructs && target.body.entry != CallEntry::framed(Framed::Gathers) {
            return activate_keeping_window_in_lane::<true>(cx, r, ip, target, arguments, this);
        }
        return activate_keeping_in_lane::<true>(
            cx,
            r,
            ip,
            target,
            ActivationArguments::Window(arguments),
            this,
            constructs,
            None,
        );
    }
    if !activation_room_in_lane(vm, target) {
        return Err(script_call_frame_in_lane(ip, usize::from(target.body.room)));
    }
    // SAFETY: the call's window of `r` holds its `argc` values.
    let passed = unsafe { r.0.add(usize::from(arguments)).cast_const() };
    Ok(push_passed_in_lane::<true, false>(cx, r, ip, target, passed, argc.min(target.body.params), this, constructs))
}

/// Whether the lane stack takes `target`'s activation: capacity, the guest-call budget and
/// a due collection (`LaneStack::activation_limit`).
#[inline(always)]
pub(in super::super) fn activation_room_in_lane(vm: &Vm, target: &ScriptCallTarget) -> bool {
    vm.lane_stack.admits_activations(1)
        && quench_runtime::staged::call::concrete_fits(vm.lane_stack.values.room() as u64, u64::from(target.body.room))
}

/// Whether `target` takes `argc` arguments and the lane stack holds it: `staged::call::admit_room` run on words.
#[inline(always)]
pub(in super::super) fn admits_window_in_lane(vm: &Vm, target: &ScriptCallTarget, argc: u16) -> bool {
    quench_runtime::staged::call::concrete_fits(u64::from(target.body.params), u64::from(argc)) && activation_room_in_lane(vm, target)
}

/// Whether the returning call at `ip` of `target`, passing `argc` arguments from the window `r`, reuses the running frame
/// (`reuse_frame_in_lane`). It is a proper tail call (DESIGN.md 4.4): a tail site of the running body
/// (`Function::tail_site_permitted`, the bit the general dispatch path reads) whose callee is an ordinary body
/// (`CallPlan::tail_callee`) of the running program, passed no more than its parameters, which the running frame's
/// plain header holds over `r`. A leaf's window is not the frame's, so a leaf running in `r` takes the call's own path.
/// A callee that needs more room than the lane stack holds takes it too. Reads facts only; writes nothing.
#[inline(always)]
pub(in super::super) fn tail_reuse_admitted_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, target: &ScriptCallTarget, argc: u16, constructs: bool) -> bool {
    let vm = unsafe { vm(cx) };
    // SAFETY: the running lane's code; `ip` is a record of the running view, whose PC `record_pc` reads.
    let code = unsafe { &*(*cx).code };
    let caller_pc = unsafe { record_pc(cx, ip) } as u32;
    !constructs
        && !vm.with_scope_entered
        && unsafe { (*cx).leaf.call }.is_null()
        && target.body.program() == unsafe { (*cx).program }
        && argc <= target.body.params
        && argc <= FIXED_ARGUMENT_COPIES
        && target.body.entry != CallEntry::framed(Framed::Gathers)
        && !target.body.settles()
        && code.function(target.body.function as usize).call_plan(code).tail_callee
        && code.function(running_function_in_lane(cx) as usize).tail_site_permitted(caller_pc)
        && vm.lane_stack.plain.registers.get() == r.0
        && vm.lane_stack.top_frame().and_then(|frame| frame.completion()) == Some(Completion::Plain)
        && activation_room_in_lane(vm, target)
}

/// The running frame's reuse by a returning call `tail_reuse_admitted_in_lane` admits: the running frame's header is
/// replaced over the same window `r` by the callee's, built as `enter_window_of_in_lane` builds one. The call record,
/// caller window, caller base and caller program stay the running frame's, so the callee's return completes the
/// running frame's caller. The `argc` arguments move into the parameter registers through copies, since the window
/// overlaps them. Returns the callee's registers and first record, as a push does.
#[inline(always)]
pub(in super::super) fn reuse_frame_in_lane(cx: *mut LaneContext, r: Registers, target: &ScriptCallTarget, passed: *const Value, argc: u16, this: Value) -> (Registers, Ip) {
    let vm = unsafe { vm(cx) };
    let mut arguments = [Value::UNDEFINED; FIXED_ARGUMENT_COPIES as usize];
    // SAFETY: the call's window of `r` holds `argc` values, which are copied before the window is written.
    unsafe { std::ptr::copy_nonoverlapping(passed, arguments.as_mut_ptr(), usize::from(argc)) };
    let running = vm.lane_stack.pop_activation().expect("a reused frame is the running plain frame");
    let record = quench_runtime::staged::call_keeping::KeepingRecord::of_kept(argc, None);
    let activation = Activation::of_parts(
        target.body.function, running.call(), running.caller_base(), running.caller_registers, running.caller_program,
        ActivationKind::Script(ScriptActivation {
            callee: target.closure.callee, argc: record.argc, completion: Completion::Plain, forwards: record.forwards,
            kept: record.kept, environment: false, record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the pop freed the header's slot, so the push has its capacity; the header goes below `r`, over the popped one.
    unsafe { vm.lane_stack.push_activation(activation, r.0) };
    let count = usize::from(target.body.registers);
    // SAFETY: the window `r` is the stack's, with room for the callee's registers and context (`activation_room_in_lane`);
    // its registers are written before the stack's top covers them.
    unsafe {
        fill_admitted_window(r.0, usize::from(target.body.fill), count);
        copy_arguments(r.0, arguments.as_ptr(), argc);
        vm.lane_stack.values.set_top(r.0.add(count + lane_values::CONTEXT));
        ((*cx).registers, (*cx).base) = (r, (*target.body.view).records());
    }
    r.set_this(this);
    r.set_env(target.closure.env);
    r.set_capture(target.closure.capture);
    vm.programs.record_js_jit_lane_entry(target.body.program(), target.body.function);
    (r, unsafe { (*target.body.view).start() })
}
