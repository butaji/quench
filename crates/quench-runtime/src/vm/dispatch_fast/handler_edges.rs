//! A native catch's frame state (`Lowering::bind_caught`): what the interpreter's handler entry
//! settles (`Vm::unwind_handler_scopes`, `Vm::bind_thrown_value`) that native stores cannot, made
//! by those operations over the home of the running function's state (`HandlerHome`): its VM
//! frame, or its activation's window. The interpreter's `throw` enters the same handlers in the
//! lane, unwinding plain activations where it is (`catch_raised`), so a raise makes no frame of
//! them unless the general loop must finish one.
use super::*;
use super::super::handler_entry::{HandlerHome, WindowHome};
use crate::bytecode::ThrownValueSlot;

/// Drops what the lane caches of the running frame's environment, which a copy of it replaced
/// (`Vm::clone_frame_environment`): its captures, its outer walk and its storage.
pub(super) fn drop_environment_caches(cx: *mut LaneContext) {
    // SAFETY: the running lane's context.
    unsafe {
        (*cx).registers.set_capture(std::ptr::null());
        (*cx).outer = NO_OUTER_WALK;
        (*cx).root_walk = NO_OUTER_WALK;
        (*cx).frame_storage = NO_FRAME_STORAGE;
    }
}

/// The running function, the home a handler's scopes and binding live in, and whether the running
/// function keeps its locals in its record: the innermost activation's, else the frame's.
fn handler_home(cx: *mut LaneContext, vm: &Vm) -> (u32, HandlerHome, bool) {
    match vm.lane_stack.top_frame() {
        Some(activation) => {
            let home = HandlerHome::Window(WindowHome {
                depth: vm.lane_stack.activation_count() - 1,
                program: unsafe { (*cx).program },
                function: activation.function(),
                window: activation.window_ptr(),
            });
            (activation.function(), home, activation.script().is_some_and(|script| script.environment))
        }
        None => {
            let frame = unsafe { (*cx).frame() };
            (vm.frames[frame].function, HandlerHome::Frame(frame), true)
        }
    }
}

/// The entry of the handler covering record `call_pc` of the running function, for the thrown
/// `value`: the `with` scopes past the handler's unwound and, for a binding in a local, the local
/// stored where the running function keeps it (`Vm::bind_thrown_value`), a captured one getting a
/// fresh environment per entry, whose copy replaces the one the lane's caches name in a frame.
/// A promoted binding is the body's own register. The running function is the innermost
/// activation's, else the frame's.
pub(in crate::vm) unsafe extern "C" fn handler_edge(
    _vm: u64,
    _residual: u64,
    context: u64,
    call_pc: u64,
    value: u64,
) -> u64 {
    let cx = context as usize as *mut LaneContext;
    let (call_pc, value) = (call_pc as u32, Value::from_bits(value));
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let (function, home, keeps_locals) = handler_home(cx, vm);
    let handler = code
        .function(function as usize)
        .handler_at(call_pc)
        .expect("a native catch's record has its handler");
    vm.unwind_handler_scopes(home, handler);
    if let Some(slot @ ThrownValueSlot::Local(_)) = handler.slot
        && keeps_locals
    {
        vm.bind_thrown_value(code, home, slot, value)
            .expect("a handler's binding is valid");
        note_allocation(vm);
        if matches!(home, HandlerHome::Frame(_)) {
            drop_environment_caches(cx);
        }
    }
    super::super::compiled::LANE_CALL_COMPLETED
}

script_handler! {
    /// `throw` (the catalog's raising record, `Op::raises`): the exception of the thrown value,
    /// made as the general step makes it, raised at this record, where a handler of the running
    /// function or of a plain caller takes it in the lane (`catch_raised`). Any other raise
    /// leaves as a body's does (`raised_exit_in_lane`): no frame of the activations is made before
    /// the raise needs one.
    #[inline(never)]
    pub(super) fn throw_record<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let value = r.get(i.a);
        let error = JsError::thrown_primitive(value).unwrap_or_else(|| JsError::thrown(value, vm.detail_string(value)));
        vm.lane_completion.raise(error);
        unwind_raised(cx, ip)
    }
}

