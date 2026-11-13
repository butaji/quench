//! A lane body's call of a callee that does not complete in a body of its own: the callee's
//! activation runs in the lane, nested under the body, and its return comes back to the
//! body's call, so the caller's specialized code keeps running (V8's and JSC's baseline
//! callers return into compiled code whatever tier the callee ran in).
use super::*;
use super::lane_body_tier::{audit_nanos, Timed};
use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT, LANE_CALL_MISSED, LANE_CALL_TERMINATED};
use super::body_forms::{BodyCall, Gathered};
use super::lane_body_stats::{miss, Miss};
use crate::staged::Arguments;

/// The end of a nested run at the return with `value` of `activation`, popped with its
/// caller's state restored: a native's callback's (`Completion::ReturnsToNative`), whose
/// native takes the value, a job's (`Completion::ResumesJob`), whose run ends with it for the
/// job to deliver, or a body's callee's, whose call's register takes it, a construction's
/// receiver (`this`) for a non-object. `None` for any other completion.
#[inline(always)]
pub(super) fn ends_run<H: Host>(cx: *mut LaneContext<H>, activation: &Activation, completion: Completion, this: Value, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    match completion {
        Completion::ReturnsToNative | Completion::ResumesJob => {}
        Completion::ReturnsToBody | Completion::ConstructsToBody => {
            let value = setters::completed_value(vm, activation, completion, this, value);
            // SAFETY: the call record belongs to the caller's live view, whose registers the context names.
            let destination = unsafe { (*activation.call).a } & crate::bytecode::REGISTER_MASK;
            unsafe { (*cx).registers }.set(destination, value);
        }
        _ => return None,
    }
    vm.lane_native_returned = Some(value);
    Some(Resumed::Exit(exit(activation.call)))
}

/// Whether `activation`, which becomes the frame at index `frame`, is the entry frame of
/// the general loop that finishes it, as a native's callback or a job's is. A body's callee is not:
/// its frame's return leaves the lane instead (`LaneStack::body_boundary`), which the
/// innermost such frame decides.
pub(super) fn enters_as_frame(stack: &mut LaneStack, activation: &Activation, frame: usize) -> bool {
    match activation.script().map(|script| script.completion) {
        Some(Completion::ReturnsToNative | Completion::ResumesJob) => true,
        Some(Completion::ReturnsToBody | Completion::ConstructsToBody) => {
            stack.body_boundary = Some(frame);
            false
        }
        _ => false,
    }
}

/// Whether `resume`, the record a body's callee left at, is where the lane left for the
/// general path (`finish`), not a record the lane continues at.
#[inline(always)]
pub(super) fn left_lane(resume: Ip) -> bool {
    resume.addr() & EXIT_TAG != 0
}

/// Changes the completion of the lane's `depth`th activation `from` one `to` another;
/// false, changing nothing, where it holds another or is no script activation.
fn recomplete<H: Host>(cx: *mut LaneContext<H>, depth: usize, from: Completion, to: Completion) -> bool {
    match unsafe { vm(cx) }.lane_stack.activations.get_mut(depth).map(|activation| &mut activation.kind) {
        Some(ActivationKind::Script(script)) if script.completion == from => {
            script.completion = to;
            true
        }
        _ => false,
    }
}

/// Leaves the body for the lane at `resume`, the callee's activation's record
/// (`LaneExit::Callee`).
fn leave<H: Host>(cx: *mut LaneContext<H>, resume: Ip, acc: Value) -> u64 {
    unsafe { (*cx).callee_resume = resume; (*cx).callee_acc = acc; }
    LANE_CALL_LEFT
}

/// The rest of the activation a body's call made, the lane's `depth`th, from its record
/// `resume` with accumulator `acc`: run in the lane to its return, which completes the call
/// (`LANE_CALL_COMPLETED`). A run that leaves the lane for the general path leaves the body
/// too, at the record the lane left at (`left_lane`), as does a call no nested run takes:
/// one below such a leave, or without native stack.
pub(super) fn finish<H: Host>(cx: *mut LaneContext<H>, depth: usize, resume: Ip, acc: Value, constructs: bool) -> u64 {
    let (plain, nested) = if constructs { (Completion::Constructs, Completion::ConstructsToBody) } else { (Completion::Plain, Completion::ReturnsToBody) };
    let Ok(_stack) = crate::stack::StackGuard::enter() else { return leave(cx, resume, acc) };
    if left_lane(resume) || !recomplete(cx, depth, plain, nested) {
        return leave(cx, resume, acc);
    }
    let left = run_nested(cx, resume, acc);
    recomplete(cx, depth, nested, plain);
    ended(cx, left)
}

