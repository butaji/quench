//! Environment activations: a call of a function whose environment window the lane fills in place, and the closures that capture it.

use super::*;

/// The frameless target of another program's function `callee` a native calls from the
/// lane (`Vm::lane_native_callback`), as a trap's (`proxy_traps::lane_trap_target`).
pub(super) fn lane_cross_callback_target<H: Host>(cx: *mut LaneContext<H>, callee: Value) -> Option<(ScriptCallTarget, bool)> {
    proxy_traps::lane_trap_target(cx, callee).filter(|&(_, crosses)| crosses)
}

/// The activation of a native's call of `target` with `args` from the lane's running
/// function (`Vm::lane_native_callback`), as the lane's own call would make it from the call
/// record `call`, completing to the native (`Completion::ReturnsToNative`): by its shape
/// (`PreparedShape`), or, for a gathering target, whose rest array it makes, or a call
/// without room, which grows the stack for the general path it takes, as
/// `lane_activate_keeping` makes it. Its first record, or `None` where the lane would not
/// activate the call.
pub(super) fn lane_push_callback<H: Host>(
    cx: *mut LaneContext<H>,
    call: Ip,
    target: &ScriptCallTarget,
    args: &[Value],
    this: Value,
    crosses: bool,
) -> Option<Ip> {
    debug_assert_eq!(crosses, lane_crosses::<H, true>(cx, target), "a native's lane callee crosses where its program is another");
    if !crosses {
        // SAFETY: the running lane's context, which nothing else accesses.
        let shape = PreparedShape::of(unsafe { &*cx }, call, target, args.len());
        if let Some(shape) = &shape
            && let Some(start) = lane_push_prepared(cx, &shape.activation, shape.room, &shape.entry(target, this), args)
        {
            return Some(start);
        }
    }
    let (r, arguments) = (unsafe { (*cx).registers }, ActivationArguments::Kept(args.as_ptr(), args.len()));
    let (_, start) = if crosses {
        lane_activate_keeping::<H, true>(cx, r, call, target, arguments, this, false, None).ok()?
    } else {
        lane_activate_keeping::<H, false>(cx, r, call, target, arguments, this, false, None).ok()?
    };
    let vm = unsafe { vm(cx) };
    if let Some(ActivationKind::Script(script)) = vm.lane_stack.activations.last_mut().map(|activation| &mut activation.kind) {
        script.completion = Completion::ReturnsToNative;
    }
    Some(start)
}

/// Writes the `passed` values into the window's locals at `local`: each of the first `bound` as
/// its parameter's local, and every one after the `locals`, as the value a frame keeps for legacy
/// reflection.
///
/// # Safety
/// `local` starts room for the `locals` and the `passed` values after them.
pub(super) unsafe fn write_window_arguments(local: *mut Value, locals: usize, passed: &[Value], bound: usize) {
    // SAFETY: the caller's contract.
    unsafe {
        for (slot, &value) in passed.iter().enumerate() {
            if slot < bound {
                local.add(slot).write(value);
            }
            local.add(locals + slot).write(value);
        }
    }
}

