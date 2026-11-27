//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
use super::*;

/// The environment of the environment window whose entries start at `entries`
/// (`WINDOW_ENVIRONMENT`): the one already made, or a new environment of `function` of
/// `program` under `parent` that takes over the slots the window held in place, its
/// captured ones from the locals, which then hold `DELETED` for them as a frame's do. The
/// window reaches the new storage from then on. The environment binds the activation's
/// `this`, where its closures' arrows find it (`Vm::captured_lexical_this`), as a frame's
/// promotion carries its bindings; an arrow's own `this` is that same lexical value. The
/// environment of a window over a record also takes over the activation's dynamic `bindings`
/// (`FrameRecord::dynamic_bindings`, `Activation::take_record_bindings`), as a frame's
/// promotion takes its own (`Vm::promote_frame_environment`); any other passes none.
///
/// # Safety
/// `entries` starts the entries of a live environment window (`activate_environment_in_lane`).
pub(super) unsafe fn window_environment(
    vm: &mut Vm,
    code: &ResidualProgram,
    entries: *mut Value,
    function: u32,
    parent: Value,
    program: ProgramId,
    this: Value,
    bindings: Option<Vec<(quench_runtime::bytecode::Atom, Value)>>,
) -> Value {
    // SAFETY: as the caller guarantees.
    unsafe {
        let made = *entries.add(usize::from(WINDOW_ENVIRONMENT));
        if !made.is_deleted() {
            return made;
        }
        let slots = window_slots_taken(entries);
        // A non-arrow binds its `this` where its closures' arrows find it
        // (`Vm::captured_lexical_this`), as a frame does when a closure or the body itself
        // can observe its implicit bindings (`Function::activation_bindings_observable`);
        // any other scope names only its program, which needs no allocation.
        let mut bindings = bindings.unwrap_or_default();
        let body = code.function(function as usize);
        if !body.is_arrow && body.activation_bindings_observable(code) && body.maker_bindings_read(code) {
            bindings.insert(0, (vm.runtime_atoms.lexical_this, this));
        }
        let scope = window_scope_ref(program, bindings);
        let (environment, storage) = vm.allocate_environment(parent, function, slots, scope);
        // Its storage links to its parent's for its closures' outer captures
        // (`EnvironmentSlots::link`), as a capture walk would find the parent's.
        if let Some(quench_runtime::tier_surface::EnvironmentRecord { function: owner, slots: parent_slots, .. }) = vm.heap_reads().environment(parent)
            && *owner != u32::MAX
            && *owner != quench_runtime::vm::ROOT_FUNCTION_ID
            && let Some(parent_storage) = parent_slots.dense_storage()
            && let Some(own) = (*storage).dense_storage()
        {
            quench_runtime::tier_surface::EnvironmentSlots::link(own, parent_storage, vm.heap_reads().collection_epoch());
        }
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(environment);
        entries
            .add(usize::from(WINDOW_SLOTS))
            .write(std::mem::transmute::<u64, Value>(storage as u64));
        environment
    }
}

/// The slots of an environment made from the window whose entries are `entries`: the locals a
/// closure captured, each cleared in the window, which the environment owns from then on.
///
/// # Safety
/// `entries` starts the entries of a live environment window (`activate_environment_in_lane`).
pub(super) unsafe fn window_slots_taken(entries: *mut Value) -> quench_runtime::tier_surface::EnvironmentSlots {
    // SAFETY: as the caller guarantees.
    unsafe {
        let captured = std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_CAPTURED)));
        let in_place = &*(std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_SLOTS)))
            as *const quench_runtime::tier_surface::EnvironmentSlots);
        let locals = entries.add(usize::from(WINDOW_LOCALS));
        let window = EnvironmentWindow { locals: in_place.len() as u16, captured, rest: false, self_binding: None, parameter_registers: 0 };
        // SAFETY: the mask names locals of the window, which hold owned values.
        let slots = quench_runtime::tier_surface::EnvironmentSlots::from_captured(in_place.len(), window.captured, locals);
        // Each captured local, one per bit of the mask, which names no slot past the locals.
        debug_assert_eq!(captured.checked_shr(u32::from(window.locals)).unwrap_or(0), 0);
        let mut remaining = captured;
        while remaining != 0 {
            locals.add(remaining.trailing_zeros() as usize).write(Value::DELETED);
            remaining &= remaining - 1;
        }
        slots
    }
}

/// The scope of a window's environment: its program's alone, which needs no allocation, or
/// holding the dynamic `bindings` it takes over.
pub(super) fn window_scope_ref(program: ProgramId, bindings: Vec<(quench_runtime::bytecode::Atom, Value)>) -> quench_runtime::tier_surface::EnvironmentScopeRef {
    if bindings.is_empty() {
        return quench_runtime::tier_surface::EnvironmentScopeRef::of_program(program.raw());
    }
    quench_runtime::tier_surface::EnvironmentScopeRef::new(quench_runtime::tier_surface::EnvironmentScope {
        program: Some(program.raw()),
        root_eval_scope: false,
        binding_site_pc: None,
        dynamic_bindings: bindings.into(),
        with_objects: Box::default(),
    })
}

/// The body of [[Construct]] for a constructor that only stores its parameters into fields
/// of `this` (`ScriptCallTarget::initializes`): its receiver
/// (`Vm::inline_construct_receiver`), and whether every store was a cached add
/// (`Vm::primary_field_add_hit`), which runs no guest code, so the body completed. After a
/// miss the constructor activates on the receiver: its body repeats the stores made so far
/// as stores of the same values to the own data properties they added, which leaves the
/// object as one activation would. A body whose adds all hit leaves its `ConstructPlan`,
/// with which a later construct allocates the finished object at once
/// (`planned_construction_in_lane`), which comes first. Out of line, so the construct keeps
/// its tail dispatch.
#[inline(never)]
pub(super) fn initialize_fields_in_lane(
    vm: &mut Vm,
    code: &ResidualProgram,
    callee: Value,
    constructor: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
) -> Option<(Value, bool)> {
    let stores = code.function(function as usize).initializer_stores(&code.constants)?;
    let env = vm.same_realm_user_target(callee).map_or(Value::UNDEFINED, |(_, _, env)| env);
    initialize_stores_in_lane(vm, callee, constructor, env, |vm, this, nested, each| {
        add_stores_in_lane(vm, code, this, stores, argument, nested, each)
    })
}

/// The receiver `Vm::inline_construct_receiver` allocates for a constructor that forwards its
/// arguments (`script_construct_keeping_miss`): from `callee`'s holding plan `plan` where it
/// has no fields, else from the template, which leaves that plan. `script_construct_miss` sends a
/// forwarding target here before it tests `ScriptCallTarget::initializes`, so a callee's plan
/// is only ever this one or `initialize_fields_in_lane`'s.
pub(super) fn planned_receiver_in_lane(
    vm: &mut Vm,
    callee: Value,
    plan: Option<&ConstructPlan>,
) -> Option<Value> {
    if let Some(plan) = plan.filter(|plan| plan.fields == 0) {
        return Some(plan.allocate(vm, |_| Value::UNDEFINED));
    }
    let template = vm.inline_construct_template(callee)?;
    let (prototype, shape) = (template.proto(), template.shape());
    let this = vm.allocate_object(template);
    if let Some(index) = construct_plan_index(callee)
        && let Some(callee_layout) = vm.function_prototype_layout(callee)
    {
        vm.lane_stack.construct_plans[index] = ConstructPlan {
            callee,
            callee_layout,
            prototype,
            shape,
            kind: ConstructPlanKind::Forwarding,
            ..EMPTY_CONSTRUCT_PLAN
        }
        .finish();
        watch_construct_plan(vm, index, true, &[callee, prototype]);
    }
    Some(this)
}