/// The status of a body's call whose nested run ended as `run_nested` says.
fn ended<H: Host>(cx: *mut LaneContext<H>, left: Option<Ip>) -> u64 {
    left.map_or(LANE_CALL_COMPLETED, |left| leave(cx, exit(left), Value::UNDEFINED))
}

/// The lane run from `resume` with accumulator `acc`, nested under the running body: `None`
/// where it ended with a value for the body's call (`ends_run`), else the record the lane
/// left at.
fn run_nested<H: Host>(cx: *mut LaneContext<H>, resume: Ip, acc: Value) -> Option<Ip> {
    let vm = unsafe { vm(cx) };
    unsafe {
        (*cx).resumed_acc = Slot::of(acc);
        (*cx).memory = MemoryView::script(cx);
    }
    let outer = vm.lane_stack.body_boundary.take();
    let left = lane_dispatch(cx, resume);
    vm.lane_stack.body_boundary = outer;
    vm.lane_native_returned.take().is_none().then_some(left)
}

/// A body's call of an array callback method whose first callback's activation the body made
/// (`array_steps::lane_body_array_iteration`), its record `resume`: run in the lane until the
/// iteration completes the call (`LANE_CALL_COMPLETED`); a run that leaves the lane for the
/// general path leaves the body at the record it left at.
pub(super) fn finish_array_iteration<H: Host>(cx: *mut LaneContext<H>, resume: Ip) -> u64 {
    ended(cx, run_nested(cx, resume, Value::UNDEFINED))
}

/// An entry of the function `own` views that `timed` (`LaneView::audit_entry`) put on the
/// interpreter's side: `Err` with the record the lane continues at after the entry ran
/// interpreted and timed (the activation, run from its first record `start` nested to its
/// return, completed, so its caller continues after the call, or left the lane, the exit
/// record); `Ok` for an entry that runs its body, timed or not. An interpreted side runs only
/// for the innermost activation of a plain call or construction of this function whose call
/// keeps its result; any other entry runs its body untimed.
pub(super) fn audited<H: Host>(cx: *mut LaneContext<H>, own: &LaneView, function: u32, start: Ip, timed: Option<Timed>) -> Result<(), Ip> {
    if timed != Some(Timed::Interpreter) {
        return Ok(());
    }
    let vm = unsafe { vm(cx) };
    let Some(depth) = vm.lane_stack.activations.len().checked_sub(1) else { return Ok(()) };
    let top = &vm.lane_stack.activations[depth];
    let constructs = match top.script().map(|script| script.completion) {
        Some(Completion::Plain) => false,
        Some(Completion::Constructs) => true,
        _ => return Ok(()),
    };
    let call = top.call;
    // SAFETY: the call record belongs to the caller's live view.
    if top.function != function || unsafe { (*call).a } & crate::bytecode::RETURN_REGISTER != 0 {
        return Ok(());
    }
    let started = std::time::Instant::now();
    if finish(cx, depth, start, Value::UNDEFINED, constructs) == LANE_CALL_COMPLETED {
        own.audit_record(Timed::Interpreter, audit_nanos(started));
        return Err(following(call));
    }
    // SAFETY: `finish` set it before it left.
    let resume = unsafe { (*cx).callee_resume };
    if left_lane(resume) { Err(resume) } else { Ok(()) }
}

/// A lane body's call, by the call's decoded form (`BodyCall`: a register call, a known call or a
/// method call): the callee that completes in its own body returns its value to the caller's native
/// code; one that leaves continues in the lane in its own activation; a native that completes in
/// place (`native_calls::lane_body_native_call`) does so too. A callee loaded from a method site's
/// caches is the one the interpreter would call. Any other callee or case is a miss, which leaves
/// the body at the call, before any effect.
pub(in crate::vm) unsafe extern "C" fn lane_body_call<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    // Static regions recover ordinary calls through their linked semantic operation.
    // Binding this JIT-only helper to a miss removes its interpreter closure entirely.
    if !crate::DYNAMIC_EXECUTION_ENABLED { return LANE_CALL_MISSED; }
    let cx = context as usize as *mut LaneContext<H>;
    // A body entered through its cell has no activation yet, which a call pushes above.
    lane_materialize_running_leaf(cx);
    // SAFETY: the context is the running lane's and `call_pc` a record of the running body's function.
    match unsafe { BodyCall::at(cx, call_pc) } {
        Some(call) => body_call(cx, &call),
        None => miss(Miss::CallForm),
    }
}

