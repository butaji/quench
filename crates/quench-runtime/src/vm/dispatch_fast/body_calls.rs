//! A lane body's call of a callee that does not complete in a body of its own: the callee's
//! activation runs in the lane, nested under the body, and its return comes back to the
//! body's call, so the caller's specialized code keeps running (V8's and JSC's baseline
//! callers return into compiled code whatever tier the callee ran in).
use super::*;
use super::super::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT, LANE_CALL_MISSED, LANE_CALL_RAISED, LANE_CALL_TERMINATED};
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
            let destination = unsafe { (*activation.call()).a } & crate::bytecode::REGISTER_MASK;
            unsafe { (*cx).registers }.set(destination, value);
        }
        _ => return None,
    }
    vm.lane_completion.set(LaneCompletion::NativeReturned(value));
    Some(Resumed::Exit(exit(activation.call())))
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
    match unsafe { vm(cx) }.lane_stack.activations.get_mut(depth).map(|activation| activation.kind_mut()) {
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
    if left_lane(resume) || !recomplete(cx, depth, plain, nested) { return leave(cx, resume, acc); }
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
    vm.lane_completion.take_native_returned().is_none().then_some(left)
}

/// A body's call of an array callback method whose first callback's activation the body made
/// (`array_steps::lane_body_array_iteration`), its record `resume`: run in the lane until the
/// iteration completes the call (`LANE_CALL_COMPLETED`); a run that leaves the lane for the
/// general path leaves the body at the record it left at.
pub(super) fn finish_array_iteration<H: Host>(cx: *mut LaneContext<H>, resume: Ip) -> u64 {
    ended(cx, run_nested(cx, resume, Value::UNDEFINED))
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
    let status = native_frames::in_innermost_frame(cx, || unsafe { lane_body_call_in(cx, call_pc) });
    // SAFETY: `context` is the running lane's.
    unsafe { refresh_body_bases::<H>(context) };
    status
}

/// `lane_body_call` with the context of the running body, which a native callee's frame made the
/// innermost (`native_frames::in_innermost_frame`).
///
/// # Safety
/// As `lane_body_call`; `call_pc` is a record of the running body's function.
unsafe fn lane_body_call_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    match unsafe { BodyCall::at(cx, call_pc) } {
        Some(call) => body_call(cx, &call),
        None => miss(unsafe { vm(cx) }, Miss::Form),
    }
}

/// `lane_body_call` of the decoded `call`.
fn body_call<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall) -> u64 {
    let vm = unsafe { vm(cx) };
    let Some(receiver) = call.receiver(vm) else { return miss(vm, Miss::Receiver) };
    let Some(callee) = call.callee(vm, receiver) else { return miss(vm, Miss::Callee) };
    if let Err(refused) = call_admitted(cx, vm, call, callee) { return refused; }
    let Some(target) = lane_body_call_target(cx, vm, callee, call.link()) else {
        return super::native_calls::lane_body_native_call(cx, call, callee, receiver);
    };
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    body_script_call(cx, call, unsafe { &*target }, receiver)
}

/// Whether the lane admits a body's call of `callee` at all: it may activate script calls, the
/// callee is not being constructed, no `with` scope is open, and a returning call may complete.
fn call_admitted<H: Host>(cx: *mut LaneContext<H>, vm: &Vm<H>, call: &BodyCall, callee: Value) -> Result<(), u64> {
    if !unsafe { (*cx).script_calls } { return Err(miss(vm, Miss::NoScriptCalls)); }
    if lane_constructing(vm, callee) { return Err(miss(vm, Miss::CallConstructing)); }
    if vm.with_scope_entered { return Err(miss(vm, Miss::WithScope)); }
    if call.site.returns && !lane_returning_call_admitted(cx) { return Err(miss(vm, Miss::CallReturningNotAdmitted)); }
    Ok(())
}

