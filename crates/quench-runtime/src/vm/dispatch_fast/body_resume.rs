//! Resuming lane bodies: reloading the bases a running body reads, and the frames a lane exit resumes.

use super::*;

/// Reloads the heap bases the running body's rows read through the payload (`Vm::refresh_body_bases`), as
/// every call, construct and step helper of the body ends: the callee's run or an allocation may have
/// moved them.
///
/// # Safety
/// `context` is the running lane's.
pub(in crate::vm) unsafe fn refresh_body_bases<H: Host>(context: u64) {
    let cx = context as usize as *mut LaneContext<H>;
    // SAFETY: the caller's contract.
    unsafe { vm(cx).refresh_body_bases() };
}

/// Names `payload`, the dispatch of the native loop region that runs on the activation of the lane
/// `context`, to the helpers its rows call (`lane_region_call`), and returns the one it replaces:
/// a region's calls run lane code that may enter regions of its own.
///
/// # Safety
/// `context` is the running lane's.
pub(in crate::vm) unsafe fn lane_region_payload<H: Host>(context: usize, payload: usize) -> usize {
    // SAFETY: the caller's contract.
    std::mem::replace(unsafe { &mut (*(context as *mut LaneContext<H>)).region_payload }, payload)
}

/// The helper a loop region's call row runs (`LaneHelper::RegionCall`): the call at `call_pc` of the
/// activation the lane context names, whose registers the region runs on, as `lane_body_call`
/// makes it, then the heap bases the region's payload holds reloaded (`LaneContext::region_payload`)
/// besides the body frame's, which the callee's run or an allocation may have moved.
///
/// # Safety
/// `context` is the running lane's, which the region entered (`lane_region_payload`), and `call_pc` a call record of its function.
pub(in crate::vm) unsafe extern "C" fn lane_region_call<H: Host>(vm: u64, residual: u64, context: u64, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    let status = unsafe { body_calls::lane_body_call::<H>(vm, residual, context, call_pc) };
    let cx = context as usize as *mut LaneContext<H>;
    // SAFETY: as above.
    unsafe { self::vm(cx).refresh_region_bases((*cx).region_payload, lane_direct_calls(cx)) };
    status
}

/// Leaves the lane at `record`, where the running activation's body raised the exception in
/// `Vm::lane_raised`: the general loop unwinds from there, from the frames frameless
/// activations become first (`lane_in_place`).
pub(super) fn lane_raised_exit<H: Host>(cx: *mut LaneContext<H>, record: Ip) -> Ip {
    let vm = unsafe { vm(cx) };
    let (function, throw_pc) = super::compiled::raise_site_parts(vm.lane_raise_site);
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(error) = vm.lane_raised.as_mut() { error.capture_source_location(code, function, throw_pc); }
    if let Some(settled) = raised_windows::settle_raised_window(cx, record) {
        return settled;
    }
    if !vm.lane_stack.activations.is_empty() {
        // SAFETY: `record` is one of the running function's records.
        let call_pc = unsafe { record.offset_from((*cx).base) } as usize;
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
        vm.materialize_deferred_lane();
    }
    exit(record)
}

/// Where the lane continues after a body left at `pc` of `own`'s function on registers `r`: in
/// the loop's published BaseJIT code where it has a resume stub for `pc` (`loop_resume`),
/// else at the record, with the accumulator that record reads.
pub(super) fn lane_body_resume_at<H: Host>(cx: *mut LaneContext<H>, r: Registers, own: &LaneView, pc: usize) -> LaneBodyLeft {
    let accumulator = own.accumulator_registers.get(pc).copied().filter(|&register| register != NO_ACCUMULATOR);
    if accumulator.is_none() && let Some(left) = loop_resume::lane_loop_resume(cx, r, own, pc) {
        return left;
    }
    let mut carried = Carried::script(cx);
    if let Some(register) = accumulator {
        carried.acc = Slot::of(r.get(register));
    }
    // SAFETY: the body leaves at a PC of the function, whose records are contiguous.
    LaneBodyLeft::Resume(r, unsafe { (*cx).base.add(pc) }, carried)
}

/// Whether activating `target` leaves the running program. A target from
/// `LaneStack::script_targets` never does: `Vm::script_call_holds` admitted it for the VM's
/// active program, which is the lane's (`lane_enter_program` sets both), so the fast call
/// forms take that fact as proven (`MAY_CROSS` false) and only the general activation asks.
#[inline(always)]
pub(super) fn lane_crosses<H: Host, const MAY_CROSS: bool>(cx: *mut LaneContext<H>, target: &ScriptCallTarget) -> bool {
    MAY_CROSS && target.program() != unsafe { (*cx).program }
}