/// `lane_body_call` of the decoded `call`.
fn body_call<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall) -> u64 {
    let vm = unsafe { vm(cx) };
    let Some(receiver) = call.receiver(vm) else { return miss(Miss::CallReceiver) };
    let Some(callee) = call.callee(vm, receiver) else { return miss(Miss::CallCallee) };
    if let Err(refused) = call_admitted(cx, vm, call, callee) { return refused; }
    let Some(target) = lane_body_call_target(cx, vm, callee, call.link()) else {
        // A native may run guest code above the native windows below: the body leaves instead.
        if Vm::<H>::lane_shadow_active() { return miss(Miss::CallShadowActive); }
        return super::native_calls::lane_body_native_call(cx, call, callee, receiver);
    };
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    body_script_call(cx, call, unsafe { &*target }, receiver)
}

/// Whether the lane admits a body's call of `callee` at all: it may activate script calls, the
/// callee is not being constructed, no `with` scope is open, and a returning call may complete.
fn call_admitted<H: Host>(cx: *mut LaneContext<H>, vm: &Vm<H>, call: &BodyCall, callee: Value) -> Result<(), u64> {
    if !unsafe { (*cx).script_calls } { return Err(miss(Miss::CallNoScriptCalls)); }
    if lane_constructing(vm, callee) { return Err(miss(Miss::CallConstructing)); }
    if vm.with_scope_entered { return Err(miss(Miss::CallWithScope)); }
    if call.site.returns && !lane_returning_call_admitted(cx) { return Err(miss(Miss::CallReturningNotAdmitted)); }
    Ok(())
}

/// A body's call of the script function `target`, binding `receiver`: answered by its read, else
/// activated as the lane's own call would.
fn body_script_call<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, target: &ScriptCallTarget, receiver: Value) -> u64 {
    let vm = unsafe { vm(cx) };
    let Some(this) = lane_direct_call_this(vm, target, receiver) else { return miss(Miss::CallReceiver) };
    // SAFETY: as `lane_call_read`.
    if target.entry.answered_by_read() { return unsafe { body_read_call(cx, call, target, this) }; }
    // An activation above native windows would need shadow frames beneath it on a leave.
    if Vm::<H>::lane_shadow_active() { return miss(Miss::CallShadowActive); }
    let Some(how) = lane_call_activation(target, call.argc()) else { return miss(Miss::CallEntryNotActivation) };
    if matches!(how, LaneActivation::Plain) && !lane_activation_room(vm, target) { return miss(Miss::CallNoActivationRoom); }
    lane_body_activate(cx, call, target, this, false, how)
}

/// `body_script_call` of a target whose body is a read: the read's hit completes it, as the
/// lane's own call does (`lane_script_call`), its value in the call's register, which the
/// body reads.
///
/// # Safety
/// As `lane_call_read`.
#[inline(always)]
unsafe fn body_read_call<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, target: &ScriptCallTarget, this: Value) -> u64 {
    // SAFETY: the caller's contract.
    let Some(value) = (unsafe { body_read_answer(cx, call, target, this) }) else { return miss(Miss::CallReadAnswer) };
    call.registers.set(call.site.result, value);
    LANE_CALL_COMPLETED
}

/// The value `target`'s read answers `call` with: memoized at the record where its layout has a
/// memo form (a register call's window), else read from the values it passes.
///
/// # Safety
/// As `lane_call_read`.
unsafe fn body_read_answer<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, target: &ScriptCallTarget, this: Value) -> Option<Value> {
    if let (true, Arguments::Window { first, count }) = (call.memoizes(), call.site.arguments) {
        let passed = ReadArguments { registers: call.registers, base: first, passed: count };
        return unsafe { lane_answered_call(cx, call.ip, target, passed, this) };
    }
    // SAFETY: the registers do not move while the read runs.
    let values = unsafe { call.values() };
    let passed = ReadArguments { registers: Registers(values.as_ptr().cast_mut()), base: 0, passed: call.argc() };
    unsafe { lane_read_answer(cx, target, passed, this) }
}

