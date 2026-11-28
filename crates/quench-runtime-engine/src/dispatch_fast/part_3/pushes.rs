//! Pushing activations and cell bodies into a lane: the exits a raised call resumes at, the push of a call's activation or passed window, the entry of a cell body, and the filling and entering of a call window.
use super::*;

/// Leaves the lane at `record`, where the running activation's body raised the exception in
/// `Vm::lane_raised`: the general loop unwinds from there, from the frames frameless
/// activations become first (`in_place_in_lane`).
pub(in super::super) fn raised_exit_in_lane(cx: *mut LaneContext, record: Ip) -> Ip {
    let vm = unsafe { vm(cx) };
    let (function, throw_pc) = crate::compiled::raise_site_parts(vm.lane_raise_site);
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(error) = vm.lane_completion.raised_mut() { error.capture_source_location(code, function, throw_pc); }
    if let Some(settled) = raised_windows::settle_raised_window(cx, record) {
        return settled;
    }
    if vm.lane_stack.has_frames() {
        // SAFETY: `record` is one of the running function's records.
        let call_pc = unsafe { record_pc(cx, record) } as usize;
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
        vm.materialize_deferred_lane();
    }
    exit(record)
}

/// Where the lane continues after a body left at `pc` of `own`'s function on registers `r`: at
/// the record, with the accumulator that record reads.
pub(in super::super) fn body_resume_at_in_lane(cx: *mut LaneContext, r: Registers, own: &LaneView, pc: usize) -> LaneBodyLeft {
    let accumulator = own.accumulator_registers.get(pc).copied().filter(|&register| register != NO_ACCUMULATOR);
    let mut carried = Carried::script(cx);
    if let Some(register) = accumulator {
        carried.acc = Slot::of(r.get(register));
    }
    // SAFETY: the body leaves at a PC of the function, whose records are contiguous.
    LaneBodyLeft::Resume(r, unsafe { (*cx).base.add(pc) }, carried)
}

/// Whether activating `target` leaves the running program. A target from
/// `LaneStack::script_targets` never does: `Vm::script_call_holds` admitted it for the VM's
/// active program, which is the lane's (`enter_program_in_lane` sets both), so the fast call
/// forms take that fact as proven (`MAY_CROSS` false) and only the general activation asks.
#[inline(always)]
pub(in super::super) fn crosses_in_lane<const MAY_CROSS: bool>(cx: *mut LaneContext, target: &ScriptCallTarget) -> bool {
    MAY_CROSS && target.program() != unsafe { (*cx).program }
}

/// `activate_in_lane` past its checks: the call passes at most `target`'s parameters, the lane
/// stack has room (`activation_room_in_lane`), and the call may complete its caller. Pushes
/// the activation and returns its registers and the callee's first record; it makes no
/// call, so a handler activating through it keeps its registers. Without `GENERAL` its
/// callers (the fast call forms) have sent another program's target to the general form
/// (`crosses_in_lane`).
#[inline(always)]
pub(in super::super) fn push_activation_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // SAFETY: `ip` is a record of the running view, whose window of `r` holds its values.
    let (passed, argc) = unsafe { (r.0.add(usize::from(arguments)).cast_const(), ImmediateLayout::argument_count((*ip).imm())) };
    push_passed_in_lane::<GENERAL, SETTLES>(cx, r, ip, target, passed, argc, this, constructs)
}

/// `push_activation_in_lane` of the `argc` values at `passed`, at most `target`'s parameters
/// and `FIXED_ARGUMENT_COPIES`, for the call record at `ip`; `SETTLES` for a settling
/// target's call (`Completion::SettlesPromise`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn push_passed_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: *const Value,
    argc: u16,
    this: Value,
    constructs: bool,
) -> (Registers, Ip) {
    // Only `script_call_settles_miss` activates a settling target, and it says so statically,
    // so the other call forms decide the completion without reading the target's entry.
    debug_assert_eq!(SETTLES, target.settles(), "a settling target activates through its own call form");
    let vm = unsafe { vm(cx) };
    let callee_registers = fill_window_in_lane(vm, target, passed, argc);
    enter_window_of_in_lane::<GENERAL, SETTLES, true>(cx, vm, r, ip, target, callee_registers, argc, this, constructs, None)
}

