//! A lane body's call of a callee that does not complete in a body of its own: the callee's
//! activation runs in the lane, nested under the body, and its return comes back to the
//! body's call, so the caller's specialized code keeps running (V8's and JSC's baseline
//! callers return into compiled code whatever tier the callee ran in).
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::{LANE_CALL_COMPLETED, LANE_CALL_LEFT, LANE_CALL_MISSED, LANE_CALL_RAISED, LANE_CALL_TERMINATED};
use super::body_forms::{BodyCall, Gathered};
use super::lane_body_stats::{miss, Miss};
use quench_runtime::staged::Arguments;
use quench_runtime::staged::call_body::{concrete_admits_target_program, CallActivation};
use quench_runtime::staged::call_complete::{concrete_call_result, concrete_write_call_result};

/// The end of a nested run at the return with `value` of `frame` (its return words), popped with its
/// caller's state restored: a native's callback's (`Completion::ReturnsToNative`), whose
/// native takes the value, a job's (`Completion::ResumesJob`), whose run ends with it for the
/// job to deliver, or a body's callee's, whose call's register takes it, a construction's
/// receiver (`this`) for a non-object. `None` for any other completion.
#[inline(always)]
pub(crate) fn ends_run(cx: *mut LaneContext, frame: &ActivationView, completion: Completion, this: Value, value: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    match completion {
        Completion::ReturnsToNative | Completion::ResumesJob => {}
        Completion::ReturnsToBody | Completion::ConstructsToBody => {
            let value = setters::completed_value(vm, frame, completion, this, value);
            // SAFETY: the call record belongs to the caller's live view, whose registers the context names.
            let destination = unsafe { (*frame.call()).a } & quench_runtime::bytecode::REGISTER_MASK;
            quench_runtime::staged::call_complete::concrete_write_call_result(unsafe { (*cx).registers }.0.expose_provenance() as u64, destination, value);
        }
        _ => return None,
    }
    vm.lane_completion.set(LaneCompletion::NativeReturned(value));
    Some(Resumed::Exit(exit(frame.call())))
}

