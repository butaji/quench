//! Keeping activations in a lane: the call activation that keeps its arguments, the script call frame it returns, the wasm handler lookup, and the return completion rows.
use super::*;

/// `activate_in_lane` for a target that keeps its passed values, or for values a forwarding
/// activation or frame kept: the activation's registers and the callee's first record, or
/// the record to continue at when the call takes the general path. Out of line, so the
/// common activation keeps its code. Without `MAY_CROSS` its callers route the running
/// program's targets, as every path but a plain call's makes them (`script_target_in_lane`),
/// so it takes that fact as proven.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn activate_keeping_in_lane<const MAY_CROSS: bool>(
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
    let count = usize::from(target.body.registers);
    // A forwarding activation keeps every value passed after its registers, as does one for
    // a skipped forwarding constructor, whose frame passed them on, and one passed more values
    // than it has parameters.
    // A rest body keeps nothing its rest array holds, unless a kept `arguments` or legacy
    // reflection reads them (`TARGET_GATHERS_KEEPING`).
    let gathers = target.body.entry == CallEntry::framed(Framed::Gathers);
    // A rest view keeps them too, as `activate_gathering_in_lane` keeps them: its reads and
    // its frame, once materialized, take the values passed, never a rest array.
    let keeps = forwarder.is_some()
        || if gathers {
            target.body.forms & (TARGET_GATHERS_KEEPING | TARGET_REST_VIEW) != 0
        } else {
            target.body.forwards() || argc > usize::from(target.body.params)
        };
    let window_size = if keeps { count + argc } else { count };
    // The rest array, made before the window is: the values past the other parameters, read
    // in place from the caller's window or the kept values.
    // A rest view that keeps the values reads them in place (`LaneFrame::rest_view`).
    let rest = (gathers && !(keeps && target.body.forms & TARGET_REST_VIEW != 0)).then(|| {
        let fixed = usize::from(target.body.params) - 1;
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
    let shape = quench_runtime::staged::call_keeping::KeepingWindow { registers: target.body.registers, params: target.body.params, keeps, gathers };
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
        target.body.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.closure.callee,
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
    let base = unsafe { (*target.body.view).records() };
    unsafe {
        (*cx).registers = callee_registers;
        (*cx).base = base;
    }
    quench_runtime::staged::call_keeping::enter_context(
        &mut quench_runtime::staged::Concrete::pure(), callee_registers.0.expose_provenance() as u64,
        this.to_bits(), target.closure.env.to_bits(), target.closure.capture.expose_provenance() as u64,
    );
    if cross {
        enter_program_in_lane(cx, target.body.program());
    }
    let start = if constructs { base } else { unsafe { (*target.body.view).start() } };
    Ok((callee_registers, start))
}

/// A frameless call without room on the lane stack: grow the stack, then take the
/// general path, which pushes a frame. Out of line so the activation path saves no
/// registers.
#[cold]
#[inline(never)]
pub(in super::super) fn script_call_frame_in_lane(ip: Ip, room: usize) -> Ip {
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
pub(in super::super) fn wasm_lane_handler(instruction: WideInstruction) -> Option<Option<LaneHandler>> {
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
pub(in super::super) fn lane_return_one_op<const LEAF: u8>(cx: *mut LaneContext, window: Registers, value: Value) -> Option<(Registers, Ip)> {
    quench_runtime::staged::call_return::step_return_one_op_of(&mut LaneScriptReturn::<LEAF> { cx, window }, value)
}

/// The completion `return_from_in_lane` expects of the activation it returns from.
#[inline(always)]
pub(in super::super) const fn returning_completion(crosses: bool, constructs: bool, settles: bool) -> Completion {
    if crosses { Completion::Cross } else if constructs { Completion::Constructs } else if settles { Completion::SettlesPromise } else { Completion::Plain }
}