/// A plain call of `target`, of the running program, passed the `argc` values at `passed` (at
/// most its parameters) and not completing its caller, whose entry cell holds a published body
/// (`LaneView::cell_method`): the body runs on the activation's window before the activation
/// exists. The call is a leaf the context holds (`CONTEXT_LEAF`: the caller's receiver and
/// environment are the leaf's to restore), so a body that returns completes the call with the
/// caller's registers and the next record, nothing pushed and nothing to pop; a leave, and any
/// helper that observes activations (`materialize_running_leaf_in_lane`), make the activation the
/// push would have made, in the room the run reserved for it (`CALLEE_ACTIVATION_PENDING`),
/// and resume as the body's prologue would (`body_concluded_in_lane`). `None`, which changes
/// nothing, where the body does not run: the lane cannot make the activations a leave inside
/// its inlined calls would (`body_admits_in_lane`).
#[inline(never)]
pub(in super::super) fn enter_cell_body_in_lane(
    cx: *mut LaneContext,
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
    let window = fill_window_in_lane(vm, target, passed, argc);
    if !body_admits_in_lane(cx, method, CALLEE_ACTIVATION_PENDING) {
        // SAFETY: the window starts at the stack's top, which the fill moved.
        unsafe { vm.lane_stack.values.set_top(window.0) };
        return None;
    }
    begin_cell_leaf_in_lane(cx, ip, r, target, argc, this, window);
    let run = crate::compiled::BodyRun {
        method, direct_calls: direct_calls_in_lane(cx), program: unsafe { (*cx).program }, residual: unsafe { &*(*cx).code },
        registers: window.0, receiver: entry_receiver_in_lane(cx, own.binds_this()), captures: capture_reads(target.capture), lane_context: cx.cast(),
        pending: CALLEE_ACTIVATION_PENDING, bases_stand: false };
    Some(cell_body_ended_in_lane(cx, window, own, target.function, method, vm.run_lane_body(&run)))
}

/// A frameless call of `target` (`CallEntry::frameless(Frameless::Native)`), passed the `argc` values at `passed`
/// (the call's argument window), whose body's compiled entry runs with no activation and no frame. The window is
/// the lane stack's top, filled from the arguments (`fill_window_in_lane`): the caller's registers past its call
/// window are not known dead (liveness reads the window alone), so the body may not write through them. The body
/// is entered as `enter_cell_body_in_lane` enters one, minus the activation: a frameless callee leaves nothing, so
/// nothing observes the window and the stack's top is restored at its return. Arguments past the parameters are
/// dropped, which a frameless body observes nowhere (an `arguments` object or rest array allocates).
/// `None`, which changes nothing, where the cell holds no body (it retired since the target resolved), the lane has
/// no room, or a `with` scope is entered: the call then takes its general form.
#[inline(never)]
pub(in super::super) fn native_call_in_lane(cx: *mut LaneContext, target: &ScriptCallTarget, passed: *const Value, argc: u16, this: Value) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let own = unsafe { &*target.view };
    let method = own.cell_method()?;
    if vm.with_scope_entered || !activation_room_in_lane(vm, target) || !body_admits_in_lane(cx, method, NO_PENDING_ACTIVATIONS) {
        return None;
    }
    let window = fill_window_in_lane(vm, target, passed, argc.min(target.params));
    let receiver = if own.binds_this() { body_calls::body_this_in_lane(target, this) } else { Value::DELETED };
    let run = crate::compiled::BodyRun {
        method, direct_calls: direct_calls_in_lane(cx), program: unsafe { (*cx).program }, residual: unsafe { &*(*cx).code },
        registers: window.0, receiver, captures: capture_reads(target.capture), lane_context: cx.cast(),
        pending: NO_PENDING_ACTIVATIONS, bases_stand: false };
    let exit = vm.run_lane_body(&run);
    // SAFETY: the window is the stack's top the fill moved; a returned call leaves the stack as it found it.
    unsafe { vm.lane_stack.values.set_top(window.0) };
    record_end_in_lane(cx, own, method, exit);
    match exit {
        crate::compiled::LaneExit::Returned { value, .. } => Some(value),
        _ => unreachable!("a frameless body leaves nothing: its call form's leaves are Never"),
    }
}

