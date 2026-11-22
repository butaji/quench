//! A derived class constructor's `super(...)` made by the lane in the frame it runs
//! (one-frame-one-call.md F6 (b) step 2): the parent's construction pushed in place
//! (`super_construct_in_frame`), the check that `super` ran once (`SuperCallCheck`, a lane step
//! made in place, `lane_general_step`) and the binding of `this` (`bind_this_in_frame`). Each is
//! made where and as the general loop makes it, so a derived construction runs in the lane from
//! its `new` to its return, its parents' frames included. Beside frameless activations, whose
//! `new.target` and `this` binding no frame holds, each takes the general path, as before.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

script_handler! {
    /// `super(...)` in the frame the lane runs (a derived constructor's, or an arrow's inside
    /// one), pushed in place as the general loop's `Construct` pushes it
    /// (`Vm::try_inline_super_construct`): the parent's [[Construct]] with the frame's
    /// `new.target` (`Vm::inline_super_construct_target`), the lane going on in the parent's
    /// frame (`push_construction_frame`). The construction's value lands in the record's
    /// register when the parent returns (`lane_return_script_frame`), and the `SuperCallCheck`
    /// and `InitializeThis` after it run in the lane. Applied arguments (an implicit
    /// constructor's rest list) pass where a dense array holds them
    /// (`Vm::dense_super_arguments`). Beside frameless activations, and for arguments or a parent
    /// the general loop would not push either, it takes the general path (`construct_super_value`).
    #[inline(never)]
    pub(crate) fn super_construct_in_frame<>(cx, r, ip, i, view) {
        super_construct(cx, r, ip, i)
    }
}

/// `super_construct_in_frame`'s push, or the general path's `exit`.
fn super_construct(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Ip {
    let vm = unsafe { vm(cx) };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    // SAFETY: the running lane's context.
    let (calls, constructs) = unsafe { ((*cx).script_calls, (*cx).script_constructs) };
    if vm.lane_stack.has_frames()
        || !quench_runtime::staged::call_construct::concrete_admits_construct(calls, constructs, vm.construct_target.is_some())
        || returns && !returning_call_admitted(cx)
    {
        return exit(ip);
    }
    let parent = r.get(i.b);
    // SAFETY: the context's program outlives the lane run.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame()) };
    let Some(construction) = vm.inline_super_construct_target(code, frame, parent) else {
        return exit(ip);
    };
    let listed;
    let arguments: *const [Value] = if ImmediateLayout::construct_array_arguments(i.imm()) {
        let Some(values) = vm.dense_super_arguments(r.get(i.c)) else {
            return exit(ip);
        };
        listed = values;
        std::ptr::from_ref(listed.as_slice())
    } else {
        let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
        // SAFETY: the construction's window is registers of the running frame.
        std::ptr::slice_from_raw_parts(unsafe { r.0.add(usize::from(i.c)) }, argc)
    };
    let new_target = (parent, construction.new_target);
    push_construction_frame(cx, ip, i, frame, construction.target, new_target, (construction.this, construction.completion), arguments)
}

/// A construction of user constructor `callee` pushed in place from frame `frame`, the one the
/// lane runs, as the general loop's in-loop construction pushes it
/// (`Vm::push_in_loop_construction`): the construction target bound for the callee's activation
/// to take (`new_target`), the callee's frame pushed over `this` with the pending call that
/// completes the construction (`completion`, which `lane_return_script_frame` applies), and the
/// lane going on in that frame. A constructor of another program runs in its own, which the push
/// makes active and the loop's executing one sets aside. A failed push (the guest call depth)
/// throws at the construction, as the general path's.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_construction_frame(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    frame: usize,
    (program, id, env): (ProgramId, u32, Value),
    (callee, new_target): (Value, Value),
    (this, completion): (Value, CallCompletion),
    arguments: *const [Value],
) -> Ip {
    let vm = unsafe { vm(cx) };
    #[cfg(feature = "profile-lane-exits")]
    if let Some(body) = frame_entry_census(vm, program, id) {
        quench_runtime::vm::lane_exits::record_detail(format!("frame construct {body}"));
    }
    // Allocating the receiver can make a collection due.
    note_allocation(vm);
    // SAFETY: the call record lies in the running view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    vm.frames[frame].pc = call_pc + 1;
    vm.frames[frame].pc_is_resume = true;
    vm.set_construct_target(Some(quench_runtime::vm::activation::ConstructionTarget { callee, new_target }));
    let caller_program = (program != unsafe { (*cx).program }).then(|| enter_callee_program(cx, program));
    // SAFETY: the context's program is now the callee's, which outlives the lane run; the
    // arguments are registers of the caller frame or a list the caller holds, which the push
    // neither moves nor frees before it copies them.
    let (code, arguments) = unsafe { (&*(*cx).code, &*arguments) };
    let pushed = vm.push_general_user_frame(code, id, env, this, arguments, CallContext::user_function(id, callee));
    vm.set_construct_target(None);
    match pushed {
        Ok(call_guard) => enter_pushed_constructor(cx, PendingGeneralCall {
            caller: frame,
            caller_cursor: None,
            call_pc: call_pc as u32,
            destination: i.a & quench_runtime::bytecode::REGISTER_MASK,
            completion,
            returns: i.a & quench_runtime::bytecode::RETURN_REGISTER != 0,
            call_guard: Some(call_guard),
            caller_program,
        }),
        Err(error) => {
            if let Some(saved) = caller_program {
                leave_callee_program(cx, saved);
            }
            vm.lane_completion.raise(error);
            exit(ip)
        }
    }
}