/// The raise in `Vm::lane_raised`, raised at the record `ip` of the running function: recorded
/// there (`Vm::lane_raise_site`), then taken by a handler of the running function or of a plain
/// caller in the lane (`catch_raised`), else left as a body's raise leaves
/// (`raised_exit_in_lane`), from wherever unwinding stopped.
pub(super) fn unwind_raised(cx: *mut LaneContext, ip: Ip) -> Ip {
    let vm = unsafe { vm(cx) };
    // SAFETY: the record lies in the running view.
    let pc = unsafe { ip.offset_from((*cx).base) } as u32;
    vm.lane_raise_site = super::super::compiled::raise_site(running_function_in_lane(cx), pc);
    match catch_raised(cx, ip) {
        Unwound::Caught(target) => target,
        Unwound::Uncaught(record) => raised_exit_in_lane(cx, record),
    }
}

/// `lane_raise` beside frameless activations (frame-switches.md #6): the instruction at `ip`
/// failed before any effect, with an error whose stack, if it made one, the capture read
/// without the activations (no native call was deferred). `again` makes it a second time in
/// place beside the activations (`unwound_in_place`): it fails the same way, and the error it
/// builds now captures its stack from the frame walk (`Vm::lane_stack_sites`), so the error is
/// the one the general path, which used to run the instruction again from the frames, raises,
/// and it is raised and unwound as any operation made in place raises (`unwind_raised` where
/// the activations stayed the lane's, else from frames on the general path). Where it
/// unexpectedly succeeded, its effect is made and the general path continues after the record.
pub(super) fn raise_again<T>(
    cx: *mut LaneContext,
    ip: Ip,
    again: impl FnOnce(&mut Vm, &ResidualProgram) -> Result<T, JsError>,
) -> Ip {
    match unwound_in_place(cx, ip, again) {
        Err(next) => next,
        Ok(_) => {
            debug_assert!(false, "an instruction that failed before any effect fails again");
            exit(following(ip))
        }
    }
}

/// Where a raise in the lane goes on (`catch_raised`).
enum Unwound {
    /// The record of the handler entered, the lane's state set to run it.
    Caught(Ip),
    /// The record unwinding stopped at, in the activation or frame now running, the raise still in
    /// `Vm::lane_raised`, for the general path to unwind from.
    Uncaught(Ip),
}

/// What a raise at a record of the running function finds there (`enter_covering`).
enum Covered {
    By(Ip),
    Not,
    Declined,
}

/// The raise in `Vm::lane_raised` at `record` of the running function, unwound in the lane as the
/// general loop unwinds it (`Vm::unwind_general_loop`, `Vm::exception_handler_target`): the handler
/// covering the record entered (`enter_covering`), else the activation popped where its frame would
/// unwind with nothing to finish (`pop_unwound`) and its caller's call record tried next. Unwinding
/// stops before any frame it cannot finish so: a raise without its thrown value, or whose message
/// the general loop rewrites (`Vm::name_non_callable_callee`), Wasm, a running leaf, a native's
/// deferred call or callback boundary, and any activation with a completion, a side record or owned
/// iterators or `with` objects.
fn catch_raised(cx: *mut LaneContext, mut record: Ip) -> Unwound {
    let Some(value) = catchable_value(cx) else {
        return Unwound::Uncaught(record);
    };
    loop {
        match enter_covering(cx, record, value) {
            Covered::By(target) => return Unwound::Caught(target),
            Covered::Declined => return Unwound::Uncaught(record),
            Covered::Not => match pop_unwound(cx) {
                Some(call) => record = call,
                None => return Unwound::Uncaught(record),
            },
        }
    }
}