/// Makes the call of `target` the context's running leaf (`CONTEXT_LEAF`), which holds what its
/// activation would (`materialize_leaf_in_lane`): the callee's receiver, environment, captures,
/// registers and records are the context's, the caller's are the leaf's; the entry is counted.
#[inline(always)]
pub(in super::super) fn begin_cell_leaf_in_lane(cx: *mut LaneContext, ip: Ip, r: Registers, target: &ScriptCallTarget, argc: u16, this: Value, window: Registers) {
    unsafe { vm(cx) }.programs.record_js_jit_lane_entry(target.program(), target.function);
    unsafe { vm(cx) }.programs.tier_events().record(Event::CellCrossed);
    // SAFETY: the running lane's context, and the callee's view outlives its program.
    // The staged leaf row (`call_leaf::enter_leaf`) writes the record, the receiver and the
    // environment, all the callee's in `CONTEXT_LEAF`.
    unsafe {
        lane_enter_leaf(cx, leaf_calls::FilledWindow, window, ip, r, target, CONTEXT_LEAF, argc, this, None);
    }
}

/// Where the lane continues after the body `enter_cell_body_in_lane` ran on `window` ended at
/// `exit`: the caller, past the call, where the body returned and the leaf still runs; else
/// the leave, in the activation made now. A helper that pushed it resolved targets since the
/// call, so the callee is `own` and `function`, not the call's target.
#[inline(always)]
pub(in super::super) fn cell_body_ended_in_lane(
    cx: *mut LaneContext,
    window: Registers,
    own: &LaneView,
    function: u32,
    method: &crate::compiled::LaneMethod,
    exit: crate::compiled::LaneExit,
) -> Resumed {
    if let crate::compiled::LaneExit::Returned { value, .. } = exit
        && !unsafe { (*cx).leaf.call }.is_null()
    {
        // SAFETY: the window held the callee's registers; the completed call leaves the stack as it found it.
        unsafe {
            (*cx).callee_warming = false;
            vm(cx).lane_stack.values.set_top(window.0);
        }
        record_end_in_lane(cx, own, method, exit);
        let (registers, next) = complete_leaf_in_lane(cx, value, CONTEXT_LEAF).expect("the leaf runs until it completes or materializes");
        return Resumed::At(registers, next, Carried::script(cx));
    }
    materialize_running_leaf_in_lane(cx);
    body_concluded_in_lane(cx, window, own.start(), own, method, exit)
}

/// The window of `target`'s plain activation at the lane stack's top, holding the `argc`
/// values at `passed` as its parameters, which the stack's values cover from here on.
#[inline(always)]
pub(in super::super) fn fill_window_in_lane(vm: &mut Vm, target: &ScriptCallTarget, passed: *const Value, argc: u16) -> Registers {
    let count = usize::from(target.registers);
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window's admitted room (`ScriptCallTarget::room`); the
    // registers live at entry are written before the length covers them, and no activation's
    // window moves. A register past `fill` is written before any instruction reads it, and
    // a collection reads it only where liveness proves it live (`ResidualProgram::entry_fill`).
    // Parameter `k` lives in register `k` of a plain activation (`Vm::place_plain_parameters`).
    unsafe {
        let registers = stack.values.top();
        fill_admitted_window(registers, usize::from(target.fill), count);
        copy_arguments(registers, passed, argc);
        stack.values.set_top(registers.add(count + lane_values::CONTEXT));
        Registers(registers)
    }
}

/// `push_passed_in_lane` past its window, which the stack's values cover from `window` on and
/// which holds the callee's registers: the activation, and the context made the callee's.
/// A native frame's materialization enters its window here too (`native_frames::advance_into_frames`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn enter_window_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    argc: u16,
    this: Value, constructs: bool,
    kept: Option<u16>,
) -> (Registers, Ip) {
    enter_window_of_in_lane::<GENERAL, SETTLES, true>(cx, unsafe { vm(cx) }, r, ip, target, callee_registers, argc, this, constructs, kept)
}