/// A body's call of the script function `target`, binding `receiver`: answered by its read, else
/// activated as the lane's own call would.
fn body_script_call<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, target: &ScriptCallTarget, receiver: Value) -> u64 {
    let vm = unsafe { vm(cx) };
    let Some(this) = lane_direct_call_this(vm, target, receiver) else { return miss(vm, Miss::Receiver) };
    // SAFETY: as `lane_call_read`.
    if target.entry.answered_by_read() { return unsafe { body_read_call(cx, call, target, this) }; }
    let Some(how) = lane_call_activation(target, call.argc()) else { return miss(vm, Miss::EntryNotActivation) };
    if matches!(how, LaneActivation::Plain) && !lane_activation_room(vm, target) { return miss(vm, Miss::NoActivationRoom); }
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
    let Some(value) = (unsafe { body_read_answer(cx, call, target, this) }) else { return miss(unsafe { vm(cx) }, Miss::CallReadAnswer) };
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
    let mut moved = true;
    let status = native_frames::in_innermost_frame(cx, || unsafe { lane_body_construct_in(cx, call_pc, &mut moved) });
    // A quick planned construction claims a free record of a block the bases already name and
    // makes no collection due, so it moves no base; anything else may have.
    // SAFETY: `context` is the running lane's.
    if moved { unsafe { refresh_body_bases::<H>(context) }; }
    status
}

/// `lane_body_construct` with the context of the running body (`lane_body_call_in`).
///
/// # Safety
/// As `lane_body_call_in`.
unsafe fn lane_body_construct_in<H: Host>(cx: *mut LaneContext<H>, call_pc: u64, moved: &mut bool) -> u64 {
    // SAFETY: the caller's contract.
    match unsafe { BodyCall::at(cx, call_pc) } {
        Some(call) if call.site.constructs() => body_construct(cx, &call, moved),
        _ => miss(unsafe { vm(cx) }, Miss::Form),
    }
}

/// `lane_body_construct` of the decoded `call`, inlined into its one caller so the decoded form is not copied.
#[inline(always)]
fn body_construct<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, moved: &mut bool) -> u64 {
    let vm = unsafe { vm(cx) };
    // A construction's callee is a register: no receiver loads it.
    let Some(callee) = call.callee(vm, Value::UNDEFINED) else { return miss(vm, Miss::Callee) };
    if let Err(refused) = construct_admitted(cx, vm) { return refused; }
    // The quick case, as the lane's own record makes it (`lane_planned_construct`): nothing out of
    // line, no base moved where no collection became due.
    if let Some(this) = planned_inline_quick(vm, callee, call.argument()) {
        call.registers.set(call.site.result, this);
        if vm.heap.should_collect() { note_allocation(vm); } else { *moved = false; }
        return LANE_CALL_COMPLETED;
    }
    // The callee's plan first, the common `new C(...)`, which needs no call target; a returned
    // construction completes by it alone.
    if let Some(this) = constructed_in_place(vm, call, callee) {
        call.registers.set(call.site.result, this);
        return LANE_CALL_COMPLETED;
    }
    // A returning construction takes the plan only: the lane completes it with its callee's result.
    if call.site.returns { return miss(vm, Miss::Returning); }
    construct_activated(cx, call, callee)
}

/// Whether the lane admits a body's construction: it activates script calls and constructs in
/// place, no construction or `with` scope is open, and no collection is due.
fn construct_admitted<H: Host>(cx: *mut LaneContext<H>, vm: &Vm<H>) -> Result<(), u64> {
    if !unsafe { (*cx).script_calls && (*cx).script_constructs } { return Err(miss(vm, Miss::NoScriptCalls)); }
    if vm.construct_target.is_some() { return Err(miss(vm, Miss::Busy)); }
    if vm.with_scope_entered { return Err(miss(vm, Miss::WithScope)); }
    if vm.heap.should_collect() { return Err(miss(vm, Miss::Busy)); }
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
    let Some(target) = lane_construct_target(cx, vm, callee) else { return miss(vm, Miss::Callee) };
    // A forwarding constructor is built by its forwarded initializer's plan or not at all here.
    if target.forwards() { return construct_forwarded(vm, call, callee); }
    if !target.constructs() { return miss(vm, Miss::ConstructNotConstructor); }
    // The activation of the lane's own construction (`lane_script_construct`): a target that
    // keeps or gathers values, or is passed more than it has parameters, by the keeping form.
    let how = match lane_call_activation(&target, call.argc()) {
        None => return miss(vm, Miss::EntryNotActivation),
        Some(how) => how,
    };
    if matches!(how, LaneActivation::Plain) && !lane_activation_room(vm, &target) { return miss(vm, Miss::NoActivationRoom); }
    let Some((this, initialized)) = construct_receiver(cx, call, callee, &target) else { return miss(vm, Miss::Receiver) };
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
    if target.initializes() {
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
        vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee).filter(|target| target.program() == vm.active_program)
    })
}