/// A lane body's `new`, as the lane's own construction does (`lane_script_construct`): the
/// callee's plan, else the receiver its target allocates and, where its body does more
/// than store fields, its activation, whose completion yields its returned object or the
/// receiver. A returned construction takes the plan only. Its results are `lane_body_call`'s.
pub(in crate::vm) unsafe extern "C" fn lane_body_construct<H: Host>(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    // As `lane_body_call`: static regions recover constructions through their semantic operation.
    if !crate::DYNAMIC_EXECUTION_ENABLED { return LANE_CALL_MISSED; }
    let cx = context as usize as *mut LaneContext<H>;
    // SAFETY: as `lane_body_call`.
    match unsafe { BodyCall::at(cx, call_pc) } {
        Some(call) if call.site.constructs() => body_construct(cx, &call),
        _ => miss(Miss::ConstructForm),
    }
}

/// `lane_body_construct` of the decoded `call`.
fn body_construct<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall) -> u64 {
    let vm = unsafe { vm(cx) };
    // A construction's callee is a register: no receiver loads it.
    let Some(callee) = call.callee(vm, Value::UNDEFINED) else { return miss(Miss::ConstructNoTarget) };
    if let Err(refused) = construct_admitted(cx, vm) { return refused; }
    // The callee's plan first, the common `new C(...)`, which needs no call target; a returned
    // construction completes by it alone.
    if let Some(this) = constructed_in_place(vm, call, callee) {
        call.registers.set(call.site.result, this);
        return LANE_CALL_COMPLETED;
    }
    // A returning construction takes the plan only: the lane completes it with its callee's result.
    if call.site.returns { return miss(Miss::ConstructReturning); }
    construct_activated(cx, call, callee)
}

/// Whether the lane admits a body's construction: it activates script calls and constructs in
/// place, no construction or `with` scope is open, and no collection is due.
fn construct_admitted<H: Host>(cx: *mut LaneContext<H>, vm: &Vm<H>) -> Result<(), u64> {
    if !unsafe { (*cx).script_calls && (*cx).script_constructs } { return Err(miss(Miss::ConstructNoScriptConstructs)); }
    if vm.construct_target.is_some() { return Err(miss(Miss::ConstructConstructTarget)); }
    if vm.with_scope_entered { return Err(miss(Miss::ConstructWithScope)); }
    if vm.heap.should_collect() { return Err(miss(Miss::ConstructCollectionDue)); }
    Ok(())
}

/// The receiver `callee`'s plan finishes (`planned_construction`, or a forwarding constructor's
/// fields), else, for `new Array()` of the realm's own `Array`, ArrayCreate(0), an empty array of
/// `Array.prototype`, which calls nothing.
fn constructed_in_place<H: Host>(vm: &mut Vm<H>, call: &BodyCall, callee: Value) -> Option<Value> {
    let this = planned_construction(vm, callee, call.argument())
        .or_else(|| lane_forwarded_fields(vm, callee, call.argument()))
        .or_else(|| (call.argc() == 0 && vm.realm_array_constructor(callee)).then(|| vm.hole_array(0)))?;
    note_allocation(vm);
    Some(this)
}

/// A body's construction of `callee` by its constructor's own activation: the receiver its target
/// allocates (or its initializer finishes) and, where its body does more than store fields, the
/// activation.
fn construct_activated<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, callee: Value) -> u64 {
    let vm = unsafe { vm(cx) };
    // What is left resolves a target and pushes an activation above the body's own, which a
    // body entered through its cell makes now.
    lane_materialize_running_leaf(cx);
    let Some(target) = lane_construct_target(cx, vm, callee) else { return miss(Miss::ConstructNoTarget) };
    // A forwarding constructor is built by its forwarded initializer's plan or not at all here.
    if target.forwards() { return construct_forwarded(vm, call, callee); }
    // The rest activates the constructor, which native windows below would need shadow frames for.
    if Vm::<H>::lane_shadow_active() { return miss(Miss::ConstructShadowActive); }
    if !target.constructs { return miss(Miss::ConstructNotConstructor); }
    // The activation of the lane's own construction (`lane_script_construct`): a target that
    // keeps or gathers values, or is passed more than it has parameters, by the keeping form.
    let how = match lane_call_activation(&target, call.argc()) {
        Some(LaneActivation::Environment) | None => return miss(Miss::ConstructEntryNotActivation),
        Some(how) => how,
    };
    if matches!(how, LaneActivation::Plain) && !lane_activation_room(vm, &target) { return miss(Miss::ConstructNoActivationRoom); }
    let Some((this, initialized)) = construct_receiver(cx, call, callee, &target) else { return miss(Miss::ConstructNoReceiver) };
    // Allocating the receiver can make a collection due.
    note_allocation(vm);
    if initialized {
        call.registers.set(call.site.result, this);
        return LANE_CALL_COMPLETED;
    }
    lane_body_activate(cx, call, &target, this, true, how)
}