/// Whether `activation`, which becomes the frame at index `frame`, is the entry frame of
/// the general loop that finishes it, as a native's callback or a job's is. A body's callee is not:
/// its frame's return leaves the lane instead (`LaneStack::body_boundary`), which the
/// innermost such frame decides.
pub(crate) fn enters_as_frame(stack: &mut LaneStack, activation: &Activation, frame: usize) -> bool {
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
pub(crate) fn left_lane(resume: Ip) -> bool {
    resume.addr() & EXIT_TAG != 0
}

/// Changes the completion of the lane's `depth`th activation `from` one `to` another, where it
/// is (`LaneStack::recomplete_at`: a header's side record, or a record); false, changing
/// nothing, where it holds another or is no script activation.
fn recomplete(cx: *mut LaneContext, depth: usize, from: Completion, to: Completion) -> bool {
    unsafe { vm(cx) }.lane_stack.recomplete_at(depth, from, to)
}

/// Leaves the body for the lane at `resume`, the callee's activation's record
/// (`LaneExit::Callee`).
fn leave(cx: *mut LaneContext, resume: Ip, acc: Value) -> u64 {
    unsafe { (*cx).callee_resume = resume; (*cx).callee_acc = acc; }
    LANE_CALL_LEFT
}

/// The rest of the activation a body's call made, the lane's `depth`th, from its record
/// `resume` with accumulator `acc`: run in the lane to its return, which completes the call
/// (`LANE_CALL_COMPLETED`). A run that leaves the lane for the general path leaves the body
/// too, at the record the lane left at (`left_lane`), as does a call no nested run takes:
/// one below such a leave, or without native stack.
pub(crate) fn finish(cx: *mut LaneContext, depth: usize, resume: Ip, acc: Value, constructs: bool) -> u64 {
    let (plain, nested) = if constructs { (Completion::Constructs, Completion::ConstructsToBody) } else { (Completion::Plain, Completion::ReturnsToBody) };
    let Ok(_stack) = quench_runtime::stack::StackGuard::enter() else { return leave(cx, resume, acc) };
    if left_lane(resume) || !recomplete(cx, depth, plain, nested) { return leave(cx, resume, acc); }
    let left = run_nested(cx, resume, acc);
    recomplete(cx, depth, nested, plain);
    ended(cx, left)
}

/// The status of a body's call whose nested run ended as `run_nested` says.
fn ended(cx: *mut LaneContext, left: Option<Ip>) -> u64 {
    left.map_or(LANE_CALL_COMPLETED, |left| leave(cx, exit(left), Value::UNDEFINED))
}

/// The lane run from `resume` with accumulator `acc`, nested under the running body: `None`
/// where it ended with a value for the body's call (`ends_run`), else the record the lane
/// left at.
fn run_nested(cx: *mut LaneContext, resume: Ip, acc: Value) -> Option<Ip> {
    let vm = unsafe { vm(cx) };
    unsafe {
        (*cx).resumed_acc = Slot::of(acc);
        (*cx).memory = MemoryView::script(cx);
    }
    let outer = vm.lane_stack.body_boundary.take();
    let left = dispatch_in_lane(cx, resume);
    vm.lane_stack.body_boundary = outer;
    vm.lane_completion.take_native_returned().is_none().then_some(left)
}

/// A body's call of an array callback method whose first callback's activation the body made
/// (`array_steps::body_array_iteration_in_lane`), its record `resume`: run in the lane until the
/// iteration completes the call (`LANE_CALL_COMPLETED`); a run that leaves the lane for the
/// general path leaves the body at the record it left at.
pub(crate) fn finish_array_iteration(cx: *mut LaneContext, resume: Ip) -> u64 {
    ended(cx, run_nested(cx, resume, Value::UNDEFINED))
}

/// A lane body's call, by the call's decoded form (`BodyCall`: a register call, a known call or a
/// method call): the callee that completes in its own body returns its value to the caller's native
/// code; one that leaves continues in the lane in its own activation; a native that completes in
/// place (`native_calls::body_native_call_in_lane`) does so too. A callee loaded from a method site's
/// caches is the one the interpreter would call. Any other callee or case is a miss, which leaves
/// the body at the call, before any effect.
pub(crate) unsafe extern "C" fn body_call_helper(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    // Static regions recover ordinary calls through their linked semantic operation.
    // Binding this JIT-only helper to a miss removes its interpreter closure entirely.
    if !quench_runtime::DYNAMIC_EXECUTION_ENABLED { return LANE_CALL_MISSED; }
    let cx = context as usize as *mut LaneContext;
    if quench_runtime::trace::Channel::BodyLeave.on() { super::lane_body_stats::take_decline(); }
    let status = native_frames::in_innermost_frame(cx, || unsafe { body_call_in_lane(cx, call_pc) });
    // SAFETY: `context` is the running lane's.
    unsafe { refresh_body_bases(context) };
    note_call_status(cx, "call", call_pc, status);
    status
}

/// With `body-leave` tracing, names a body helper's call that did not complete in place.
fn note_call_status(cx: *mut LaneContext, helper: &str, call_pc: u64, status: u64) {
    if status != LANE_CALL_COMPLETED && quench_runtime::trace::Channel::BodyLeave.on() {
        let why = if status == LANE_CALL_MISSED { super::lane_body_stats::take_decline() } else { "" };
        eprintln!("QUENCH_CALL_STATUS helper={helper} function={} pc={call_pc} status={status} why={why}", running_function_in_lane(cx));
    }
}

/// `body_call_helper` with the context of the running body, which a native callee's frame made the
/// innermost (`native_frames::in_innermost_frame`).
///
/// # Safety
/// As `body_call_helper`; `call_pc` is a record of the running body's function.
unsafe fn body_call_in_lane(cx: *mut LaneContext, call_pc: u64) -> u64 {
    // SAFETY: the caller's contract.
    match unsafe { BodyCall::at(cx, call_pc) } {
        Some(call) => body_call(cx, &call),
        None => miss(unsafe { vm(cx) }, Miss::Form),
    }
}

/// `body_call_helper` of the decoded `call`.
fn body_call(cx: *mut LaneContext, call: &BodyCall) -> u64 {
    let vm = unsafe { vm(cx) };
    let Some(receiver) = call.receiver(vm) else { return miss(vm, Miss::Receiver) };
    let Some(callee) = call.callee(vm, receiver) else { return miss(vm, Miss::Callee) };
    if let Err(refused) = call_admitted(cx, vm, call, callee) { return refused; }
    let Some(target) = body_call_target_in_lane(cx, vm, callee, call.link()) else {
        return super::native_calls::body_native_call_in_lane(cx, call, callee, receiver);
    };
    super::proven_callees::rebind_body_call(cx, vm, call.argc(), target);
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target or collects.
    body_script_call(cx, call, unsafe { &*target }, receiver)
}

/// Whether the lane admits a body's call of `callee` at all: it may activate script calls, the
/// callee is not being constructed, no `with` scope is open, and a returning call may complete.
fn call_admitted(cx: *mut LaneContext, vm: &Vm, call: &BodyCall, _callee: Value) -> Result<(), u64> {
    // One compare of the admission word; the miss names which condition refused.
    if !vm.lane_stack.admits_program(vm.active_program) {
        let reason = if !vm.lane_stack.script_calls_allowed() { Miss::NoScriptCalls } else { Miss::CallConstructing };
        return Err(miss(vm, reason));
    }
    if vm.with_scope_entered { return Err(miss(vm, Miss::WithScope)); }
    if call.site.returns && !returning_call_admitted(cx) { return Err(miss(vm, Miss::CallReturningNotAdmitted)); }
    Ok(())
}

/// The miss of a call of `target` whose body is retired to `Declined`: it has no body to run, so the body leaves at the
/// call before any activation is pushed, and the interpreter makes the call. `body_run_in_lane` would push the
/// activation and then leave with it. `None` where the callee has a body, or no view.
fn declined_callee_miss(cx: *mut LaneContext, target: &ScriptCallTarget) -> Option<u64> {
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(target.function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view)?;
    if !own.declined_in_lane() { return None; }
    // The demand a native caller makes of a declined callee, as `body_run_in_lane` makes it.
    let rest_view = unsafe { &*(*cx).code }.function_get(target.function as usize).is_some_and(|function| function.rest_view());
    if !rest_view && !calling_body_in_lane(cx) { own.note_native_caller(); }
    Some(miss(unsafe { vm(cx) }, Miss::CalleeDeclined))
}

/// Whether the running body's caller has a body of its own (`body_site`), which planned the call already (`bind_callees`).
fn calling_body_in_lane(cx: *mut LaneContext) -> bool {
    unsafe { (*cx).body_site }.and_then(|(_, function, _, _)| unsafe { &*(*cx).views }.get(function as usize))
        .and_then(quench_runtime::vm::program_store::derived_lane_view).is_some_and(|caller| caller.body_view_in_lane().is_some())
}

/// A body's call of the script function `target`, binding `receiver`: answered by its read, else
/// activated as the lane's own call would.
fn body_script_call(cx: *mut LaneContext, call: &BodyCall, target: &ScriptCallTarget, receiver: Value) -> u64 {
    let vm = unsafe { vm(cx) };
    let Some(this) = direct_call_this_in_lane(vm, target, receiver) else { return miss(vm, Miss::Receiver) };
    // SAFETY: as `call_read_in_lane`.
    if target.entry.answered_by_read() {
        // SAFETY: as `call_read_in_lane`.
        if let Some(value) = unsafe { body_read_answer(cx, call, target, this) } {
            call.registers.set(call.site.result, value);
            return LANE_CALL_COMPLETED;
        }
        // The read missed (a receiver its layout does not name): the body runs as the registers-only
        // activation such an entry makes then (`CallEntry::activates_registers_only`), not a leave.
        let fixed = call.argc() <= target.params.min(FIXED_ARGUMENT_COPIES);
        let how = quench_runtime::staged::call_body::concrete_activation(target.entry.activates_registers_only(), target.forwards(), fixed, false);
        if !how.plain { return miss(vm, Miss::CallReadAnswer); }
        if !activation_room_in_lane(vm, target) { return miss(vm, Miss::NoActivationRoom); }
        if let Some(missed) = declined_callee_miss(cx, target) { return missed; }
        return body_activate_in_lane(cx, call, target, this, false, how);
    }
    let Some(how) = activation_form(target, call.argc()) else { return miss(vm, Miss::EntryNotActivation) };
    if how.plain && !activation_room_in_lane(vm, target) { return miss(vm, Miss::NoActivationRoom); }
    if let Some(missed) = declined_callee_miss(cx, target) { return missed; }
    body_activate_in_lane(cx, call, target, this, false, how)
}

/// The value `target`'s read answers `call` with: memoized at the record where its layout has a
/// memo form (a register call's window), else read from the values it passes.
///
/// # Safety
/// As `call_read_in_lane`.
unsafe fn body_read_answer(cx: *mut LaneContext, call: &BodyCall, target: &ScriptCallTarget, this: Value) -> Option<Value> {
    if let (true, Arguments::Window { first, count }) = (call.memoizes(), call.site.arguments) {
        let passed = ReadArguments { registers: call.registers, base: first, passed: count };
        return unsafe { answered_call_in_lane(cx, call.ip, target, passed, this) };
    }
    // SAFETY: the registers do not move while the read runs.
    let values = unsafe { call.values() };
    let passed = ReadArguments { registers: Registers(values.as_ptr().cast_mut()), base: 0, passed: call.argc() };
    unsafe { read_answer_in_lane(cx, target, passed, this) }
}

/// A lane body's `new`, as the lane's own construction does (`script_construct_miss`): the
/// callee's plan, else the receiver its target allocates and, where its body does more
/// than store fields, its activation, whose completion yields its returned object or the
/// receiver. A returned construction takes the plan only. Its results are `body_call_helper`'s.
pub(crate) unsafe extern "C" fn body_construct_helper(_vm: u64, _residual: u64, context: u64, call_pc: u64) -> u64 {
    // As `body_call_helper`: static regions recover constructions through their semantic operation.
    if !quench_runtime::DYNAMIC_EXECUTION_ENABLED { return LANE_CALL_MISSED; }
    let cx = context as usize as *mut LaneContext;
    let mut moved = true;
    if quench_runtime::trace::Channel::BodyLeave.on() { super::lane_body_stats::take_decline(); }
    let status = native_frames::in_innermost_frame(cx, || unsafe { body_construct_in_lane(cx, call_pc, &mut moved) });
    // A quick planned construction claims a free record of a block the bases already name and
    // makes no collection due, so it moves no base; anything else may have.
    // SAFETY: `context` is the running lane's.
    if moved { unsafe { refresh_body_bases(context) }; }
    note_call_status(cx, "construct", call_pc, status);
    status
}

/// `body_construct_helper` with the context of the running body (`body_call_in_lane`).
///
/// # Safety
/// As `body_call_in_lane`.
unsafe fn body_construct_in_lane(cx: *mut LaneContext, call_pc: u64, moved: &mut bool) -> u64 {
    // SAFETY: the caller's contract.
    match unsafe { BodyCall::at(cx, call_pc) } {
        Some(call) if call.site.constructs() => body_construct(cx, &call, moved),
        _ => miss(unsafe { vm(cx) }, Miss::Form),
    }
}

/// `body_construct_helper` of the decoded `call`, inlined into its one caller so the decoded form is not copied.
#[inline(always)]
fn body_construct(cx: *mut LaneContext, call: &BodyCall, moved: &mut bool) -> u64 {
    let vm = unsafe { vm(cx) };
    // A construction's callee is a register: no receiver loads it.
    let Some(callee) = call.callee(vm, Value::UNDEFINED) else { return miss(vm, Miss::Callee) };
    if let Err(refused) = construct_admitted(cx, vm) { return refused; }
    // The quick case, as the lane's own record makes it (`planned_construct_miss`): nothing out of
    // line, no base moved where no collection became due.
    if let Some(this) = super::construct_plan::planned_inline_quick(vm, callee, call.argument()) {
        call.registers.set(call.site.result, this);
        if vm.collection_due() { note_allocation(vm); } else { *moved = false; }
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
fn construct_admitted(cx: *mut LaneContext, vm: &Vm) -> Result<(), u64> {
    if !quench_runtime::staged::call_construct::concrete_admits_construct_run(unsafe { (*cx).script_calls }, unsafe { (*cx).script_constructs }) {
        return Err(miss(vm, Miss::NoScriptCalls));
    }
    if !quench_runtime::staged::call_construct::concrete_unpending(vm.construct_target.is_some()) { return Err(miss(vm, Miss::Busy)); }
    if vm.with_scope_entered { return Err(miss(vm, Miss::WithScope)); }
    if vm.collection_due() { return Err(miss(vm, Miss::Busy)); }
    Ok(())
}

/// The receiver `callee`'s plan finishes (`planned_construction`, or a forwarding constructor's
/// fields), else, for `new Array()` of the realm's own `Array`, ArrayCreate(0), an empty array of
/// `Array.prototype`, which calls nothing.
fn constructed_in_place(vm: &mut Vm, call: &BodyCall, callee: Value) -> Option<Value> {
    let this = planned_construction(vm, callee, call.argument())
        .or_else(|| forwarded_fields_in_lane(vm, callee, call.argument()))
        .or_else(|| (call.argc() == 0 && vm.realm_array_constructor(callee)).then(|| vm.hole_array(0)))?;
    note_allocation(vm);
    Some(this)
}

/// A body's construction of `callee` by its constructor's own activation: the receiver its target
/// allocates (or its initializer finishes) and, where its body does more than store fields, the
/// activation.
fn construct_activated(cx: *mut LaneContext, call: &BodyCall, callee: Value) -> u64 {
    let vm = unsafe { vm(cx) };
    let Some(target) = construct_target_in_lane(cx, vm, callee) else { return miss(vm, Miss::Callee) };
    // A forwarding constructor is built by its forwarded initializer's plan or not at all here.
    if target.forwards() { return construct_forwarded(vm, call, callee); }
    if !target.constructs() { return miss(vm, Miss::ConstructNotConstructor); }
    // The activation of the lane's own construction (`script_construct_miss`): a target that
    // keeps or gathers values, or is passed more than it has parameters, by the keeping form.
    let how = match activation_form(&target, call.argc()) {
        None => return miss(vm, Miss::EntryNotActivation),
        Some(how) => how,
    };
    if how.plain && !activation_room_in_lane(vm, &target) { return miss(vm, Miss::NoActivationRoom); }
    // Before the receiver is allocated: a declined constructor's construction is the interpreter's, with no effect here.
    if let Some(missed) = declined_callee_miss(cx, &target) { return missed; }
    let Some((this, initialized)) = construct_receiver(cx, call, callee, &target) else { return miss(vm, Miss::Receiver) };
    // Allocating the receiver can make a collection due.
    note_allocation(vm);
    if initialized {
        call.registers.set(call.site.result, this);
        return LANE_CALL_COMPLETED;
    }
    body_activate_in_lane(cx, call, &target, this, true, how)
}

/// The receiver of a construction by `target`, with whether its initializer's cached adds finished
/// its fields: the allocation of `callee`'s prototype, or the finished receiver.
fn construct_receiver(cx: *mut LaneContext, call: &BodyCall, callee: Value, target: &ScriptCallTarget) -> Option<(Value, bool)> {
    let vm = unsafe { vm(cx) };
    if target.initializes() {
        // SAFETY: the context's program outlives the lane run.
        initialize_fields_in_lane(vm, unsafe { &*(*cx).code }, callee, callee, target.function, call.argument())
    } else {
        vm.inline_construct_receiver(callee).map(|this| (this, false))
    }
}

/// The target a lane body's `new` of `callee` activates: its cached entry, else the one the
/// lane's own construction resolves for a function of the running program.
fn construct_target_in_lane(cx: *mut LaneContext, vm: &mut Vm, callee: Value) -> Option<ScriptCallTarget> {
    // The resolution itself is the VM's (`resolve_script_call_target`); the target's admission
    // is the program row (`staged::call_body::admits_target_program`).
    // SAFETY: as `Vm::script_call_entry`; the target is copied before anything allocates.
    vm.script_call_entry(callee).map(|entry| unsafe { *entry }).or_else(|| {
        // SAFETY: the context's program outlives the lane run.
        vm.resolve_script_call_target(unsafe { &*(*cx).code }, callee)
            .filter(|target| concrete_admits_target_program(target.program().raw(), vm.active_program.raw()))
    })
}

/// A forwarding constructor's fields, built by its forwarded initializer's plan.
fn construct_forwarded(vm: &mut Vm, call: &BodyCall, callee: Value) -> u64 {
    let Some(this) = forwarded_fields_in_lane(vm, callee, call.argument()) else { return miss(vm, Miss::NoPlan) };
    note_allocation(vm);
    call.registers.set(call.site.result, this);
    LANE_CALL_COMPLETED
}

/// The target a lane body's call of `callee` activates: the site's linked entry, else the one
/// the lane's call resolves for a script function of the running program
/// (`script_call_resolve_miss`: `Vm::resolve_script_call_target`), which links the site.
fn body_call_target_in_lane(cx: *mut LaneContext, vm: &mut Vm, callee: Value, link: &std::cell::Cell<u16>) -> Option<*const ScriptCallTarget> {
    // The linked entry is the admission (`linked_script_call_entry`); its miss resolves through
    // the VM and admits a target of the running program (`staged::call_body::admits_target_program`).
    vm.linked_script_call_entry(callee, link).or_else(|| {
        if matches!(vm.function_record(callee), Some(FunctionRecord { kind: FunctionKind::Native(_), .. })) || vm.cross_call_entry(callee).is_some() {
            return None;
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let resolved = vm.resolve_script_call_target(code, callee)?;
        if !concrete_admits_target_program(resolved.program().raw(), vm.active_program.raw()) { return None; }
        vm.linked_script_call_entry(callee, link)
    })
}

/// How the lane activates a call of `target` passed `argc` values, the activation row's three
/// exclusive conditions (`staged::call_body::activation`): its registers alone (`plain`), as
/// `activate_in_lane`; by the keeping form (`keeping`) where it keeps or gathers values or is passed
/// more than it has parameters; by the environment form (`environment`, a window holding its
/// environment and locals). A frame or a settling entry takes the lane's own call, which is
/// `None`, where none holds. The one chooser of the lane's own calls and of a body's: the interpreter's
/// `script_call_full_miss` reads `is_some` for its activation test, where a target that forwards or
/// is an environment is already routed away.
#[inline]
pub(crate) fn activation_form(target: &ScriptCallTarget, argc: u16) -> Option<CallActivation<bool>> {
    let fixed = argc <= target.params.min(FIXED_ARGUMENT_COPIES);
    let form = quench_runtime::staged::call_body::concrete_activation(target.entry.activates(), target.forwards(), fixed, matches!(target.entry, CallEntry::Environment));
    (form.plain || form.keeping || form.environment).then_some(form)
}

/// The `this` a callee's body is entered with: the deleted marker where it reads a lexical one.
#[inline]
fn body_this_in_lane(target: &ScriptCallTarget, this: Value) -> Value {
    Value::from_bits(quench_runtime::staged::call_body::concrete_body_this(this.to_bits(), target.this_binding == ThisBinding::Lexical))
}

/// The activation of `target` a lane body's call (`constructs`: `new`) makes, run by the target's
/// own body where it has one (`body_run_in_lane`). It is pushed by the form the activation row chose
/// (`how`, `activation_form`: `staged::call_body::activation`), consumed here, each form's
/// words its push's: `push_plain` (`BodyCall::push_plain`), `activate_keeping_in_lane`,
/// `activate_environment_in_lane`; a form that routes the call to the general path is the helper's
/// miss.
fn body_activate_in_lane(cx: *mut LaneContext, call: &BodyCall, target: &ScriptCallTarget, this: Value, constructs: bool, how: CallActivation<bool>) -> u64 {
    if record_late_call(cx, call.ip, target, call.argc()) { return miss(unsafe { vm(cx) }, Miss::Callee) }
    let refused = || miss(unsafe { vm(cx) }, Miss::NoActivationRoom);
    let registers = if how.plain {
        call.push_plain(cx, target, this, constructs)
    } else if how.keeping {
        // The activation the lane's own call makes where values are kept or gathered.
        let mut gathered = Gathered::new();
        let arguments = call.kept(&mut gathered);
        match activate_keeping_in_lane::<false>(cx, call.registers, call.ip, target, arguments, this, constructs, None) {
            Ok(pushed) => pushed.0,
            Err(_) => return refused(),
        }
    } else {
        debug_assert!(how.environment, "activation_form admits one of the three forms");
        // SAFETY: the call's values are registers of the running function.
        let values = unsafe { call.values() };
        match activate_environment_in_lane(cx, call.registers, call.ip, target, &values, (this, constructs)) {
            Ok(pushed) => pushed.0,
            Err(_) => return refused(),
        }
    };
    body_run_in_lane(cx, call, target, this, constructs, registers)
}

/// The run of the activation a body's call pushed at `registers` by the target's own body, warmed
/// toward one first where it has none, as the lane's own calls warm it (`specialize_in_lane`: a
/// constructor only body code constructs is entered by nothing else): its completion writes the
/// call's register (`LANE_CALL_COMPLETED`), and any other end leaves the lane in the activation
/// (`LANE_CALL_LEFT`). Its words are the entered `this` (`body_this_in_lane`, `call_body::body_this`)
/// and the return to the call (`body_returned_in_lane`).
fn body_run_in_lane(cx: *mut LaneContext, call: &BodyCall, target: &ScriptCallTarget, this: Value, constructs: bool, registers: Registers) -> u64 {
    let vm = unsafe { vm(cx) };
    // A leave resumes at a record of the callee's view, never at its tier prologue, which
    // lies outside its records: a loop region's `Callee` exit (`loop_native_in_lane`) leaves the
    // lane there and needs its PC. The warming below takes the prologue's step.
    let start = unsafe { (*cx).base };
    // The activation's index among every frame, which `finish` finds it at where it is.
    let depth = vm.lane_stack.activation_count() - 1;
    let leave = move |resume: Ip, acc: Value| finish(cx, depth, resume, acc, constructs);
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(target.function as usize).and_then(quench_runtime::vm::program_store::derived_lane_view);
    let Some(own) = own else {
        if quench_runtime::trace::Channel::BodyLeave.on() { eprintln!("QUENCH_CALLEE_LEAVE callee={} why=no-view", target.function); }
        return leave(start, Value::UNDEFINED)
    };
    let respecializes = own.respecializes();
    if respecializes || own.body_view_in_lane().is_none() { specialize_in_lane(cx, own, target.function, respecializes, &[], None); }
    let Some(method) = own.body_view_in_lane().filter(|method| body_admits_in_lane(cx, method, NO_PENDING_ACTIVATIONS)) else {
        unsafe { (*cx).callee_warming = own.warming() };
        // Each call of it from native code leaves that code until it has a body (`note_native_caller`).
        // Not a rest view yet (TOD-2052): its body, entered from a region's call, spreads its kept rest values
        // with no frame of its own to read them from.
        let rest_view = unsafe { &*(*cx).code }.function_get(target.function as usize).is_some_and(|function| function.rest_view());
        // A caller with a body of its own planned this call already (`bind_callees`); a loop region did not.
        let caller_body = calling_body_in_lane(cx);
        if own.body_view_in_lane().is_none() && !rest_view && !caller_body { own.note_native_caller(); }
        if quench_runtime::trace::Channel::BodyLeave.on() {
            let why = if own.body_view_in_lane().is_some() { "not-admitted" } else if own.warming() { "warming" } else { "no-body" };
            eprintln!("QUENCH_CALLEE_LEAVE callee={} why={why} tier={}", target.function, own.tier_note());
        }
        return leave(start, Value::UNDEFINED);
    };
    let (program, code) = unsafe { ((*cx).program, &*(*cx).code) };
    let captures = capture_reads(unsafe { (*cx).registers.capture() });
    let exit = vm.run_nested_lane_method(&method, program, code, registers.0, body_this_in_lane(target, this), captures, cx.cast());
    // The common end: the body returned and left no native frame, so there is nothing to
    // resume, link or feed back (`body_left_in_lane` of a return), only the completion to count.
    if let crate::compiled::LaneExit::Returned { value, .. } = exit
        && !native_frames::left_native_frames(cx)
    {
        record_end_in_lane(cx, own, &method, exit);
        drop(method);
        super::note_body_end_in_lane(cx, own, exit, |_| {});
        return body_returned_in_lane(cx, call, Some(value), constructs);
    }
    if quench_runtime::trace::Channel::BodyLeave.on() { eprintln!("QUENCH_CALLEE_EXIT callee={} exit={exit:?}", target.function); }
    let (resumed, feedback) = body_left_in_lane(cx, registers, own, &method, exit);
    drop(method);
    if leave_counts_in_lane(cx, exit) {
        super::note_body_end_in_lane(cx, own, feedback, |left| note_leave_in_lane(cx, target.function, left));
    } else {
        super::note_warming_leave_in_lane(cx, own);
    }
    match resumed {
        LaneBodyLeft::Terminated => LANE_CALL_TERMINATED,
        LaneBodyLeft::Resume(_, resume, carried) => leave(resume, unsafe { carried.acc.get() }),
        LaneBodyLeft::Raised(_) => body_returned_in_lane(cx, call, None, constructs),
        LaneBodyLeft::Completed(_, value) => body_returned_in_lane(cx, call, Some(value), constructs),
    }
}

/// The return of the activation a body's call made to that call, popped through the lane's
/// return (`complete_activation_in_lane` for a construction, `return_to_body_in_lane`): with
/// `Some(value)`, its completion, a construction's yielding its returned object or the receiver
/// (`LANE_CALL_COMPLETED`); with `None`, its body raised (`LaneBodyLeft::Raised`): the pop returns
/// `undefined` and the call's register keeps its value across it
/// (`staged::call_complete::call_result`, then `write_call_result`), which a handler of the
/// caller may read, the exception waiting in `Vm::lane_raised` for the caller's body
/// (`LANE_CALL_RAISED`).
fn body_returned_in_lane(cx: *mut LaneContext, call: &BodyCall, completed: Option<Value>, constructs: bool) -> u64 {
    let registers = call.registers.0.expose_provenance() as u64;
    let kept = completed.is_none().then(|| concrete_call_result(registers, call.site.result));
    let value = completed.unwrap_or(Value::UNDEFINED);
    let popped = if constructs { complete_activation_in_lane(cx, call.ip, value).is_some() } else { return_to_body_in_lane(cx, value).is_some() };
    debug_assert!(popped, "the call made the activation it returns from");
    let Some(kept) = kept else { return LANE_CALL_COMPLETED };
    concrete_write_call_result(registers, call.site.result, kept);
    LANE_CALL_RAISED
}