/// `activate_in_lane` for a target that keeps its passed values, or for values a forwarding
/// activation or frame kept: the activation's registers and the callee's first record, or
/// the record to continue at when the call takes the general path. Out of line, so the
/// common activation keeps its code. Without `MAY_CROSS` its callers route the running
/// program's targets, as every path but a plain call's makes them (`script_target_in_lane`),
/// so it takes that fact as proven.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn activate_keeping_in_lane<const MAY_CROSS: bool>(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    arguments: ActivationArguments,
    this: Value,
    constructs: bool,
    forwarder: Option<SkippedForwarder>,
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    // A plain call passed more values than its parameters activates another program's callee
    // here too (`activate_in_lane`).
    let cross = crosses_in_lane::<MAY_CROSS>(cx, target);
    debug_assert!(!(cross && forwarder.is_some()));
    let argc = match arguments {
        ActivationArguments::Window(_) => usize::from(ImmediateLayout::argument_count(i.imm())),
        ActivationArguments::Kept(_, count) => count,
    };
    #[cfg(feature = "profile-lane-exits")]
    if let Some(body) = vm.programs.residual(target.program()).and_then(|code| {
        let body = code.function_get(target.function as usize)?;
        let plan = body.call_plan(code);
        Some(format!(
            "keeping {} @{:?} entry={:?} argc={argc} params={} strict={} legacy={} forwards={} arguments={}",
            body.name.map(|atom| code.atoms[atom as usize].to_string()).unwrap_or_default(),
            body.source_span.map(|span| span.start), target.entry, target.params, body.strict,
            plan.legacy_reflection, plan.forwards_arguments, body.arguments_slot.is_some(),
        ))
    }) {
        quench_runtime::vm::lane_exits::record_detail(body);
    }
    let count = usize::from(target.registers);
    // A forwarding activation keeps every value passed after its registers, as does one for
    // a skipped forwarding constructor, whose frame passed them on, and one passed more values
    // than it has parameters.
    // A rest body keeps nothing its rest array holds, unless a kept `arguments` or legacy
    // reflection reads them (`TARGET_GATHERS_KEEPING`).
    let gathers = target.entry == CallEntry::framed(Framed::Gathers);
    // A rest view keeps them too, as `activate_gathering_in_lane` keeps them: its reads and
    // its frame, once materialized, take the values passed, never a rest array.
    let keeps = forwarder.is_some()
        || if gathers {
            target.forms & (TARGET_GATHERS_KEEPING | TARGET_REST_VIEW) != 0
        } else {
            target.forwards() || argc > usize::from(target.params)
        };
    let window_size = if keeps { count + argc } else { count };
    // The rest array, made before the window is: the values past the other parameters, read
    // in place from the caller's window or the kept values.
    // A rest view that keeps the values reads them in place (`LaneFrame::rest_view`).
    let rest = (gathers && !(keeps && target.forms & TARGET_REST_VIEW != 0)).then(|| {
        let fixed = usize::from(target.params) - 1;
        let passed: &[Value] = match arguments {
            // SAFETY: the window holds `argc` registers of the caller.
            ActivationArguments::Window(base) => unsafe { std::slice::from_raw_parts(r.0.add(usize::from(base)), argc) },
            // SAFETY: kept values hold `argc` values.
            ActivationArguments::Kept(values, _) => unsafe { std::slice::from_raw_parts(values, argc) },
        };
        vm.rest_parameter_array(passed.get(fixed..).unwrap_or_default())
    });
    // A call that returns its callee's completion completes its caller too, which only an
    // activation's return does in the lane. A strict caller's terminal calls are proper tail
    // calls, which the general dispatch path takes as jumps instead.
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && !returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    if argc > usize::from(u16::MAX)
        || vm.with_scope_entered
    {
        return Err(exit(ip));
    }
    // Capacity, the guest-call budget and a due collection (`LaneStack::activation_limit`):
    // the keeping row's admission (`staged::call_keeping::admit_keeping`).
    let needed = fill_room::<ACTIVATION_FILL_BLOCK>(window_size);
    if !quench_runtime::staged::call_keeping::concrete_admits_keeping(
        vm.lane_stack.activation_count(), vm.lane_stack.activation_limit.get(), vm.lane_stack.values.room(), needed,
    ) {
        return Err(script_call_frame_in_lane(ip, needed));
    }
    let shape = quench_runtime::staged::call_keeping::KeepingWindow { registers: target.registers, params: target.params, keeps, gathers };
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window; each slot is written before the length covers
    // it, and no activation's window moves. Parameter `k` lives in register `k` of a
    // plain activation (`Vm::place_plain_parameters`). The passed values lie in the caller's
    // window or where they were kept, below the new window or in a frame, outside it.
    let callee_registers = unsafe {
        let registers = stack.values.top();
        let passed = match arguments {
            ActivationArguments::Window(base) => r.0.add(usize::from(base)).cast_const(),
            ActivationArguments::Kept(values, _) => values,
        };
        // The keeping row: the parameters, the kept values, and the rest register, whose
        // `undefined` in a rest view makes its reads take the kept values.
        quench_runtime::staged::call_keeping::fill_keeping(
            &mut quench_runtime::staged::Concrete::pure(), registers.expose_provenance() as u64,
            passed.expose_provenance() as u64, argc as u16, shape, rest.unwrap_or(Value::UNDEFINED).to_bits(),
        );
        stack.values.set_top(registers.add(window_size + lane_values::CONTEXT));
        Registers(registers)
    };
    // An activation keeping nothing passed its parameters alone, which its window holds
    // (a rest body's rest array among them): its materialization reads them there.
    let record = quench_runtime::staged::call_keeping::KeepingRecord::of(shape, argc);
    let activation = Activation::of_parts(
        target.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.callee,
            argc: record.argc,
            completion: if forwarder.is_some() {
                Completion::Forwarder
            } else {
                Completion::of(constructs, cross, false)
            },
            forwards: record.forwards,
            kept: record.kept,
            environment: false,
            record: std::ptr::null_mut(),
        }),
    );
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { stack.push_activation(activation, callee_registers.0) };
    if let Some(forwarder) = forwarder {
        stack.attach_payload(SidePayload::Forwarder(forwarder));
    }
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    quench_runtime::staged::call_keeping::enter_context(
        &mut quench_runtime::staged::Concrete::pure(), callee_registers.0.expose_provenance() as u64,
        this.to_bits(), target.env.to_bits(), target.capture.expose_provenance() as u64,
    );
    if cross {
        enter_program_in_lane(cx, target.program());
    }
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    let start = if constructs { base } else { unsafe { (*target.view).start() } };
    Ok((callee_registers, start))
}

/// A frameless call without room on the lane stack: grow the stack, then take the
/// general path, which pushes a frame. Out of line so the activation path saves no
/// registers.
#[cold]
#[inline(never)]
pub(super) fn script_call_frame_in_lane(ip: Ip, room: usize) -> Ip {
    let cx = context_in_lane();
    let vm = unsafe { vm(cx) };
    let stack = &vm.lane_stack;
    // The callee's own room, which a function of many registers makes larger than a common
    // window: reserving less would send each of its calls at this depth here again.
    let room = room.max(LANE_SCRIPT_WINDOW);
    let full = stack.full(room);
    vm.materialize_lane_activations(cx);
    vm.lane_stack.grow(full, room);
    exit(ip)
}

/// A Wasm view's handler for `op` where it differs from a JavaScript view's: a reference call
/// enters Wasm functions and a return completes a Wasm activation; a value row (a capture
/// access, `this`) reads the context a Wasm view lacks.
pub(super) fn wasm_lane_handler(instruction: WideInstruction) -> Option<Option<LaneHandler>> {
    let call = LaneHandler { handler: reference_call_handler(), produces: false };
    let ret = LaneHandler { handler: lane_return_wasm, produces: false };
    if matches!(instruction.op().staged(), Some(quench_runtime::bytecode::Decoder::Value)) {
        // A value row reads the receiver and the captures from the context, which a Wasm view lacks.
        return Some(None);
    }
    let op = instruction.op();
    match op {
        _ if quench_runtime::staged::call_family(op) => Some(Some(call)),
        _ if quench_runtime::staged::return_family(op) => Some(Some(ret)),
        _ => None,
    }
}

/// The return of the record of form `LEAF` with `value` as one op, making no call
/// (`call_return::step_return_one_op_of`), from the running plain frame's `window`. `None`,
/// which changes nothing, for any other, which the past-op completion decides.
#[inline(always)]
pub(super) fn lane_return_one_op<const LEAF: u8>(cx: *mut LaneContext, window: Registers, value: Value) -> Option<(Registers, Ip)> {
    quench_runtime::staged::call_return::step_return_one_op_of(&mut LaneScriptReturn::<LEAF> { cx, window }, value)
}

/// The completion `return_from_in_lane` expects of the activation it returns from.
#[inline(always)]
pub(super) const fn returning_completion(crosses: bool, constructs: bool, settles: bool) -> Completion {
    if crosses { Completion::Cross } else if constructs { Completion::Constructs } else if settles { Completion::SettlesPromise } else { Completion::Plain }
}