/// The receiver of a construction by `target`, with whether its initializer's cached adds finished
/// its fields: the allocation of `callee`'s prototype, or the finished receiver.
fn construct_receiver<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, callee: Value, target: &ScriptCallTarget) -> Option<(Value, bool)> {
    let vm = unsafe { vm(cx) };
    if target.initializes {
        // SAFETY: the context's program outlives the lane run.
        lane_initialize_fields(vm, unsafe { &*(*cx).code }, callee, callee, target.function, call.argument())
    } else {
        vm.inline_construct_receiver(callee).map(|this| (this, false))
    }
}

/// The target a lane body's `new` of `callee` activates: its cached entry, else the one the
/// lane's own construction resolves for a function of the running program.
fn lane_construct_target<H: Host>(cx: *mut LaneContext<H>, vm: &mut Vm<H>, callee: Value) -> Option<ScriptCallTarget> {
    // SAFETY: as `Vm::script_call_entry`; the target is copied before anything allocates.
    vm.script_call_entry(callee).map(|entry| unsafe { *entry }).or_else(|| {
        // SAFETY: the context's program outlives the lane run.
        vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee).filter(|target| target.program == vm.active_program)
    })
}

/// A forwarding constructor's fields, built by its forwarded initializer's plan.
fn construct_forwarded<H: Host>(vm: &mut Vm<H>, call: &BodyCall, callee: Value) -> u64 {
    let Some(this) = lane_forwarded_fields(vm, callee, call.argument()) else { return miss(Miss::ConstructForwarding) };
    note_allocation(vm);
    call.registers.set(call.site.result, this);
    LANE_CALL_COMPLETED
}

/// The target a lane body's call of `callee` activates: the site's linked entry, else the one
/// the lane's call resolves for a script function of the running program
/// (`lane_script_call_resolve`: `Vm::resolve_script_call_target`), which links the site.
fn lane_body_call_target<H: Host>(cx: *mut LaneContext<H>, vm: &mut Vm<H>, callee: Value, link: &std::cell::Cell<u16>) -> Option<*const ScriptCallTarget> {
    vm.linked_script_call_entry(callee, link).or_else(|| {
        if matches!(vm.heap.function(callee), Some(FunctionRecord { kind: FunctionKind::Native(_), .. })) || vm.cross_call_entry(callee).is_some() {
            return None;
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let resolved = vm.resolve_script_call_target(code, callee)?;
        if resolved.program != vm.active_program { return None; }
        vm.linked_script_call_entry(callee, link)
    })
}

/// How the lane activates a call of `target` passed `argc` values: its registers alone, as
/// `lane_activate`; by the keeping form where it keeps or gathers values or is passed more than it
/// has parameters; by the environment form. A frame or a settling entry takes the lane's own
/// call, which is `None`.
fn lane_call_activation(target: &ScriptCallTarget, argc: u16) -> Option<LaneActivation> {
    match target.entry {
        entry if entry.activates() && argc <= target.params.min(FIXED_ARGUMENT_COPIES) => Some(LaneActivation::Plain),
        entry if entry.activates() || target.forwards() => Some(LaneActivation::Keeping),
        CallEntry::Environment => Some(LaneActivation::Environment),
        _ => None,
    }
}

/// How a lane body's call activates its target: its window copied (`BodyCall::push_plain`), or
/// by the lane's keeping form (`lane_activate_keeping`).
#[derive(Clone, Copy)]
enum LaneActivation {
    Plain,
    Keeping,
    /// A target whose window holds its environment and locals (`lane_activate_environment`).
    Environment,
}

/// The activation a lane body's call makes of `target` (`how`), its registers; the helper's
/// miss where the lane's keeping or environment form routes the call to the general path.
fn lane_body_push<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, target: &ScriptCallTarget, this: Value, constructs: bool, how: LaneActivation)
    -> Result<Registers, u64> {
    let refused = || miss(Miss::CallNoActivationRoom);
    Ok(match how {
        LaneActivation::Plain => call.push_plain(cx, target, this, constructs),
        // The activation the lane's own call makes where values are kept or gathered.
        LaneActivation::Keeping => {
            let mut gathered = Gathered::new();
            let arguments = call.kept(&mut gathered);
            lane_activate_keeping::<H, false>(cx, call.registers, call.ip, target, arguments, this, constructs, None).map_err(|_| refused())?.0
        }
        LaneActivation::Environment => {
            // SAFETY: the call's values are registers of the running function.
            let values = unsafe { call.values() };
            lane_activate_environment(cx, call.registers, call.ip, target, &values, this).map_err(|_| refused())?.0
        }
    })
}