/// A forwarding constructor's fields, built by its forwarded initializer's plan.
fn construct_forwarded<H: Host>(vm: &mut Vm<H>, call: &BodyCall, callee: Value) -> u64 {
    let Some(this) = lane_forwarded_fields(vm, callee, call.argument()) else { return miss(vm, Miss::NoPlan) };
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
        if resolved.program() != vm.active_program { return None; }
        vm.linked_script_call_entry(callee, link)
    })
}

/// How the lane activates a call of `target` passed `argc` values: its registers alone, as
/// `lane_activate`; by the keeping form where it keeps or gathers values or is passed more than it
/// has parameters; by the environment form. A frame or a settling entry takes the lane's own
/// call, which is `None`.
#[inline]
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
    let refused = || miss(unsafe { vm(cx) }, Miss::NoActivationRoom);
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
            lane_activate_environment(cx, call.registers, call.ip, target, &values, (this, constructs)).map_err(|_| refused())?.0
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
#[inline]
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
    let captures = capture_reads(unsafe { (*cx).registers.capture() });
    let exit = vm.run_nested_lane_method(&method, program, code, registers.0, lane_body_this(target, this), captures, cx.cast());
    // The common end: the body returned and left no native frame, so there is nothing to
    // resume, link or feed back (`lane_body_left` of a return), only the completion to count.
    if let super::super::compiled::LaneExit::Returned { value, .. } = exit
        && !native_frames::left_native_frames(cx)
    {
        lane_record_end(cx, own, &method, exit);
        drop(method);
        super::lane_note_body_end(cx, own, exit, |_| {});
        return lane_body_completed(cx, call.ip, value, constructs);
    }
    let (resumed, feedback) = lane_body_left(cx, registers, own, &method, exit);
    drop(method);
    if lane_leave_counts(cx, exit) {
        super::lane_note_body_end(cx, own, feedback, |left| lane_note_leave(cx, target.function, left));
    } else {
        super::lane_note_warming_leave(cx, own);
    }
    match resumed {
        LaneBodyLeft::Terminated => LANE_CALL_TERMINATED,
        LaneBodyLeft::Resume(_, resume, carried) => leave(resume, unsafe { carried.acc.get() }),
        LaneBodyLeft::Raised(_) => lane_pop_raised(cx, call, constructs),
        LaneBodyLeft::Completed(_, value) => lane_body_completed(cx, call.ip, value, constructs),
    }
}

/// The call whose callee's body raised (`LaneBodyLeft::Raised`): the callee's activation is popped and
/// the call's register keeps its value, which a handler of the caller may read; the exception waits
/// in `Vm::lane_raised` for the caller's body (`LANE_CALL_RAISED`).
fn lane_pop_raised<H: Host>(cx: *mut LaneContext<H>, call: &BodyCall, constructs: bool) -> u64 {
    let kept = call.registers.get(call.site.result);
    let popped = if constructs { lane_complete_activation(cx, call.ip, Value::UNDEFINED).is_some() } else { lane_return_to_body(cx, Value::UNDEFINED).is_some() };
    debug_assert!(popped, "the call made the activation its body raised in");
    call.registers.set(call.site.result, kept);
    LANE_CALL_RAISED
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