/// `call_return::step_activation_return_of` of the innermost activation, a script one whose call keeps its
/// result: of the caller's program (`Completion::Plain`), or, where `CROSSES`, of another
/// (`Completion::Cross`), which enters the caller's program first, or, where `CONSTRUCTS`, a
/// `new` of its program (`Completion::Constructs`), or, where `SETTLES`, a settling async
/// call (`Completion::SettlesPromise`), whose promise the value resolves and the call yields.
/// `INTO_BODY` admits a returning call too.
#[inline(always)]
pub(super) fn return_from_in_lane<const CROSSES: bool, const INTO_BODY: bool, const CONSTRUCTS: bool, const SETTLES: bool>(cx: *mut LaneContext, mut value: Value) -> Option<(Registers, Ip)> {
    debug_assert!(unsafe { (*cx).leaf.call }.is_null(), "a leaf's own returns complete it");
    let vm = unsafe { vm(cx) };
    // A resolution that would observe the value returns it from the frame instead.
    let then = if SETTLES { settled_then_in_lane(vm, value)? } else { Value::UNDEFINED };
    // A construction yields its receiver, the activation's `this`, for a non-object result.
    // The construct family's completion row (`staged::call_construct::complete_construct`): the
    // receiver is the returning window's `this` context word, read only for a non-object.
    if CONSTRUCTS {
        let window = unsafe { (*cx).registers }.0 as u64;
        value = Value::from_bits(quench_runtime::staged::call_construct::concrete_complete_construct(window, value.to_bits(), vm.is_object_like(value)));
    }
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // A plain header's own return (`completions::lane_return_header`) ran before this one
    // (`call_return::step_activation_return_of`, `return_to_body_in_lane`): what reaches here is a side record's
    // completion, a returning call's chain or a record.
    // The innermost read where it is, a header with its side record or a record (no spill:
    // one-frame-one-call.md F4).
    let top = stack.top_frame()?;
    let expected = returning_completion(CROSSES, CONSTRUCTS, SETTLES);
    if top.completion() != Some(expected) { return None; }
    // SAFETY: the call record belongs to the caller's live view.
    let destination = unsafe { (*top.call()).a };
    if !INTO_BODY && destination & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        if CROSSES || CONSTRUCTS || SETTLES {
            return None;
        }
        return return_chain_in_lane(cx, value);
    }
    let destination = destination & quench_runtime::bytecode::REGISTER_MASK;
    let (call, caller_registers, caller_base, window) = (top.call(), top.caller_registers(), top.caller_base(), top.window_word());
    let caller_program = top.caller_program();
    // SAFETY: a script activation holds no drop glue and no Wasm storage
    // (`Activation::frame_storage_bytes`); the values hold no drop glue.
    unsafe {
        stack.discard_top_frame();
        stack.values.release_window(window);
    }
    // A return from inside a `with` leaves its scope without an exit.
    stack.drop_with_objects_from(stack.activation_count());
    // The caller's memory view reads its program's records (`MemoryView::script`), which an
    // activation of the caller's own JavaScript program left in place.
    if CROSSES {
        enter_program_in_lane(cx, caller_program);
        unsafe { (*cx).memory = MemoryView::script(cx) };
    }
    debug_assert!(unsafe { (*cx).memory.bytes } == cx.cast::<u8>(), "a JavaScript run carries its context");
    unsafe { (*cx).registers = caller_registers; (*cx).base = caller_base; }
    if SETTLES { value = settle_promise_in_lane(cx, value, then, top.payload().settles()); }
    caller_registers.set(destination, value);
    Some((caller_registers, following(call)))
}

/// `call_return::step_activation_return_of` of a plain activation that returns the completion of its call, and
/// whose callers do in turn (`return f(...)`), down to the plain activation whose call takes
/// `value`: that activation's caller resumes with it, as the run's completion one activation
/// at a time (`complete_plain_chain_in_lane`) makes it. `None`, which changes nothing, for a run
/// another completion ends; the general completions take it.
#[inline(always)]
pub(super) fn return_chain_in_lane(cx: *mut LaneContext, value: Value) -> Option<(Registers, Ip)> {
    let vm = unsafe { vm(cx) };
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // The run walked where its frames are, headers with their side records, then records: no
    // spill and no record built (one-frame-one-call.md F4). `above` counts the frames inside
    // the one whose call takes the value.
    let (outer, destination, above) = {
        let mut frames = stack.frames_innermost();
        // The innermost returns its call's completion, which the caller of `return_from_in_lane` proved.
        let innermost = frames.next()?;
        debug_assert!(innermost.completion() == Some(Completion::Plain));
        let mut above = 1;
        loop {
            let frame = frames.next()?;
            if frame.completion() != Some(Completion::Plain) {
                return None;
            }
            // SAFETY: the call record belongs to the caller's live view.
            let destination = unsafe { (*frame.call()).a };
            if destination & quench_runtime::bytecode::RETURN_REGISTER == 0 {
                break (frame, destination, above);
            }
            above += 1;
        }
    };
    let (call, caller_registers, caller_base, window) = (outer.call(), outer.caller_registers(), outer.caller_base(), outer.window_word());
    let index = stack.activation_count() - 1 - above;
    stack.drop_owned_from(index + 1);
    // SAFETY: script activations hold no drop glue or Wasm storage; the values hold no drop
    // glue. The frames from the outer one in go where they are.
    unsafe {
        for _ in 0..=above { stack.discard_top_frame(); }
        stack.values.release_window(window);
        (*cx).registers = caller_registers; (*cx).base = caller_base;
    }
    caller_registers.set(destination & quench_runtime::bytecode::REGISTER_MASK, value);
    Some((caller_registers, following(call)))
}

/// Complete the innermost lane activation with `value`, which the record at `ip` of that
/// activation returns, as a return from it does: its caller resumes after the call with the
/// value in the call's result register, or, for a call that returns its callee's
/// completion, completes in turn. `None` when no activation runs: the running function is a
/// VM frame.
#[inline(always)]
pub(super) fn complete_activation_in_lane(cx: *mut LaneContext, ip: Ip, mut value: Value) -> Option<Resumed> {
    if let Some(resumed) = complete_plain_chain_in_lane(cx, value) {
        return Some(resumed);
    }
    let vm = unsafe { vm(cx) };
    // The innermost's completion read where it is, a header with its side record or a record:
    // no spill and no record built (one-frame-one-call.md F4).
    let top = vm.lane_stack.top_frame()?;
    let completion = top.completion();
    if completion == Some(Completion::ToWasm) {
        return Some(lane_complete_to_wasm(cx, ip, value));
    }
    if proxy_traps::trap_result_needs_general_path(vm, top.payload(), value) {
        return Some(Resumed::Exit(frame_return(vm, ip, value)));
    }
    // A settling activation whose promise's resolution would observe the value returns it
    // from its frame, whose pending call resolves the promise (`settled_then_in_lane`); else the
    // `then` read here is the one its resolution uses.
    let mut then = Value::UNDEFINED;
    if completion == Some(Completion::SettlesPromise) {
        let Some(read) = settled_then_in_lane(vm, value) else { return Some(Resumed::Exit(frame_return(vm, ip, value))) };
        then = read;
    }
    // Each frame popped where it is (`ActivationView`): a header's pop builds no record.
    let mut frame = vm.lane_stack.pop_activation()?;
    loop {
        let (caller_memory, this) = restore_caller_in_lane(cx, &frame);
        if let Some(completion) = frame.completion() {
            // A native's callback ends its run, whatever the native's own call record is.
            if let Some(ended) = body_calls::ends_run(cx, &frame, completion, this, value) {
                return Some(ended);
            }
            if completion == Completion::ArrayStep {
                return Some(step_array_iteration_in_lane(cx, &frame, value));
            }
            if completion.is_generator() {
                return Some(generator_returned_in_lane(cx, &frame, caller_memory, value));
            }
            if completion == Completion::AsyncRequest {
                return Some(async_requests::request_returned_in_lane(cx, &frame, caller_memory, value));
            }
            // [[Construct]] of an ordinary constructor yields its returned object,
            // otherwise the receiver it allocated. A skipped forwarding constructor returns
            // `undefined` after the method, so its construction yields the receiver.
            // A trap of another program (`AccessorCall::trap_target`) completes as a
            // trap, not as `Completion::Cross`, and leaves its program the same way.
            if completion.crosses() || frame.caller_program() != unsafe { (*cx).program } {
                enter_program_in_lane(cx, frame.caller_program());
            }
            value = setters::completed_value(vm, &frame, completion, this, value);
            if completion == Completion::Forwarder {
                value = Value::from_bits(quench_runtime::staged::call_construct::concrete_forwarder_value(value.to_bits(), this.to_bits()));
            }
            if completion == Completion::SettlesPromise {
                value = settle_promise_in_lane(cx, value, then, frame.payload().settles());
            }
        } else if let Some(result) = frame.export_result() {
            // The export's caller runs its own program again, as the run's JavaScript.
            enter_program_in_lane(cx, frame.caller_program());
            unsafe {
                (*cx).inline_calls = false;
                set_script_calls_in_lane(cx, true);
                (*cx).instance = Value::DELETED;
            }
            value = vm.wasm_export_result(value, result);
        }
        // SAFETY: the call record belongs to the caller's live view.
        if unsafe { (*frame.call()).a } & quench_runtime::bytecode::RETURN_REGISTER == 0 {
            return Some(resume_after_call_in_lane(&frame, caller_memory, value));
        }
        // The call returns its callee's completion: its caller completes in turn, an
        // activation here or the frame in the general loop, which the exit hands it to.
        if caller_returns_in_frame(vm, value, &mut then) {
            return Some(Resumed::Exit(frame_return(vm, frame.call(), value)));
        }
        frame = vm.lane_stack.pop_activation().expect("the caller activation is the innermost");
    }
}