/// The thrown value of the raise in `Vm::lane_raised`, where the lane may unwind it (`catch_raised`).
fn catchable_value(cx: *mut LaneContext) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the running lane's context; its program outlives the run.
    let (leaf, wasm) = unsafe { (!(*cx).leaf.call.is_null(), (*(*cx).code).kind == crate::bytecode::ProgramKind::Wasm) };
    if leaf || wasm || vm.lane_deferred.is_some() || vm.lane_stack.native_boundary.is_some() {
        return None;
    }
    vm.lane_completion.raised_mut().filter(|error| unwinds_in_lane(error))?.thrown_value()
}

/// Whether the lane may unwind `error` (`catch_raised`): a thrown value whose message the
/// general loop does not rewrite (`Vm::name_non_callable_callee`). Any other error (a
/// termination, a trap, an error with no value yet) the general path raises.
pub(super) fn unwinds_in_lane(error: &JsError) -> bool {
    let renamed = error.has_message(super::super::call_printer::NOT_CALLABLE_MESSAGE)
        || error.has_message(super::super::call_printer::NOT_CONSTRUCTOR_MESSAGE);
    !renamed && error.thrown_value().is_some()
}

/// The handler of the running function covering `record`, entered for the thrown `value` as the
/// general loop enters it (`Vm::exception_handler_target`): the `with` scopes past it unwound, its
/// binding stored, the raise taken, and the accumulator its first record reads carried
/// (`carried_at_in_lane`). `Declined`, before any effect, where the running function is no script
/// activation's, or its binding is a local the activation does not keep.
fn enter_covering(cx: *mut LaneContext, record: Ip, value: Value) -> Covered {
    let vm = unsafe { vm(cx) };
    if vm.lane_stack.top_frame().is_some_and(|frame| frame.script().is_none()) {
        return Covered::Declined;
    }
    // SAFETY: the context's program outlives the lane run; `record` lies in the running view.
    let (code, pc) = unsafe { (&*(*cx).code, record.offset_from((*cx).base) as u32) };
    let (function, home, keeps_locals) = handler_home(cx, vm);
    let Some(handler) = code.function(function as usize).handler_at(pc) else {
        return Covered::Not;
    };
    let local = matches!(handler.slot, Some(ThrownValueSlot::Local(_)));
    let Some(view) = vm.view_any_in_lane(unsafe { (*cx).program }, function).filter(|_| keeps_locals || !local) else {
        return Covered::Declined;
    };
    vm.lane_completion.take_raised();
    vm.unwind_handler_scopes(home, handler);
    if let Some(slot) = handler.slot {
        vm.bind_thrown_value(code, home, slot, value).expect("a handler's binding is valid");
        note_allocation(vm);
        if local && matches!(home, HandlerHome::Frame(_)) {
            drop_environment_caches(cx);
        }
    }
    // SAFETY: the running lane's context; a handler's target is a record of the running view.
    unsafe {
        (*cx).resumed_acc = carried_at_in_lane(cx, view, (*cx).registers, handler.target).acc;
        Covered::By((*cx).base.add(handler.target as usize))
    }
}

/// Pops the innermost activation for a raise no handler of its function covers, where its frame
/// would unwind with nothing to finish (`Vm::unwind_general_loop`): a plain header with no side
/// record, a plain completion (no trampoline's) and no loop iterator or `with` object of its own.
/// Its caller is restored (`restore_caller_in_lane`), and its call record is where the raise goes on;
/// `None` changes nothing.
fn pop_unwound(cx: *mut LaneContext) -> Option<Ip> {
    let vm = unsafe { vm(cx) };
    let stack = &vm.lane_stack;
    if stack.plain.depth.get() == 0 || stack.innermost_side().is_some() {
        return None;
    }
    let depth = stack.activation_count() - 1;
    let owns = |owner: Option<usize>| owner.is_some_and(|owner| owner >= depth);
    if owns(stack.iterators.last().map(|(owner, _)| *owner)) || owns(stack.with_objects.last().map(|(owner, _)| *owner)) {
        return None;
    }
    if stack.top_frame()?.completion() != Some(Completion::Plain) {
        return None;
    }
    let popped = vm.lane_stack.pop_activation()?;
    restore_caller_in_lane(cx, &popped);
    Some(popped.call())
}