/// The lane going on in the constructor's frame `push_construction_frame` pushed, above its
/// caller's, with `call` pending for the general loop as its own push leaves it.
fn enter_pushed_constructor(cx: *mut LaneContext, call: PendingGeneralCall) -> Ip {
    let vm = unsafe { vm(cx) };
    let callee_frame = call.caller + 1;
    // SAFETY: the general loop's pending calls outlive the lane run.
    unsafe { &mut *(*cx).pending }.push(call);
    // The pushed frame's guard narrows the activations the thread admits.
    vm.lane_stack.refresh_activation_limit();
    // SAFETY: the context's program is the callee's, which outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if let Some(return_pc) = vm.js_tier_frame_entry(code, callee_frame) {
        vm.frames[callee_frame].pc = return_pc;
    }
    enter_script_frame_in_lane(cx, callee_frame).into_loop()
}

/// Make `program` the lane's and the general loop's executing one for a constructor of it the
/// lane pushes, as `lane_push_frame_call` does for a call: the caller's, set aside, for its return.
fn enter_callee_program(cx: *mut LaneContext, program: ProgramId) -> quench_runtime::vm::dispatch_frame::SavedProgram {
    let vm = unsafe { vm(cx) };
    // SAFETY: the general loop's executing program outlives the lane run.
    let executing = unsafe { &mut *(*cx).executing };
    let saved = quench_runtime::vm::dispatch_frame::SavedProgram {
        id: executing.id,
        retained: std::mem::replace(&mut executing.retained, vm.programs.get(program)),
    };
    executing.id = program;
    enter_program_in_lane(cx, program);
    saved
}

/// The caller's program `saved` executing again, a push `enter_callee_program` made having failed.
fn leave_callee_program(cx: *mut LaneContext, saved: quench_runtime::vm::dispatch_frame::SavedProgram) {
    // SAFETY: as `enter_callee_program`.
    let executing = unsafe { &mut *(*cx).executing };
    executing.id = saved.id;
    executing.retained = saved.retained;
    enter_program_in_lane(cx, saved.id);
}

script_handler! {
    /// `InitializeThis` (the catalog's `binds_this`): `super(...)` binding `this` in the frame the
    /// lane runs, made in place as the general step makes it (`Vm::initialize_this_binding`: the
    /// binding stored, the frames that resolve it refreshed, the instance initializer run, whose
    /// throw unwinds from this record, `unwound_in_place`). The binding is initialized for good
    /// then, so the frame fixes it (`Vm::current_this`) and it becomes the lane's `this` word, as
    /// the frame's next entry would set it (`Vm::frame_lane_this`): the stores to `this` after
    /// `super(...)` read the receiver from the word. Beside frameless activations, whose binding
    /// no frame holds, it takes the general path.
    #[inline(never)]
    pub(crate) fn bind_this_in_frame<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if vm.lane_stack.has_frames() {
            return exit(ip);
        }
        // SAFETY: the running lane's context.
        let frame = unsafe { (*cx).frame() };
        let value = r.get(i.a);
        if let Err(next) = unwound_in_place(cx, ip, |vm, code| vm.initialize_this_binding(code, frame, value)) {
            return next;
        }
        // SAFETY: as above; the frame's registers are the lane's after the step.
        let r = unsafe { (*cx).registers };
        r.set_this(vm.current_this(frame).unwrap_or(Value::DELETED));
        next!(cx, r, following(ip), Carried::script_in(cx, view))
    }
}