/// The return of a JavaScript import's callee (`Completion::ToWasm`) (`step_import_return_of`).
#[inline(never)]
pub(super) fn lane_complete_to_wasm(cx: *mut LaneContext, ip: Ip, value: Value) -> Resumed {
    quench_runtime::staged::wasm_activation::step_import_return_of(&mut LaneReturn { cx, ip }, value)
}

/// The return of a Wasm activation whose call keeps its result (`step_wasm_return_of`), else `None`.
#[inline(always)]
pub(super) fn lane_complete_wasm_activation(cx: *mut LaneContext, value: Value) -> Option<Resumed> {
    quench_runtime::staged::wasm_activation::step_wasm_return_of(&mut LaneReturn { cx, ip: std::ptr::null() }, value)
}

/// The cached read of a single-base returning `GetField` of `KIND` (`RETURNING_FIELD_OWN`,
/// `_POLYMORPHIC`, `_HOLDER`, `_INHERITED`) at `cache`: a hit, which runs no guest code.
#[inline(always)]
pub(super) fn returning_field_hit<const KIND: u8>(
    vm: &mut Vm,
    code: &ResidualProgram,
    object: Value,
    cache: u16,
) -> Option<Value> {
    match KIND {
        RETURNING_FIELD_OWN => vm.primary_own_field_hit(code, object, cache),
        RETURNING_FIELD_POLYMORPHIC => vm.inline_polymorphic_field_hit(code, object, cache),
        RETURNING_FIELD_HOLDER => vm.primary_holder_field_hit(code, object, cache),
        _ => vm.inherited_field_hit(code, object, cache),
    }
}

/// The value a frameless read target (`CallEntry::reads`, `CallEntry::frameless(Frameless::ForwardsRead)`)
/// returns for a call passing `arguments.get(k)` as parameter `k` (`None` where not passed) on
/// `this`, where the reads its records have quickened to hit. Answering it at the call
/// leaves nothing for an activation to observe: each hit runs no guest code and allocates
/// nothing. `None` for any other record state or a miss, which the call activates to run.
///
/// # Safety
/// `cx` is the running lane's context and `target` a target of its program.
#[inline(always)]
pub(super) unsafe fn call_read_in_lane(
    cx: *mut LaneContext,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    // SAFETY: the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    // A forwarding body's answer is its method's leaf read on the field it reads, so both
    // kinds reach one leaf read, which the call handler then holds once.
    let (target, arguments, this) = if target.entry == CallEntry::frameless(Frameless::ForwardsRead) {
        // SAFETY: as for `ForwardRead::of`.
        let (callee, receiver) = unsafe { ForwardRead::of(target) }?.method(vm, code, arguments, this)?;
        // SAFETY: as `Vm::script_call_entry`; a read resolves no target and collects nothing.
        let inner = unsafe { &*vm.script_call_entry(callee)? };
        if !inner.entry.reads() {
            return None;
        }
        let inner_this = direct_call_this_in_lane(vm, inner, receiver)?;
        // The call passes the parameters in order, so its argument `k` is parameter `k`.
        (inner, arguments.within(target.params), inner_this)
    } else {
        (target, arguments, this)
    };
    match target.entry {
        // The receiver the call binds (`direct_call_this_in_lane`), which is what it returns.
        _ if target.entry == CallEntry::frameless(Frameless::ReadsThis) => Some(this),
        // SAFETY: a capture-read target of the running program.
        _ if target.entry == CallEntry::frameless(Frameless::ReadsCapture) => unsafe { read_capture_in_lane(vm, target) },
        // SAFETY: as for `LeafRead::of`.
        _ => unsafe { LeafRead::of(target) }?.run(vm, code, arguments, this),
    }
}

/// The capture a `CallEntry::frameless(Frameless::ReadsCapture)` target's closure environment holds, read as its
/// frameless activation's `LoadCapture` would (`load_capture_walk_miss`); an uninitialized
/// binding misses.
///
/// # Safety
/// `target` is a `CallEntry::frameless(Frameless::ReadsCapture)` target of the running program, whose view holds
/// its `LoadCapture` record first.
#[inline(never)]
pub(super) unsafe fn read_capture_in_lane(vm: &mut Vm, target: &ScriptCallTarget) -> Option<Value> {
    // SAFETY: the program store keeps a derived view alive with its program.
    let imm = unsafe { &*(*target.view).records() }.imm();
    let (depth, slot) = (ImmediateLayout::capture_depth(imm), ImmediateLayout::capture_slot(imm));
    let slots = capture_reads(target.capture);
    if depth == 0 && !slots.is_null() {
        // SAFETY: as `ScriptCallTarget::capture`.
        return unsafe { quench_runtime::tier_surface::EnvironmentSlots::read_storage(slots, slot) };
    }
    let (slots, root) = vm.walked_capture_slots(target.env, false, depth)?;
    // SAFETY: as `Vm::walked_capture_slots`; no collection runs in the lane.
    vm.owned_capture(unsafe { &*slots }, root, slot)
}

/// The values a call that a read answers passed (`call_read_in_lane`): parameter `k` is the
/// call window's register `base + k` while `k < passed`, else unpassed. A concrete value,
/// not a closure, so the reads it feeds are one copy whatever forwarded them.
/// `call_read_in_lane` of the call at `site`, memoized when it answers (`note_read_in_lane`). Out
/// of line: the read's calls take stack temporaries, which a handler inlining them would hold
/// on its common path.
///
/// # Safety
/// As `call_read_in_lane`.
#[inline(never)]
pub(super) unsafe fn answered_call_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    // SAFETY: the caller's contract.
    let value = unsafe { read_answer_in_lane(cx, target, arguments, this) };
    // A mutating operation must never enter the pure read memo.
    if value.is_some() && target.entry != CallEntry::frameless(Frameless::AppendsCapture) {
        note_read_in_lane(cx, site, target, arguments, this);
    }
    value
}

/// The value `target`'s read answers a call with, or what a capture append returns, without the
/// site's memo: what a call whose record has no memo form (a method call's) is answered by.
///
/// # Safety
/// As `call_read_in_lane`.
#[inline(always)]
pub(super) unsafe fn read_answer_in_lane(
    cx: *mut LaneContext,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    if target.entry == CallEntry::frameless(Frameless::AppendsCapture) {
        return unsafe { capture_append::run(cx, target, arguments) };
    }
    unsafe { call_read_in_lane(cx, target, arguments, this) }
}

/// Every bound own-field read (`own_formed_handler`), of `this` and of a register.
pub(super) fn own_bound_handlers() -> [*const (); OWN_FORMS.len() + OWN_FORMS.len()] {
    if cfg!(not(feature = "jit")) { return [std::ptr::null(); OWN_FORMS.len() + OWN_FORMS.len()]; }
    macro_rules! row { ($site:expr) => { generated_row!(FIELD_LEAF_READ, ACC_NONE, $site) as *const () } }
    own_sites!(row)
}

/// A memo leaf's first step (`MemoLeaf::run`): `field` of the object `base` names, read
/// from `this`'s record where the caller holds it (a forwarding body's receiver, already
/// proven a direct object).
#[inline(always)]
pub(super) fn memo_first_step(
    vm: &Vm,
    field: quench_runtime::vm::FieldCache,
    base: u16,
    arguments: ReadArguments,
    this: Value,
    this_record: Option<&quench_runtime::tier_surface::Object>,
) -> Option<Value> {
    match this_record {
        Some(record) if base == quench_runtime::bytecode::FieldBase::THIS.0 => vm.record_field_formed_hit_in(field, record),
        _ => vm.record_field_formed_hit(field, read_base_in_lane(quench_runtime::bytecode::FieldBase(base), arguments, this)?),
    }
}