/// `lane_activate` of an environment target (`CallEntry::Environment`): its window holds
/// its registers, then its environment, made now as the frame's promotion would make it
/// (each captured slot its parameter's argument or `undefined`, every other slot owned
/// elsewhere), then its frame locals (each parameter its argument, a captured slot owned
/// by the environment, every other local `undefined`), then the values passed. The
/// environment is reachable only from the window until the lane leaves, which makes the
/// activation a frame holding it.
#[inline(always)]
pub(super) fn lane_activate_environment<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    (this, constructs): (Value, bool),
) -> Result<(Registers, Ip), Ip> {
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    // Another program's callee enters its program and completes across (`Completion::Cross`).
    let cross = target.program() != unsafe { (*cx).program };
    let Ok(argc) = u16::try_from(passed.len()) else {
        return Err(exit(ip));
    };
    if i.a & crate::bytecode::RETURN_REGISTER != 0 && !lane_returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    // SAFETY: the program store keeps a derived view alive with its program, and an
    // environment target's view has a window (`ScriptCallTarget::entry`).
    let layout = unsafe { (*target.view).environment_window.unwrap_unchecked() };
    // A body reading its `arguments` keeps every value passed past its parameters too (see
    // `passes_extra_values`); elsewhere the window keeps only those its parameters bind.
    let Some(extra) = passes_extra_values(vm, target, layout.rest, argc) else {
        return Err(exit(ip));
    };
    if vm.with_scope_entered {
        return Err(exit(ip));
    }
    let (passed, argc) = kept_arguments(passed, argc, extra, target.params);
    // A rest body keeps every value passed, the room sized for its parameters and the rest.
    let extra = usize::from(argc.saturating_sub(target.params));
    if !environment_room_fits(vm, target, extra) {
        return Err(lane_script_call_frame::<H>(ip, usize::from(target.room) + extra));
    }
    // The rest array, made before the window is: the values past the other parameters,
    // which the rest parameter's local holds (`lane_activate_keeping` gathers likewise).
    let gathered = gather_rest_parameter(vm, target, passed, layout.rest);
    // SAFETY: capacity holds the window (`ScriptCallTarget::room`); each slot is written
    // before the length covers it, and no activation's window moves.
    let callee_registers = unsafe { fill_environment_window(&mut vm.lane_stack, target, &layout, (passed, argc), gathered) };
    push_environment_activation(cx, vm, (ip, r), target, (argc, (constructs, cross)), callee_registers);
    // SAFETY: the program store keeps a derived view alive with its program.
    let base = unsafe { (*target.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    callee_registers.set_env(target.env);
    callee_registers.set_capture(target.capture);
    callee_registers.set_this(this);
    if cross {
        lane_enter_program(cx, target.program());
    }
    vm.programs.record_js_jit_lane_entry(target.program(), target.function);
    Ok((callee_registers, base))
}

/// Whether a call of `target` passing `argc` values passes more values than its parameters that
/// the body reads: a body that binds no rest parameter and passes values past its parameters
/// keeps all of them where its `arguments` (`CallPlan::reads_passed_arguments`) or legacy
/// reflection reads them. `None` where the body's program is gone or a legacy-reflection body
/// reads them, which the lane leaves to the general path.
#[inline(always)]
fn passes_extra_values<H: Host>(vm: &Vm<H>, target: &ScriptCallTarget, rest: bool, argc: u16) -> Option<bool> {
    if argc > target.params && !rest {
        reads_extra_arguments(vm, target)
    } else {
        Some(false)
    }
}

/// The values an environment activation keeps: the `params` its parameters bind, when no
/// extra values are read, else every value passed (with `argc` the count kept).
#[inline(always)]
fn kept_arguments(passed: &[Value], argc: u16, extra: bool, params: u16) -> (&[Value], u16) {
    if extra { (&passed[..usize::from(params)], params) } else { (passed, argc) }
}

/// Whether the stacks have room for an environment activation of `target` with `extra` values
/// past its window's parameters.
#[inline(always)]
fn environment_room_fits<H: Host>(vm: &Vm<H>, target: &ScriptCallTarget, extra: usize) -> bool {
    vm.lane_stack.activations.len() < vm.lane_stack.activation_limit.get()
        && vm.lane_stack.values.room() >= usize::from(target.room) + extra
}

/// The rest parameter's array of the values passed past the other parameters, when `rest`.
#[inline(always)]
fn gather_rest_parameter<H: Host>(vm: &mut Vm<H>, target: &ScriptCallTarget, passed: &[Value], rest: bool) -> Option<Value> {
    if rest {
        let fixed = usize::from(target.params) - 1;
        Some(vm.rest_parameter_array(passed.get(fixed..).unwrap_or_default()))
    } else {
        None
    }
}

/// Pushes the activation of an environment call of `target` made from `ip` over the window
/// `callee_registers`, which the caller's registers `r` and base in the context call.
#[inline(always)]
fn push_environment_activation<H: Host>(
    cx: *mut LaneContext<H>,
    vm: &mut Vm<H>,
    (ip, r): (Ip, Registers),
    target: &ScriptCallTarget,
    (argc, (constructs, cross)): (u16, (bool, bool)),
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
            completion: Completion::of(constructs, cross, false),
            forwards: false,
            environment: true,
            record: std::ptr::null_mut(),
        }),
    };
    // SAFETY: the capacity check above leaves room for the activation.
    unsafe { vm.lane_stack.push_activation(activation, callee_registers.0) };
}