/// A target without a body warms toward one by a body's calls too, as by the lane's own
/// (`lane_specialize`): a constructor only body code constructs is entered by nothing else.
fn lane_warm_callee<H: Host>(cx: *mut LaneContext<H>, own: &LaneView, function: u32) {
    if own.respecializes() {
        lane_specialize(cx, own, function, true, &[]);
    } else if own.lane_body().is_none() {
        lane_specialize(cx, own, function, false, &[]);
    }
}

/// The `this` a callee's body is entered with: the deleted marker where it reads a lexical one.
fn lane_body_this(target: &ScriptCallTarget, this: Value) -> Value {
    if target.this_binding == ThisBinding::Lexical || this.is_deleted() { Value::DELETED } else { this }
}

/// The activation of `target` a lane body's call (`constructs`: `new`) makes, run by the target's
/// own body where it has one: its completion writes the call's register (`LANE_CALL_COMPLETED`),
/// and any other end leaves the lane in the activation (`LANE_CALL_LEFT`).
fn lane_body_activate<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, target: &ScriptCallTarget, this: Value, constructs: bool, how: LaneActivation) -> u64 {
    let vm = unsafe { vm(cx) };
    lane_note_late_call(cx, call.ip, target, call.argc());
    let registers = match lane_body_push(cx, call, target, this, constructs, how) { Ok(registers) => registers, Err(refused) => return refused };
    // A leave resumes at a record of the callee's view, never at its tier prologue, which
    // lies outside its records: a loop region's `Callee` exit (`lane_loop_native`) leaves the
    // lane there and needs its PC. `lane_specialize` below takes the prologue's warming step.
    let start = unsafe { (*cx).base };
    let depth = vm.lane_stack.activations.len() - 1;
    let leave = move |resume: Ip, acc: Value| finish(cx, depth, resume, acc, constructs);
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(target.function as usize).and_then(super::super::program_store::derived_lane_view);
    let Some(own) = own else { return leave(start, Value::UNDEFINED) };
    lane_warm_callee(cx, own, target.function);
    let Some(method) = own.lane_body().filter(|method| lane_body_admits(cx, method, NO_PENDING_ACTIVATIONS)) else {
        unsafe { (*cx).callee_warming = own.warming() };
        return leave(start, Value::UNDEFINED);
    };
    let (program, code) = unsafe { ((*cx).program, &*(*cx).code) };
    let captures = capture_reads(unsafe { (*cx).capture });
    let exit = vm.run_nested_lane_method(&method, program, code, registers.0, lane_body_this(target, this), captures, cx.cast());
    let (resumed, feedback) = lane_body_left(cx, registers, own, &method, exit);
    drop(method);
    if lane_leave_counts(cx, exit) {
        own.note_lane_body_end(feedback, false, |left| lane_note_leave(cx, target.function, left));
    }
    match resumed {
        LaneBodyLeft::Terminated => LANE_CALL_TERMINATED,
        LaneBodyLeft::Resume(_, resume, carried) => leave(resume, unsafe { carried.acc.get() }),
        LaneBodyLeft::Completed(_, value) => lane_body_completed(cx, call.ip, value, constructs),
    }
}

/// The completion of the activation a body's call made, with `value`: a construction's yields its
/// returned object or the receiver.
fn lane_body_completed<H: Host>(cx: *mut LaneContext<H>, ip: Ip, value: Value, constructs: bool) -> u64 {
    if constructs {
        lane_complete_activation(cx, ip, value).expect("the construction made an activation");
    } else {
        lane_return_to_body(cx, value).expect("the call made a plain activation");
    }
    LANE_CALL_COMPLETED
}