/// The `script_call_memo_miss` instantiation of memo shape `shape` (`MemoRead::shape`).
pub(super) fn script_call_memo_handler(shape: u8) -> Handler {
    const _: () = assert!(MEMO_NESTED < MEMO_FORWARD);
    macro_rules! shapes {
        ($($shape:expr),*) => {
            match shape {
                $(shape if shape == $shape => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::FRAME_CALL | ($shape as u16) }>(),)*
                _ => unreachable!("a memo shape is a leaf step, forwarded or not"),
            }
        };
    }
    shapes!(
        MEMO_FIELD,
        MEMO_ELEMENT,
        MEMO_LENGTH,
        MEMO_NESTED,
        MEMO_FORWARD | MEMO_FIELD,
        MEMO_FORWARD | MEMO_ELEMENT,
        MEMO_FORWARD | MEMO_LENGTH,
        MEMO_FORWARD | MEMO_NESTED
    )
}

/// The memo the call record `site` made, in the slot it names (`CallOperand::memo`); `None`
/// before its first, or once the table refilled and gave the slot to another site.
#[inline(always)]
pub(super) fn site_read_memo<'a>(memos: &'a [ReadMemo], site: &LaneInstruction) -> Option<&'a ReadMemo> {
    // SAFETY: a memo's site is a call record (`Op::makes_script_call`), whose operand is `call`.
    let slot = unsafe { site.operand.call.memo };
    memos.get(slot as usize).filter(|held| std::ptr::eq(held.site, site))
}

/// Records `target`'s read at the call record `site` (`memoize_read_in_lane`) unless the site's
/// own memo of this generation missed, which keeps the full form until the generation ends.
#[inline(always)]
pub(super) fn note_read_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    let stack = unsafe { &vm(cx).lane_stack };
    // SAFETY: `site` is a call record of the running view.
    if site_read_memo(&stack.read_memos, unsafe { &*site }).is_none_or(|held| held.admission.generation() != stack.generation) {
        memoize_read_in_lane(cx, site, target, arguments, this);
    }
}

/// The memo of the read `target` just answered at `site` for `arguments` and `this`, and the
/// site's memo form (`script_call_memo_miss`); nothing for an arrow's own `this`, which the
/// memo does not hold, or a read whose records no longer hit.
#[cold]
#[inline(never)]
pub(super) fn memoize_read_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    // SAFETY: the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    // The memo answers the site from now on, so no call of it links its record: its record
    // names the callee for the bodies that inline it (`site_targets_in_lane`).
    // SAFETY: `site` is a record of the running view.
    vm.note_site_callee(unsafe { &(*site).call_link }, crate::compiled::CallTarget {
        callee: crate::compiled::CalleeBinding::Identity(target.callee), function: target.function });
    if target.this_binding == ThisBinding::Lexical {
        return;
    }
    let read = if target.entry == CallEntry::frameless(Frameless::ForwardsRead) {
        // SAFETY: as for `ForwardRead::of`.
        let Some(forward) = (unsafe { ForwardRead::of(target) }) else {
            return;
        };
        let Some((method, _)) = forward.method(vm, code, arguments, this) else {
            return;
        };
        // SAFETY: as `Vm::script_call_entry`; nothing here resolves a target or collects.
        let Some(inner) = vm.script_call_entry(method).map(|inner| unsafe { &*inner }) else {
            return;
        };
        if !inner.entry.reads() || inner.this_binding == ThisBinding::Lexical {
            return;
        }
        // SAFETY: as for `LeafRead::of`.
        let Some(read) = (unsafe { LeafRead::of(inner) }) else {
            return;
        };
        let entry = |hit: FieldHit| match hit {
            FieldHit::Own(cache) | FieldHit::Holder(cache) => vm.field_cache_entry(cache),
        };
        let Some(leaf) = MemoLeaf::of(vm, code, read) else {
            return;
        };
        // The steps as whole-word compares of the objects this call read (`MemoLeaf::formed`).
        let (field, method_field) = (entry(forward.field), entry(forward.method));
        let base = read_base_in_lane(quench_runtime::bytecode::FieldBase(forward.base), arguments, this);
        let receiver = base.and_then(|base| vm.record_field_entry_hit(field, base));
        let read = match receiver.and_then(|receiver| receiver_this_in_lane(vm, inner.this_binding, receiver)) {
            Some(method_this) => leaf.formed(vm, arguments.within(target.params), method_this),
            None => leaf,
        };
        MemoRead::Forward {
            base: forward.base,
            field: base.map_or(field, |base| vm.formed_entry(field, base)),
            method_field: receiver.map_or(method_field, |receiver| vm.formed_entry(method_field, receiver)),
            params: target.params,
            method,
            read,
        }
    } else {
        // SAFETY: as for `LeafRead::of`.
        let Some(read) = (unsafe { LeafRead::of(target) }) else {
            return;
        };
        match MemoLeaf::of(vm, code, read) {
            Some(read) => MemoRead::Leaf(read.formed(vm, arguments, this)),
            None => return,
        }
    };
    let handler = script_call_memo_handler(read.shape());
    let memo = ReadMemo {
        site,
        callee: target.callee,
        admission: vm.lane_stack.current_admission(),
        this_binding: target.this_binding,
        read,
    };
    let memos = &mut vm.lane_stack.read_memos;
    // SAFETY: `site` is a call record (`Op::makes_script_call`) of the running view, whose operand is
    // `call`, written as `requicken` writes it.
    unsafe {
        if site_read_memo(memos, &*site).is_some() {
            memos[(*site).operand.call.memo as usize] = memo;
        } else {
            if memos.len() == READ_MEMOS {
                memos.clear();
            }
            (*site.cast_mut()).operand.call.memo = memos.len() as u32;
            memos.push(memo);
        }
    }
    requicken(site, handler);
}

/// The value the memo of the call record `i` answers its call with (`ReadMemo`); `None`
/// where the memo is another site's or stale, the call is another callee's or a construction's
/// (`constructing_in_lane`), or a hit misses.
///
/// # Safety
/// `cx` is the running lane's context and `i` its call record.
#[inline(always)]
pub(super) unsafe fn memo_read_in_lane<const SHAPE: u8>(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let vm: &Vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    let held = site_read_memo(&vm.lane_stack.read_memos, i)?;
    if held.callee != callee
        // A pending construction refuses every memo (`LaneStack::set_refused`).
        || held.admission != vm.lane_stack.admission
    {
        return None;
    }
    // A memo's steps read `this` only as a field base, which hits on a direct object alone: the
    // receiver binds itself then (`receiver_this_in_lane`), and any other receiver, whatever it
    // binds, misses the step as its binding would, so the receiver stands for `this` untested.
    if held.this_binding == ThisBinding::Lexical {
        return None;
    }
    let this = r.get(i.c);
    let arguments = ReadArguments {
        registers: r,
        base: ImmediateLayout::call_window_base(i.imm()),
        passed: ImmediateLayout::argument_count(i.imm()),
    };
    let leaf = SHAPE & !MEMO_FORWARD;
    match &held.read {
        MemoRead::Leaf(read) if SHAPE & MEMO_FORWARD == 0 => read.run(leaf, vm, arguments, this, None),
        MemoRead::Forward { base, field, method_field, params, method, read }
            if SHAPE & MEMO_FORWARD != 0 =>
        {
            let base = read_base_in_lane(quench_runtime::bytecode::FieldBase(*base), arguments, this)?;
            let receiver = vm.record_field_formed_hit(*field, base)?;
            // R4-B2b: needs heap (HeapReads has no record_id; the engine cannot reach &Heap)
            let record = vm.heap_reads().object_record_by_id(vm.heap_reads().record_id(receiver.direct_object_index()?));
            if vm.record_field_formed_hit_in(*method_field, record)? != *method {
                return None;
            }
            // A memo's method binds no lexical `this` (`memoize_read_in_lane`), so the receiver, a
            // direct object, is its `this` (`receiver_this_in_lane`), and a step on `this` reads
            // its record.
            // The call passes the parameters in order, so its argument `k` is parameter `k`.
            read.run(leaf, vm, arguments.within(*params), receiver, Some(record))
        }
        _ => None,
    }
}

/// The object a read body's field record names: `this`, or the parameter its activation
/// would hold in that register; `None` for an unpassed one, whose `undefined` no read hits.
#[inline(always)]
pub(super) fn read_base_in_lane(
    base: quench_runtime::bytecode::FieldBase,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    match base.register_index() {
        Some(register) => arguments.get(register),
        // A local base (`FieldBase::local`) names no activation register; the call runs.
        None if base.local_index().is_some() => None,
        None => Some(this),
    }
}

/// The `returning_miss` kind of a value record that returns its result from the frame.
pub(super) fn returning_kind(instruction: WideInstruction) -> Option<u8> {
    if !instruction.returns_from_frame() {
        return None;
    }
    let traits = instruction.op().traits();
    if traits.has(Traits::RETURNING_FIELD) { Some(RETURNING_FIELD) }
    else if traits.has(Traits::RETURNING_UNARY) { Some(RETURNING_UNARY) }
    else if traits.has(Traits::RETURNING_BINARY) { Some(RETURNING_BINARY) }
    else if traits.has(Traits::RETURNING_OBJECT) { Some(RETURNING_OBJECT) }
    else if traits.has(Traits::RETURNING_OBJECT_LITERAL) { Some(RETURNING_OBJECT_LITERAL) }
    else if traits.has(Traits::RETURNING_LITERAL) { Some(RETURNING_LITERAL) }
    else { None }
}