/// Whether the body of `target`, which is passed more values than its parameters and binds no
/// rest parameter, reads the values past them (`CallPlan::reads_passed_arguments`). `None`
/// where the body's program is gone or a legacy-reflection body reads them, which the lane
/// leaves to the general path.
pub(super) fn reads_extra_arguments<H: Host>(vm: &Vm<H>, target: &ScriptCallTarget) -> Option<bool> {
    let code = vm.programs.residual(target.program())?;
    let plan = code.function(target.function as usize).call_plan(code);
    if !plan.reads_passed_arguments && plan.legacy_reflection {
        return None;
    }
    Some(!plan.reads_passed_arguments)
}

/// Writes the environment entries of a window whose entries start at `entries`: no environment
/// yet, its captured slots, and its storage laid out in place for `locals` locals.
///
/// # Safety
/// `entries` starts the entries of a window with room for them and the storage.
pub(super) unsafe fn write_environment_entries(entries: *mut Value, layout: &EnvironmentWindow, locals: usize) {
    unsafe {
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(Value::DELETED);
        entries
            .add(usize::from(WINDOW_CAPTURED))
            .write(std::mem::transmute::<u64, Value>(layout.captured));
        let storage = crate::heap::EnvironmentSlots::lay_out_in_place(entries.add(usize::from(WINDOW_STORAGE)), locals);
        entries
            .add(usize::from(WINDOW_SLOTS))
            .write(std::mem::transmute::<u64, Value>(storage as u64));
    }
}

/// Fills the window an environment activation of `target` takes at the top of the value stack:
/// its registers and locals `undefined`, its environment's window entries, the `passed` values
/// (at most its parameters, the rest excepted) and the `gathered` rest array.
///
/// # Safety
/// The value stack has room for the window (`ScriptCallTarget::room`), which no activation's
/// window moves.
pub(super) unsafe fn fill_environment_window(
    stack: &mut LaneStack,
    target: &ScriptCallTarget,
    layout: &EnvironmentWindow,
    (passed, argc): (&[Value], u16),
    gathered: Option<Value>,
) -> Registers {
    let count = usize::from(target.registers);
    let locals = usize::from(layout.locals);
    let held = count + usize::from(WINDOW_LOCALS);
    unsafe {
        let registers = stack.values.top();
        // Every register live at entry (`ScriptCallTarget::fill`) and every local starts
        // `undefined` within the target's reserved room (`ScriptCallTarget::room`); the
        // metadata entries are then written, and parameter `k` local `k` with its argument
        // where one was passed. The environment is made when one is needed.
        fill_window(registers, target.entry_registers(), Value::UNDEFINED);
        fill_window(registers.add(count), held - count + locals, Value::UNDEFINED);
        write_environment_entries(registers.add(count), layout, locals);
        let local = registers.add(held);
        // The few values a call passes are copied in place: one pass writes each as its
        // parameter's local and, after the locals, as the value a frame keeps for legacy
        // reflection, with no library copy per run.
        write_window_arguments(local, locals, passed, usize::from(argc.min(target.params)));
        write_window_bindings(local, (target, layout), gathered);
        stack.values.set_top(registers.add(held + locals + usize::from(argc) + lane_values::CONTEXT));
        Registers(registers)
    }
}

/// Writes the rest array `gathered` as the rest parameter's local and the callee as the local of
/// its own name's binding, where the window holds them.
///
/// # Safety
/// `local` starts the locals of the window of `target`.
unsafe fn write_window_bindings(local: *mut Value, (target, layout): (&ScriptCallTarget, &EnvironmentWindow), gathered: Option<Value>) {
    unsafe {
        if let Some(gathered) = gathered {
            local.add(usize::from(target.params) - 1).write(gathered);
        }
        if let Some(slot) = layout.self_binding {
            local.add(usize::from(slot)).write(target.callee);
        }
    }
}

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
/// `entries` starts the entries of a live environment window (`lane_activate_environment`).
pub(super) unsafe fn window_environment<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, entries: *mut Value,
    function: u32, parent: Value, program: ProgramId, this: Value, bindings: Option<Vec<(crate::bytecode::Atom, Value)>>) -> Value {
    // SAFETY: as the caller guarantees.
    unsafe {
        let made = *entries.add(usize::from(WINDOW_ENVIRONMENT));
        if !made.is_deleted() {
            return made;
        }
        let slots = window_slots_taken(entries);
        let scope = window_scope(vm, code, (function, program, this), bindings);
        let (environment, storage) = vm.heap.alloc_environment(parent, function, slots, scope);
        link_window_storage(vm, parent, storage);
        record_window_environment(entries, environment, storage);
        environment
    }
}