/// `lane_activate` past its checks: the call passes at most `target`'s parameters, the lane
/// stack has room (`lane_activation_room`), and the call may complete its caller. Pushes
/// the activation and returns its registers and the callee's first record; it makes no
/// call, so a handler activating through it keeps its registers. Without `GENERAL` its
/// callers (the fast call forms) have sent another program's target to the general form
/// (`lane_crosses`).
#[inline(always)]
pub(super) fn lane_push_activation<H: Host, const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // SAFETY: `ip` is a record of the running view, whose window of `r` holds its values.
    let (passed, argc) = unsafe { (r.0.add(usize::from(arguments)).cast_const(), ImmediateLayout::argument_count((*ip).imm())) };
    lane_push_passed::<H, GENERAL, SETTLES>(cx, r, ip, target, passed, argc, this, constructs)
}

/// `lane_push_activation` of the `argc` values at `passed`, at most `target`'s parameters
/// and `FIXED_ARGUMENT_COPIES`, for the call record at `ip`; `SETTLES` for a settling
/// target's call (`Completion::SettlesPromise`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn lane_push_passed<H: Host, const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // Only `lane_script_call_settles` activates a settling target, and it says so statically,
    // so the other call forms decide the completion without reading the target's entry.
    debug_assert_eq!(SETTLES, target.settles(), "a settling target activates through its own call form");
    let callee_registers = lane_fill_window(unsafe { vm(cx) }, target, passed, argc);
    lane_enter_window::<H, GENERAL, SETTLES>(cx, r, ip, target, callee_registers, argc, this, constructs, None)
}

/// A plain call of `target`, of the running program, passed the `argc` values at `passed` (at
/// most its parameters) and not completing its caller, whose entry cell holds a published body
/// (`LaneView::cell_method`): the body runs on the activation's window before the activation
/// exists. The call is a leaf the context holds (`CONTEXT_LEAF`: the caller's receiver and
/// environment are the leaf's to restore), so a body that returns completes the call with the
/// caller's registers and the next record, nothing pushed and nothing to pop; a leave, and any
/// helper that observes activations (`lane_materialize_running_leaf`), make the activation the
/// push would have made, in the room the run reserved for it (`CALLEE_ACTIVATION_PENDING`),
/// and resume as the body's prologue would (`lane_body_concluded`). `None`, which changes
/// nothing, where the body does not run: the lane cannot make the activations a leave inside
/// its inlined calls would (`lane_body_admits`).
#[inline(never)]
pub(super) fn lane_enter_cell_body<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let own = unsafe { &*target.view };
    let method = own.cell_method()?;
    // Most entries of a published body come through its cell: one of the audit's interpreted
    // epoch activates the call as the lane's own would (`lane_body_audit`).
    if own.audit_entry(false) {
        return None;
    }
    let window = lane_fill_window(vm, target, passed, argc);
    if !lane_body_admits(cx, method, CALLEE_ACTIVATION_PENDING) {
        // SAFETY: the window starts at the stack's top, which the fill moved.
        unsafe { vm.lane_stack.values.set_top(window.0) };
        return None;
    }
    lane_begin_cell_leaf(cx, ip, r, target, argc, this, window);
    let run = lane_cell_body_run(cx, target, own, method, window);
    Some(lane_cell_body_ended(cx, window, own, target.function, method, vm.run_lane_body(&run)))
}

/// The run of `method`, the body of the callee `own` that `target` calls, over its window.
#[inline(always)]
pub(super) fn lane_cell_body_run<'a, H: Host>(cx: *mut LaneContext<H>, target: &ScriptCallTarget, own: &LaneView, method: &'a super::compiled::LaneMethod, window: Registers) -> super::compiled::BodyRun<'a> {
    super::compiled::BodyRun {
        method, direct_calls: lane_direct_calls(cx), program: unsafe { (*cx).program }, residual: unsafe { &*(*cx).code },
        registers: window.0, receiver: lane_entry_receiver(cx, own.binds_this()), captures: capture_reads(target.capture), lane_context: cx.cast(),
        pending: CALLEE_ACTIVATION_PENDING }
}

/// Makes the call of `target` the context's running leaf (`CONTEXT_LEAF`), which holds what its
/// activation would (`lane_materialize_leaf`): the callee's receiver, environment, captures,
/// registers and records are the context's, the caller's are the leaf's; the entry is counted.
#[inline(always)]
pub(super) fn lane_begin_cell_leaf<H: Host>(cx: *mut LaneContext<H>, ip: Ip, r: Registers, target: &ScriptCallTarget, argc: u16, this: Value, window: Registers) {
    unsafe { vm(cx) }.programs.record_js_jit_lane_entry(target.program(), target.function);
    unsafe { vm(cx) }.programs.tier_events().record(Event::CellCrossed);
    // SAFETY: the running lane's context, and the callee's view outlives its program.
    unsafe {
        (*cx).leaf = LeafCall {
            call: ip, form: CONTEXT_LEAF, argc, caller_registers: r, caller_base: (*cx).base, target, caller_program: None,
        };
        window.set_this(this);
        window.set_env(target.env);
        window.set_capture(target.capture);
        ((*cx).registers, (*cx).base) = (window, (*target.view).records());
    }
}