/// The value of a `Unary` (`unary`) or `Binary` record whose operands the lane's own values
/// decline, made by the general operation in place (`unwound_in_place`): it may reach guest
/// code (an object's `ToPrimitive` hook) or throw, which is raised at the record. `Err` is the
/// record the run goes on at: the handler a throw entered, or the general path, also for an
/// operand the lane cannot read. Out of line, so the returning handlers keep their code.
#[cold]
#[inline(never)]
pub(super) fn operation_in_place(cx: *mut LaneContext, ip: Ip, r: Registers, i: &LaneInstruction, unary: bool) -> Result<Value, Ip> {
    let operator = i.imm();
    if unary {
        let operand = r.get(i.b);
        return unwound_in_place(cx, ip, |vm, code| vm.unary(code, operator, operand));
    }
    let (Some(left), Some(right)) = (operand_value_in_lane(cx, r, i.b), operand_value_in_lane(cx, r, i.c)) else {
        return Err(exit(ip));
    };
    unwound_in_place(cx, ip, |vm, code| vm.binary(code, operator, left, right))
}

/// The protocol miss body of a returning constant-right `Binary` of operator `operator` (a
/// `BinaryOperator`'s value) at leaf form `LEAF` (`protocol_misses::returns`): `returning_operator_miss`
/// of that operator; `None` for a value no arithmetic operator has.
pub(super) const fn returning_operator_miss_of<const LEAF: u8>(operator: u8) -> Option<Handler> {
    macro_rules! pick {
        ($op:expr) => {
            returning_operator_miss::<{ $op }, true, LEAF> as Handler
        };
    }
    arithmetic_operators!(operator as u32, pick)
}

/// The kind a returning `GetField` takes on its first run (`row_returning_field_first`),
/// which its record and site alone decide.
pub(super) fn returning_field_first_kind<const LEAF: u8>(
    vm: &Vm,
    code: &ResidualProgram,
    i: &LaneInstruction,
) -> Handler {
    if i.b != quench_runtime::bytecode::FieldBase::NESTED && vm.is_private_name(i.imm()) {
        return field_return_row::<LEAF>(RETURNING_FIELD_PRIVATE);
    }
    if is_single_field_base(i.b) && !vm.reads_function_info(i.imm()) {
        by_leaf!(LEAF, |L| single_base_handler(i.b, generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(true, L)),
            generated_row!(FIELD_LEAF_READ, ACC_NONE, returning_site(false, L)),
            returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD, L) as Handler))
    } else if field_site_tier_in_lane(vm, code, i) == FieldSiteTier::Nested {
        field_return_row::<LEAF>(RETURNING_FIELD_NESTED)
    } else {
        field_return_row::<LEAF>(RETURNING_FIELD)
    }
}

/// The protocol row of a returning `GetField` of `kind` (a `RETURNING_FIELD*` kind) at leaf form
/// `LEAF`: its RETURN code (`returning_code`), which `returning_miss` completes.
pub(super) fn field_return_row<const LEAF: u8>(kind: u8) -> Handler {
    by_leaf!(LEAF, |L| match kind {
        RETURNING_FIELD => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD, L) as Handler,
        RETURNING_FIELD_OWN => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_OWN, L) as Handler,
        RETURNING_FIELD_POLYMORPHIC => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_POLYMORPHIC, L) as Handler,
        RETURNING_FIELD_HOLDER => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_HOLDER, L) as Handler,
        RETURNING_FIELD_INHERITED => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_INHERITED, L) as Handler,
        RETURNING_FIELD_NESTED => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_NESTED, L) as Handler,
        RETURNING_FIELD_PRIVATE => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_PRIVATE, L) as Handler,
        RETURNING_FIELD_GETTER => returning_row_of!(quench_runtime::bytecode::FIELD_READ, RETURNING_FIELD_GETTER, L) as Handler,
        _ => unreachable!("a returning GetField's kind is a field kind"),
    })
}

/// `frame_call_in_lane` of an async function the general loop would push as a settling call
/// (`Vm::in_loop_settling_target`): the callee starts as a job (`Vm::start_async_call`) beside
/// the caller's activations, as a native that runs beside them does (`call_native_out_in_lane`),
/// so its first `await` suspends its window in place and the caller continues in the lane with
/// the promise. `None` leaves the call to the general path: a call that completes its caller,
/// or a callee of another program or not yet derived.
#[inline(never)]
pub(super) fn start_async_call_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction, callee: Value) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    if !unsafe { (*cx).script_calls } || i.a & quench_runtime::bytecode::RETURN_REGISTER != 0 {
        return None;
    }
    let (program, function, env) = vm.in_loop_settling_target(code, callee)?;
    if program != unsafe { (*cx).program } || !vm.call_may_derive_in_lane(program, function) {
        return None;
    }
    let window = usize::from(ImmediateLayout::call_window_base(i.imm()));
    let argc = usize::from(ImmediateLayout::argument_count(i.imm()));
    // SAFETY: the call's window is registers of the running function; the values are copied
    // before any activation becomes a frame.
    let arguments = quench_runtime::vm::call_arguments::CallArguments::from_slice(unsafe { std::slice::from_raw_parts(r.0.add(window), argc) });
    let this = r.get(i.c);
    // SAFETY: `ip` is a record of the active view, which is the running function's.
    let call_pc = unsafe { record_pc(cx, ip) } as usize;
    let beside = park_async_caller_in_lane(vm, cx, call_pc);
    let context = CallContext::user_function(function, callee);
    let result = vm.start_async_call(code, function, env, this, arguments.as_slice(), context);
    note_allocation(vm);
    vm.lane_stack.refresh_activation_limit();
    Some(async_call_resumed_in_lane(vm, cx, ip, i, call_pc, beside, result))
}

/// Where the lane goes once `start_async_call_in_lane`'s job has run its first step: on with the
/// promise in the call's register, or out with the error, materializing the deferred caller
/// the job left unobserved so the error has a frame to unwind.
pub(super) fn async_call_resumed_in_lane(vm: &mut Vm, cx: *mut LaneContext, ip: Ip, i: &LaneInstruction, call_pc: usize, beside: bool, result: Result<Value, JsError>) -> Resumed {
    let unobserved = beside && vm.lane_deferred.take().is_some();
    match result {
        Ok(promise) => {
            // The activations may have become frames, whose registers the context now names.
            let r = unsafe { (*cx).registers };
            r.set(i.a, promise);
            Resumed::At(r, following(ip), Carried::script(cx))
        }
        Err(error) => {
            if unobserved {
                vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
                vm.materialize_deferred_lane();
            }
            vm.lane_completion.raise(error);
            Resumed::Exit(exit(ip))
        }
    }
}

/// Where `start_async_call_in_lane`'s caller resumes once the callee's job settles its first
/// step: beside other activations, a deferred lane call the job materializes only if it
/// observes the caller (true); otherwise the caller's own frame, resuming past the call.
pub(super) fn park_async_caller_in_lane(vm: &mut Vm, cx: *mut LaneContext, call_pc: usize) -> bool {
    let beside = vm.lane_stack.has_frames();
    if beside {
        vm.lane_deferred = Some(DeferredLaneCall { cx: cx.cast(), call_pc, frames: vm.frames.len() });
    } else {
        // SAFETY: the context outlives the lane run.
        let frame = unsafe { (*cx).frame() };
        vm.frames[frame].pc = call_pc + 1;
        vm.frames[frame].pc_is_resume = true;
    }
    beside
}

