//! Call windows of a lane: entering a call's window, counting a proved entry, entering a plain window, and pushing a callback's activation.
use super::*;

/// `enter_window_in_lane` with the VM its caller already holds, so the push reads the lane
/// stack through it rather than through the context again after the window's writes.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn enter_window_of_in_lane<const GENERAL: bool, const SETTLES: bool>(
    cx: *mut LaneContext,
    vm: &mut Vm,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    argc: u16,
    this: Value, constructs: bool,
    // The register of the first value a keeping activation kept past its registers
    // (`ScriptActivation::kept`), which marks it forwarding; `None` for any other.
    kept: Option<u16>,
) -> (Registers, Ip) {
    // The receiver goes to its context word first, so it holds no register past the push.
    callee_registers.set_this(this);
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // Only a plain call activates another program's callee (`Vm::resolve_script_call_target`).
    let cross = crosses_in_lane::<GENERAL>(cx, target);
    // The keeping row's record words (`staged::call_keeping::KeepingRecord`).
    let record = quench_runtime::staged::call_keeping::KeepingRecord::of_kept(argc, kept);
    let activation = Activation::of_parts(
        target.body.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.closure.callee,
            argc: record.argc,
            completion: Completion::of(constructs, cross, SETTLES),
            forwards: record.forwards,
            kept: record.kept,
            environment: false,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation, callee_registers.0) };
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.body.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    callee_registers.set_env(target.closure.env);
    callee_registers.set_capture(target.closure.capture);
    if cross {
        enter_program_in_lane(cx, target.body.program());
    }
    // A construction runs its body from the first record; the tier enters plain calls.
    let start = if constructs { base } else { unsafe { (*target.body.view).start() } };
    (callee_registers, start)
}

/// `enter_window_of_in_lane` of a proved plain call (`plain_call_activation` past `PLAIN_LINK`):
/// the record it would build is always plain (no general form, so no crossing; no
/// construction, settling, keeping or forwarding; an empty native prefix; a lane stack
/// window), so its header goes below the window straight from the call's words
/// (`protocol::push_plain_at`), with no `Activation` value and no `LaneStack::header_flags`
/// test. `depth` is the plain depth `LaneStack::plain_room` admitted it at.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn enter_plain_window(
    cx: *mut LaneContext,
    vm: &mut Vm,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    callee_registers: Registers,
    depth: usize,
    argc: u16,
    this: Value,
) -> (Registers, Ip) {
    use quench_runtime::staged::protocol::{push_plain_at, PlainHeader};
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: the running lane's context.
    let (caller_base, program) = unsafe { ((*cx).base, (*cx).program) };
    debug_assert!(depth == stack.plain.depth.get());
    debug_assert!(stack.plain_program_holds(program));
    debug_assert!(callee_registers.0.addr() & HEAP_WINDOW == 0, "a lane stack window");
    // The record `enter_window_of_in_lane` builds here keeps `argc` (`KeepingRecord::of_kept`
    // without kept values) and completes `Completion::of(false, false, false)`, `Plain`.
    let header = PlainHeader::new(
        ip.expose_provenance() as u64, r.0.expose_provenance() as u64, caller_base.expose_provenance() as u64,
        target.body.function, argc, 0, target.closure.callee.to_bits(),
    );
    push_plain_at(
        &mut quench_runtime::staged::Concrete::pure(),
        stack.plain_frames_word() as u64, callee_registers.0.expose_provenance() as u64, header, u64::from(program.raw()), depth as u64,
    );
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.body.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    quench_runtime::staged::call_keeping::enter_context(
        &mut quench_runtime::staged::Concrete::pure(), callee_registers.0.expose_provenance() as u64,
        this.to_bits(), target.closure.env.to_bits(), target.closure.capture.expose_provenance() as u64,
    );
    (callee_registers, unsafe { (*target.body.view).start() })
}

/// The frameless target of another program's function `callee` a native calls from the
/// lane (`Vm::native_callback_in_lane`), as a trap's (`AccessorCall::trap_target`).
pub(in super::super) fn cross_callback_target_in_lane(cx: *mut LaneContext, callee: Value) -> Option<(ScriptCallTarget, bool)> {
    AccessorCall::of(cx).trap_target(callee).filter(|&(_, crosses)| crosses)
}

/// The activation of a native's call of `target` with `args` from the lane's running
/// function (`Vm::native_callback_in_lane`), as the lane's own call would make it from the call
/// record `call`, completing to the native (`Completion::ReturnsToNative`): by its shape
/// (`PreparedShape`), or, for a gathering target, whose rest array it makes, or a call
/// without room, which grows the stack for the general path it takes, as
/// `activate_keeping_in_lane` makes it. Its first record, or `None` where the lane would not
/// activate the call.
pub(in super::super) fn push_callback_in_lane(
    cx: *mut LaneContext,
    call: Ip,
    target: &ScriptCallTarget,
    args: &[Value],
    this: Value,
    crosses: bool,
) -> Option<Ip> {
    debug_assert_eq!(crosses, crosses_in_lane::<true>(cx, target), "a native's lane callee crosses where its program is another");
    if !crosses {
        // SAFETY: the running lane's context, which nothing else accesses.
        let shape = PreparedShape::of(unsafe { &*cx }, call, target, args.len());
        if let Some(shape) = &shape
            && let Some(start) = push_prepared_in_lane(cx, shape, &shape.entry(target, this), args)
        {
            return Some(start);
        }
    }
    let (r, arguments) = (unsafe { (*cx).registers }, ActivationArguments::Kept(args.as_ptr(), args.len()));
    let (_, start) = if crosses {
        activate_keeping_in_lane::<true>(cx, r, call, target, arguments, this, false, None).ok()?
    } else {
        activate_keeping_in_lane::<false>(cx, r, call, target, arguments, this, false, None).ok()?
    };
    let vm = unsafe { vm(cx) };
    // Given its completion where it is, a header's side record or the record (no spill).
    vm.lane_stack.recomplete_top(Completion::ReturnsToNative);
    Some(start)
}