/// The scope of the environment a window of `function` makes: its dynamic `bindings`, and the
/// `this` of a non-arrow whose closures' arrows (or its body) observe it, as a frame does when a
/// closure or the body itself can observe its implicit bindings (`Function::activation_bindings_observable`).
/// Any other scope names only its program, which needs no allocation.
#[inline(always)]
fn window_scope<H: Host>(vm: &Vm<H>, code: &ResidualProgram, (function, program, this): (u32, ProgramId, Value),
    bindings: Option<Vec<(crate::bytecode::Atom, Value)>>) -> crate::heap::EnvironmentScopeRef {
    let mut bindings = bindings.unwrap_or_default();
    let body = code.function(function as usize);
    if !body.is_arrow && body.activation_bindings_observable(code) {
        bindings.insert(0, (vm.runtime_atoms.lexical_this, this));
    }
    window_scope_ref(program, bindings)
}

/// Records the environment a window made, and its storage, in the window's entries.
///
/// # Safety
/// `entries` starts the entries of a live environment window.
#[inline(always)]
unsafe fn record_window_environment(entries: *mut Value, environment: Value, storage: *const crate::heap::EnvironmentSlots) {
    unsafe {
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(environment);
        entries
            .add(usize::from(WINDOW_SLOTS))
            .write(std::mem::transmute::<u64, Value>(storage as u64));
    }
}

/// Links the storage of an environment made over `parent` to its parent's, for its closures' outer
/// captures (`EnvironmentSlots::link`), as a capture walk would find the parent's.
///
/// # Safety
/// `storage` is the live storage of the environment just made.
pub(super) unsafe fn link_window_storage<H: Host>(vm: &Vm<H>, parent: Value, storage: *const crate::heap::EnvironmentSlots) {
    if let Some(Cell::Environment { function: owner, slots: parent_slots, .. }) = vm.heap.get(parent)
        && *owner != u32::MAX
        && *owner != super::ROOT_FUNCTION_ID
        && let Some(parent_storage) = parent_slots.dense_storage()
        && let Some(own) = unsafe { (*storage).dense_storage() }
    {
        crate::heap::EnvironmentSlots::link(own, parent_storage, vm.heap.collection_epoch());
    }
}

/// The slots of an environment made from the window whose entries are `entries`: the locals a
/// closure captured, each cleared in the window, which the environment owns from then on.
///
/// # Safety
/// `entries` starts the entries of a live environment window (`lane_activate_environment`).
pub(super) unsafe fn window_slots_taken(entries: *mut Value) -> crate::heap::EnvironmentSlots {
    // SAFETY: as the caller guarantees.
    unsafe {
        let captured = std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_CAPTURED)));
        let in_place = &*(std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_SLOTS)))
            as *const crate::heap::EnvironmentSlots);
        let locals = entries.add(usize::from(WINDOW_LOCALS));
        let window = EnvironmentWindow { locals: in_place.len() as u16, captured, rest: false, self_binding: None };
        // SAFETY: the mask names locals of the window, which hold owned values.
        let slots = crate::heap::EnvironmentSlots::from_captured(in_place.len(), window.captured, locals);
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
pub(super) fn window_scope_ref(program: ProgramId, bindings: Vec<(crate::bytecode::Atom, Value)>) -> crate::heap::EnvironmentScopeRef {
    if bindings.is_empty() {
        return crate::heap::EnvironmentScopeRef::of_program(program.raw());
    }
    crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
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
/// (`lane_planned_construction`), which comes first. Out of line, so the construct keeps
/// its tail dispatch.
#[inline(never)]
pub(super) fn lane_initialize_fields<H: Host>(
    vm: &mut Vm<H>,
    code: &ResidualProgram,
    callee: Value,
    constructor: Value,
    function: u32,
    argument: impl Fn(u16) -> Value,
) -> Option<(Value, bool)> {
    let stores = code.function(function as usize).initializer_stores(&code.constants)?;
    lane_initialize_stores(vm, callee, constructor, |vm, this, each| {
        lane_add_stores(vm, code, this, stores, argument, |_, _| None, each)
    })
}