/// The facts that keep function `function` of `program` out of a frameless activation, for
/// the census of frame calls and constructions.
#[cfg(feature = "profile-lane-exits")]
pub(super) fn frame_entry_census(vm: &Vm, program: ProgramId, function: u32) -> Option<String> {
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
pub(super) fn push_frame_call_in_lane(
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
pub(super) fn lane_wasm_export_frame(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Resumed> {
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
pub(super) fn frame_construct_in_lane(cx: *mut LaneContext, r: Registers, ip: Ip, i: &LaneInstruction) -> Ip {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let callee = r.get(i.b);
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
    let target = match vm.inline_construct_target(code, frame, callee) {
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
pub(super) fn enter_script_frame_in_lane(cx: *mut LaneContext, frame: usize) -> Resumed {
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
pub(super) fn return_script_frame_in_lane(
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
pub(super) fn return_frame_in_lane(r: Registers, ip: Ip) -> Resumed {
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
pub(super) fn lane_return_wasm_export(cx: *mut LaneContext, value: Value) -> Resumed {
    // The record is unused: a frame's return reads only its pending call.
    quench_runtime::staged::wasm_activation::step_export_frame_return_of(&mut LaneExportCall { cx, r: Registers(std::ptr::null_mut()), ip: std::ptr::null(), i: std::ptr::null(), call: None }, value)
}

/// The handler serving opcode `OP` whose accumulator field is `ACC`, derived
/// from the opcode's semantic family; `None` when the general path executes
/// the opcode.
pub(super) const fn handler_for<const OP: u16, const BACKWARD: bool, const ACC: u8>()
-> Option<LaneHandler> {
    let Some(op) = Op::from_index(OP as usize) else {
        return None;
    };
    // Past the first 256 opcodes only generated rows run in the lane; the rest leave it, as
    // before the table covered them, and instantiate nothing.
    if OP >= 256 && !admits_row(op) && !jumps_by_row(op) {
        return None;
    }
    if jumps_by_row(op) {
        return Some(LaneHandler { handler: row_jump!(OP, BACKWARD, FLAG_UNREAD, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }), produces: false });
    }
    if admits_row(op) {
        return Some(LaneHandler { handler: generated_row!(OP, ACC, FLAG_UNREAD), produces: row_produces(op) });
    }
    let handler: Handler = match other_handler(op) {
        Some(handler) => handler,
        None => return None,
    };
    Some(LaneHandler {
        handler,
        produces: false,
    })
}

/// The handlers of a class's private elements, none of which runs guest code: a field's
/// definition, a method's brand, and the brand checks of a read and of `#x in`.
pub(super) const fn private_element_handler(opcode: Op) -> Option<Handler> {
    match opcode {
        _ if quench_runtime::staged::private_field_definition_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_FIELD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_PRIVATE_FIELD as u16 }) as Handler),
        _ if quench_runtime::staged::private_name_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_NAME_MARK as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::MARK_PRIVATE_NAME as u16 }) as Handler),
        _ if quench_runtime::staged::private_check_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_CHECK as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::CHECK_PRIVATE as u16 }) as Handler),
        _ if quench_runtime::staged::private_in_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PRIVATE_IN as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::PRIVATE_IN as u16 }) as Handler),
        _ => None,
    }
}

/// Handlers that neither produce nor read the accumulator, apart from the
/// branch tests, which read it.
pub(super) const fn other_handler(opcode: Op) -> Option<Handler> {
    match opcode {
        // The lane's synthetic prologue records: their staged rows, which the lane's activations start at.
        quench_runtime::bytecode::TIER_PROLOGUE => Some(tier_prologue_row()),
        quench_runtime::bytecode::BODY_ENTRY => Some(body_entry_row()),
        // A raising record unwinds in the lane where a handler takes it (`handler_edges::throw_record`).
        _ if opcode.raises() => Some(handler_edges::throw_record),
        // A derived constructor's `this` binding in its own frame (`derived_frames`).
        _ if opcode.binds_this() => Some(derived_frames::bind_this_in_frame),
        _ if quench_runtime::staged::field_read_family(opcode) => Some(row_get_field_first),
        // merge: theirs' `lane_set_object_field_own` / `lane_set_this_field_own` are undefined; HEAD's `field_store_row` serves both receivers.
        _ if opcode.field_store().is_some() => match opcode.field_store() { Some(store) => Some(field_store_row(store)), None => None },
        _ if quench_runtime::staged::property_record_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::PROPERTY_RECORD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_PROPERTY_RECORD as u16 }) as Handler),
        _ if quench_runtime::staged::field_definition_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::FIELD_DEFINE as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::DEFINE_FIELD as u16 }) as Handler),
        _ if private_element_handler(opcode).is_some() => private_element_handler(opcode),
        _ if quench_runtime::staged::copy_properties_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::COPY_DATA_PROPERTIES as u16 }, ACC_NONE, { protocol_family::PRIVATE | protocol_misses::private::COPY_DATA_PROPERTIES as u16 }) as Handler),
        _ if opcode.steps_in_lane() => Some(general_step_in_lane),
        _ if quench_runtime::staged::element_read_family(opcode) => Some(protocol_row!(ELEMENT_READ, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::GET_INDEX as u16 }) as Handler),
        _ if quench_runtime::staged::element_write_family(opcode) => Some(protocol_row!(ELEMENT_WRITE, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::SET_INDEX as u16 }) as Handler),
        _ if quench_runtime::staged::known_call_family(opcode) => Some(known_call_handler()),
        _ if quench_runtime::staged::call_family(opcode) => Some(script_call_row(ANY_ARGC, false, false)),
        _ if quench_runtime::staged::call_array_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::CALL_ARRAY as u16 }, ACC_NONE, { protocol_family::NATIVE_CALL | protocol_misses::native_call::CALL_ARRAY as u16 }) as Handler),
        _ if quench_runtime::staged::call_spread_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::CALL_SPREAD as u16 }, ACC_NONE, { protocol_family::NATIVE_CALL | protocol_misses::native_call::CALL_SPREAD as u16 }) as Handler),
        _ if quench_runtime::staged::delete_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::DELETE_ROW as u16 }, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::DELETE as u16 }) as Handler),
        _ if quench_runtime::staged::construction_family(opcode) => Some(protocol_handler::<{ quench_runtime::bytecode::CONSTRUCT_ROW as u16 }, ACC_NONE, { protocol_family::CONSTRUCT | protocol_misses::construct::SCRIPT_CONSTRUCT as u16 }>()),
        _ if quench_runtime::staged::forwarded_call_family(opcode) => Some(protocol_row!({ quench_runtime::bytecode::FORWARDED_CALL as u16 }, ACC_NONE, { protocol_family::ENTRY | protocol_misses::entry::FORWARD_CALL as u16 })),
        _ if quench_runtime::staged::return_family(opcode) => Some(return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(RETURN_KIND, NOT_LEAF))),
        _ if generator_yields::suspend_handler(opcode).is_some() => generator_yields::suspend_handler(opcode),
        _ => None,
    }
}

pub(super) const fn flatten_rows(
    rows: [[Option<LaneHandler>; TABLE_ROW as usize]; TABLE_ROWS],
) -> [Option<LaneHandler>; TABLE_SLOTS] {
    let mut table = [None; TABLE_SLOTS];
    let mut slot = 0;
    while slot < TABLE_SLOTS {
        table[slot] = rows[slot / TABLE_ROW as usize][slot % TABLE_ROW as usize];
        slot += 1;
    }
    table
}

/// Whether the lane may run `instruction` from its view record: every
/// register it names lies inside the register file and carries no flag.
pub(crate) fn fields_fit_in_lane(instruction: WideInstruction, function: &Function) -> bool {
    let registers = function.registers;
    let op = instruction.op();
    let fields = [
        (InstructionField::A, instruction.a()),
        (InstructionField::B, instruction.b()),
        (InstructionField::C, instruction.c()),
    ];
    let numeric_local = instruction.writes_numeric_local()
        && instruction.a() & quench_runtime::bytecode::RETURN_REGISTER == 0
        && instruction.result_register() < function.locals;
    let registers_fit = fields
        .iter()
        .all(|&(field, value)| match op.field_layout(field) {
            FieldLayout::ResultRegister if field == InstructionField::A && numeric_local => true,
            // A lane call reads its return flag from the field (`call_fits_in_lane`), as does
            // the spread of a kept rest view (`call_rest_miss`), whose every path completes a
            // returning call.
            FieldLayout::ResultRegister
                if field == InstructionField::A
                    && (op.traits().has(quench_runtime::bytecode::Traits::FRAME_RETURNABLE)
                        || op.traits().has(quench_runtime::bytecode::Traits::REST_SPREAD_CALL)
                        || (quench_runtime::staged::computes_binary(op) && instruction.returns_from_frame())) =>
            {
                instruction.result_register() < registers
            }
            FieldLayout::FieldBase => {
                value == quench_runtime::bytecode::FieldBase::NESTED
                    || quench_runtime::bytecode::FieldBase(value)
                        .register_index()
                        .is_none_or(|register| register < registers)
            }
            FieldLayout::OptionalRegister | FieldLayout::OptionalReadRegister => value
                .checked_sub(quench_runtime::bytecode::OPTIONAL_REGISTER_BIAS)
                .is_none_or(|register| register < registers),
            FieldLayout::Operand => {
                let operand = quench_runtime::bytecode::Operand(value);
                operand.kind() != Some(quench_runtime::bytecode::OperandKind::Register)
                    || operand.payload() < registers
            }
            layout => !layout.is_register_field() || value < registers,
        });
    let window_fits = op.field_layout(InstructionField::B) != FieldLayout::RegisterWindowBase
        || u32::from(instruction.b()) + u32::from(instruction.c()) <= u32::from(registers);
    let pair_fits = op.immediate_layout() != ImmediateLayout::RegisterPair || {
        let (first, second) = ImmediateLayout::register_pair(instruction.imm());
        first < registers && second < registers
    };
    registers_fit && window_fits && pair_fits
}