/// Where the lane continues after the body `lane_enter_cell_body` ran on `window` ended at
/// `exit`: the caller, past the call, where the body returned and the leaf still runs; else
/// the leave, in the activation made now. A helper that pushed it resolved targets since the
/// call, so the callee is `own` and `function`, not the call's target.
#[inline(always)]
pub(super) fn lane_cell_body_ended<H: Host>(
    cx: *mut LaneContext<H>,
    window: Registers,
    own: &LaneView,
    function: u32,
    method: &super::compiled::LaneMethod,
    exit: super::compiled::LaneExit,
) -> Resumed {
    if let super::compiled::LaneExit::Returned { value, .. } = exit
        && !unsafe { (*cx).leaf.call }.is_null()
    {
        // SAFETY: the window held the callee's registers; the completed call leaves the stack as it found it.
        unsafe {
            (*cx).callee_warming = false;
            vm(cx).lane_stack.values.set_top(window.0);
        }
        lane_record_end(cx, own, method, exit);
        lane_note_body_end(cx, own, exit, |_| {});
        let (registers, next) = lane_complete_leaf(cx, value, CONTEXT_LEAF).expect("the leaf runs until it completes or materializes");
        return Resumed::At(registers, next, Carried::script(cx));
    }
    lane_materialize_running_leaf(cx);
    lane_body_concluded::<H>(cx, window, own.start(), own, method, exit, function)
}

/// The window of `target`'s plain activation at the lane stack's top, holding the `argc`
/// values at `passed` as its parameters, which the stack's values cover from here on.
#[inline(always)]
pub(super) fn lane_fill_window<H: Host>(vm: &mut Vm<H>, target: &ScriptCallTarget, passed: *const Value, argc: u16) -> Registers {
    let count = usize::from(target.registers);
    let stack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window's admitted room (`ScriptCallTarget::room`); the
    // registers live at entry are written before the length covers them, and no activation's
    // window moves. A register past `fill` is written before any instruction reads it, and
    // a collection reads it only where liveness proves it live (`RegisterRoots::entry_fill`).
    // Parameter `k` lives in register `k` of a plain activation (`Vm::place_plain_parameters`).
    unsafe {
        let registers = stack.values.top();
        fill_admitted_window(registers, usize::from(target.fill), count);
        copy_arguments(registers, passed, argc);
        stack.values.set_top(registers.add(count + lane_values::CONTEXT));
        Registers(registers)
    }
}

/// Pushes the activation of a call of `target` made from `ip` by the caller's registers `r`, over
/// the window `callee_registers` at the top of the stack.
#[inline(always)]
fn push_window_activation<H: Host>(
    stack: &mut LaneStack,
    cx: *mut LaneContext<H>,
    (ip, r): (Ip, Registers),
    target: &ScriptCallTarget,
    (argc, completion): (u16, Completion),
    callee_registers: Registers,
) {
    let activation = Activation {
        native: crate::vm::compiled::ShadowFrame::EMPTY, function: target.function,
        call: ip,
        caller_base: unsafe { (*cx).base },
        caller_registers: r,
        caller_program: unsafe { (*cx).program },
        kind: ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc,
            completion,
            forwards: false,
            environment: false,
            record: std::ptr::null_mut(),
        }),
    };
    // SAFETY: the capacity check of the call leaves room for the activation.
    unsafe { stack.push_activation(activation, callee_registers.0) };
}

/// `lane_push_passed` past its window, which the stack's values cover from `window` on and
/// which holds the callee's registers: the activation, and the context made the callee's.
/// A native frame's materialization enters its window here too (`native_frames::advance_into_frames`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn lane_enter_window<H: Host, const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    argc: u16,
    this: Value,
    constructs: bool,
    skipped: Option<SkippedForwarder>,
) -> (Registers, Ip) {
    // The receiver goes to its context word first, so it holds no register past the push.
    callee_registers.set_this(this);
    let vm = unsafe { vm(cx) };
    let stack = &mut vm.lane_stack;
    // Only a plain call activates another program's callee (`Vm::resolve_script_call_target`).
    let cross = lane_crosses::<H, GENERAL>(cx, target);
    let completion = if skipped.is_some() { Completion::Forwarder } else { Completion::of(constructs, cross, SETTLES) };
    push_window_activation(stack, cx, (ip, r), target, (argc, completion), callee_registers);
    if let Some(skipped) = skipped { stack.forwarders.push(skipped); }
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    callee_registers.set_env(target.env);
    callee_registers.set_capture(target.capture);
    if cross {
        lane_enter_program(cx, target.program());
    }
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    // A construction runs its body from the first record; the tier enters plain calls.
    let start = if constructs { base } else { unsafe { (*target.view).start() } };
    (callee_registers, start)
}