/// The record operand of `instruction` at `pc`, or `None` when its jump or
/// constant cannot be resolved.
pub(crate) fn operand_in_lane(
    instruction: WideInstruction,
    pc: usize,
    constants: &[Value],
) -> Option<LaneOperand> {
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    Some(match instruction.op().immediate_role() {
        ImmediateRole::JumpTarget => {
            let distance = isize::try_from(instruction.imm()).ok()? - isize::try_from(pc).ok()?;
            LaneOperand {
                jump: distance.checked_mul(record)?,
            }
        }
        // A case selector reads its run of constants from the context by index, as a constant
        // array reads its elements' (the `MakeConstArray` row).
        ImmediateRole::ConstantIndex
            if quench_runtime::staged::selects_string_case(instruction.op()) || quench_runtime::staged::const_array_family(instruction.op()) =>
        {
            let start = instruction.imm() as usize;
            constants.get(start..start.checked_add(usize::from(instruction.b()))?)?;
            LaneOperand { imm: instruction.imm() }
        }
        ImmediateRole::ConstantIndex => LaneOperand {
            constant: *constants.get(instruction.imm() as usize)?,
        },
        _ if holds_field_hint(instruction.op()) => LaneOperand {
            field: FieldOperand { imm: instruction.imm(), shape: quench_runtime::vm::NO_FIELD_RECEIVER },
        },
        _ if instruction.op().makes_script_call() => LaneOperand {
            call: CallOperand { imm: instruction.imm(), memo: NO_READ_MEMO },
        },
        _ => LaneOperand {
            imm: instruction.imm(),
        },
    })
}

/// A `Call` runs in the lane only as the general loop pushes one in place: it evaluates no
/// direct or parameter-scope `eval`, which a window activation's call record runs apart (`lane_eval`). It, `GetField`, `Unary` and `Binary` may return their
/// result from the frame (`returns_in_lane`, `returning_miss`); a returning record runs no
/// other result flag.
pub(super) fn call_fits_in_lane(instruction: WideInstruction) -> bool {
    match instruction.op() {
        _ if quench_runtime::staged::call_family(instruction.op()) || quench_runtime::staged::call_array_family(instruction.op())
            || quench_runtime::staged::call_spread_family(instruction.op()) => {
            returns_in_lane(instruction)
                && !instruction.direct_eval()
                && !instruction.parameter_eval()
        }
        _ if quench_runtime::staged::forwarded_call_family(instruction.op()) => returns_in_lane(instruction),
        // `super(...)` takes either argument form (`derived_frames::super_construct_in_frame`).
        _ if quench_runtime::staged::construction_family(instruction.op()) => {
            returns_in_lane(instruction)
                && (instruction.is_super_construct() || !ImmediateLayout::construct_array_arguments(instruction.imm()))
        }
        _ if quench_runtime::staged::field_read_family(instruction.op()) => returns_in_lane(instruction) || sinks_field_in_lane(instruction),
        _ if quench_runtime::staged::unary_family(instruction.op()) || quench_runtime::staged::object_pair_family(instruction.op())
            || quench_runtime::staged::super_const_array_family(instruction.op()) || quench_runtime::staged::object_literal_family(instruction.op()) => {
            returns_in_lane(instruction)
        }
        _ if quench_runtime::staged::binary_family(instruction.op()) && instruction.returns_from_frame() => returns_in_lane(instruction),
        _ if spreads_rest_view(instruction) => returns_in_lane(instruction),
        _ => true,
    }
}

/// Whether `instruction`'s staged row is a call spreading a rest parameter kept as a view
/// (`Passing::Rest`): the interpreter's own decode names it, so no opcode is listed here.
pub(super) fn spreads_rest_view(instruction: WideInstruction) -> bool {
    matches!(quench_runtime::staged::Operation::<u64, ()>::decode_names(instruction),
        Some(quench_runtime::staged::Operation::SpreadCall { passing: quench_runtime::staged::Passing::Rest, .. }))
}

/// A `Call` record's operand, flagged when only the `Await` right after the call reads its
/// result: the await overwrites the result register, or liveness (`live`) shows it dead after
/// the await. A settling call there makes its promise only where one becomes observable
/// (`ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG`). Liveness is derived only for a call
/// followed by an `Await`, of a function whose suspension derives it anyway
/// (`Vm::suspend_activation`).
pub(super) fn call_awaited_marked_in_lane<'live>(
    operand: LaneOperand,
    decoded: &[WideInstruction],
    pc: usize,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
) -> LaneOperand {
    let call = decoded[pc];
    let Some(&awaited) = decoded.get(pc + 1) else {
        return operand;
    };
    if !quench_runtime::staged::calls(call.op()) || call.result_flags() != 0 || !quench_runtime::staged::awaits(awaited.op()) {
        return operand;
    }
    let result = call.result_register();
    let feeds = awaited.register_b() == result
        && (awaited.result_register() == result
            || live()
                .and_then(|masks| masks.get(pc + 2))
                .is_some_and(|live| quench_runtime::compile::liveness::register_dead(live, usize::from(result))));
    if !feeds {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`operand_in_lane`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG }
}

/// A `Call` record's operand, flagged when liveness (`live`, the registers live before each
/// pc) shows its result register dead after the call: nothing reads the value the callee
/// returns (`ImmediateLayout::lane_call_result_used`).
pub(super) fn call_result_marked_in_lane<'live>(
    operand: LaneOperand,
    instruction: WideInstruction,
    pc: usize,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
) -> LaneOperand {
    if !quench_runtime::staged::calls(instruction.op()) {
        return operand;
    }
    let Some(masks) = live() else {
        return operand;
    };
    if !quench_runtime::compile::liveness::call_result_dead(instruction, pc, masks) {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`operand_in_lane`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_UNUSED_FLAG }
}

/// Whether the `Call` at `pc` is an iteration step: no arguments, its result checked by the
/// `RequireIteratorResult` after it, as the compiler emits a for-of or destructuring step,
/// whose result register only the step's own `done` and `value` reads take
/// (`iterator_step_miss`).
pub(super) fn iteration_step_in_lane(decoded: &[WideInstruction], pc: usize) -> bool {
    let call = decoded[pc];
    call.result_flags() == 0
        && ImmediateLayout::argument_count(call.imm()) == 0
        && decoded.get(pc + 1).is_some_and(|next| {
            quench_runtime::staged::requires_iterator_result(next.op()) && next.register_b() == call.result_register()
        })
}

/// How the iteration step at `pc` (`iteration_step_in_lane`) begins a for-of step: after its
/// `RequireIteratorResult`, a plain read of the result into a register, a test of that
/// register (`ForOfTest`), and a second plain read of the result where the body begins.
/// `iterator_step_reads_miss` then makes the reads from the step itself; their atoms it
/// tests as it runs.
pub(super) fn for_of_step_in_lane(decoded: &[WideInstruction], pc: usize) -> Option<ForOfTest> {
    let result = decoded[pc].result_register();
    let record = |offset: usize| decoded.get(pc + offset).copied();
    let reads_result = |offset: usize| {
        record(offset).is_some_and(|read| quench_runtime::staged::reads_named_field(read.op()) && read.result_flags() == 0 && read.b() == result)
    };
    record(FOR_OF_REQUIRE_RESULT).filter(|require| quench_runtime::staged::meaning_fast::require_iterator_result_operand(*require).is_some())?;
    let done = record(FOR_OF_DONE_READ).filter(|_| reads_result(FOR_OF_DONE_READ))?.result_register();
    let test = record(FOR_OF_DONE_TEST).filter(|test| test.a() == done)?;
    let form = match quench_runtime::staged::truth_jump(test.op())? {
        false => ForOfTest::Emitted,
        true => ForOfTest::Threaded,
    };
    let exit = record(form.exit())?;
    let leaves = (if form == ForOfTest::Emitted { quench_runtime::staged::jump_family(exit.op()) } else { quench_runtime::staged::truth_jump(exit.op()) == Some(true) })
        && exit.jump_target() as usize > pc + form.value_read();
    let enters = form == ForOfTest::Threaded || test.jump_target() as usize == pc + form.value_read();
    (leaves && enters && reads_result(form.value_read())).then_some(form)
}
