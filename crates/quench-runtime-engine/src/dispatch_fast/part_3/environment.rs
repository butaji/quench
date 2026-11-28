//! Environment windows of a lane: activating a call whose window needs an environment record.
use super::*;

/// `activate_in_lane` of an environment target (`CallEntry::framed(Framed::Environment)`): its window holds
/// its registers, then its environment, made now as the frame's promotion would make it
/// (each captured slot its parameter's argument or `undefined`, every other slot owned
/// elsewhere), then its frame locals (each parameter its argument, a captured slot owned
/// by the environment, every other local `undefined`), then the values passed. A parameter
/// promoted in place (`EnvironmentWindow::parameter_registers`) starts in its register and
/// leaves its local `undefined`, as a frame's promoted parameter does. The
/// environment is reachable only from the window until the lane leaves, which makes the
/// activation a frame holding it.
#[inline(always)]
pub(in super::super) fn activate_environment_in_lane(
    cx: *mut LaneContext,
    r: Registers,
    ip: Ip,
    target: &ScriptCallTarget,
    passed: &[Value],
    (this, constructs): (Value, bool),
) -> Result<(Registers, Ip), Ip> {
    use quench_runtime::staged::call_environment as environment_row;
    let vm = unsafe { vm(cx) };
    // SAFETY: `ip` is a record of the running view.
    let i = unsafe { &*ip };
    // Another program's callee enters its program and completes across (`Completion::Cross`).
    let cross = target.body.program() != unsafe { (*cx).program };
    let Ok(argc) = u16::try_from(passed.len()) else {
        return Err(exit(ip));
    };
    let returns = i.a & quench_runtime::bytecode::RETURN_REGISTER != 0;
    if returns && !returning_call_admitted(cx) {
        return Err(exit(ip));
    }
    // SAFETY: the program store keeps a derived view alive with its program, and an
    // environment target's view has a window (`ScriptCallTarget::entry`).
    let layout = unsafe { (*target.body.view).environment_window.unwrap_unchecked() };
    let (rest, self_binding) = (layout.rest, layout.self_binding);
    // A body reading its `arguments` keeps every value passed past its parameters too, as
    // a frame does where legacy reflection (`f.arguments`) can read them; elsewhere nothing
    // observes them, and the window keeps only those its parameters bind. Only a call
    // passing more values than the parameters reads the body's plan.
    let extra = argc > target.body.params && !rest && {
        let Some(code) = vm.programs.residual(target.body.program()) else {
            return Err(exit(ip));
        };
        let plan = code.function(target.body.function as usize).call_plan(code);
        if !plan.reads_passed_arguments && plan.legacy_reflection {
            return Err(exit(ip));
        }
        !plan.reads_passed_arguments
    };
    if vm.with_scope_entered {
        return Err(exit(ip));
    }
    let (passed, argc) = if extra { (&passed[..usize::from(target.body.params)], target.body.params) } else { (passed, argc) };
    // A rest body keeps every value passed, the room sized for its parameters and the rest.
    let extra = usize::from(argc.saturating_sub(target.body.params));
    let needed = usize::from(target.body.room) + extra;
    // Both stacks' room, one row (`call_environment::admit_environment`).
    if !environment_row::concrete_admit_environment(
        vm.lane_stack.activation_count() as u64,
        vm.lane_stack.activation_limit.get() as u64,
        vm.lane_stack.values.room() as u64,
        needed as u64,
    ) {
        return Err(script_call_frame_in_lane(ip, needed));
    }
    // The rest array, made before the window is: the values past the other parameters,
    // which the rest parameter's local holds (`activate_keeping_in_lane` gathers likewise).
    // A plain branch: `bool::then` with this closure stayed an out-of-line call on every
    // activation.
    let gathered = if rest {
        let fixed = usize::from(target.body.params) - 1;
        Some(vm.rest_parameter_array(passed.get(fixed..).unwrap_or_default()))
    } else {
        None
    };
    // The environment window, one row (`call_environment::enter_environment`): every register
    // live at entry (`ScriptCallTarget::fill`) and every local starts `undefined` within the
    // target's reserved room (`ScriptCallTarget::room`), the entries are laid out (the
    // environment is made when one is needed), parameter `k` local `k` holds its argument where
    // one was passed and every value is kept after the locals for legacy reflection, then the
    // rest array, the self binding and the frame context (`this`, `env`, `capture`).
    const _: () = assert!(
        environment_row::ENTRY_ENVIRONMENT == WINDOW_ENVIRONMENT
            && environment_row::ENTRY_SLOTS == WINDOW_SLOTS
            && environment_row::ENTRY_CAPTURED == WINDOW_CAPTURED
            && environment_row::ENTRY_STORAGE == WINDOW_STORAGE
            && environment_row::STORAGE_WORDS as usize == quench_runtime::tier_surface::IN_PLACE_SLOTS_WORDS
            && environment_row::ENTRY_WORDS == WINDOW_LOCALS
    );
    let shape = environment_row::EnvironmentShape {
        registers: target.body.registers,
        live: target.body.entry_registers() as u16,
        locals: layout.locals,
        captured: layout.captured,
    };
    let held = shape.held();
    debug_assert!(fill_room::<SCRIPT_FILL_BLOCK>(held) <= usize::from(target.body.room));
    let rest = if let Some(gathered) = gathered { Some((target.body.params - 1, gathered.to_bits())) } else { None };
    let self_binding = if let Some(slot) = self_binding { Some((slot, target.closure.callee.to_bits())) } else { None };
    let stack: &mut LaneStack = &mut vm.lane_stack;
    // SAFETY: capacity holds the window (`ScriptCallTarget::room`); each slot is written
    // before the length covers it, and no activation's window moves.
    let callee_registers = unsafe {
        let registers = stack.values.top();
        environment_row::enter_environment(
            &mut quench_runtime::staged::Concrete::pure(),
            registers.expose_provenance() as u64,
            shape,
            environment_row::EnvironmentPassed {
                arguments: passed.as_ptr().expose_provenance() as u64,
                count: argc,
                bound: argc.min(target.body.params),
                in_registers: argc.min(layout.parameter_registers),
                rest,
                self_binding,
            },
            environment_row::WindowContext {
                this: this.to_bits(),
                env: target.closure.env.to_bits(),
                capture: target.closure.capture.expose_provenance() as u64,
            },
        );
        stack.values.set_top(registers.add(held + usize::from(argc) + lane_values::CONTEXT));
        Registers(registers)
    };
    let activation = Activation::of_parts(
        target.body.function, ip, unsafe { (*cx).base }, r, unsafe { (*cx).program },
        ActivationKind::Script(ScriptActivation {
            callee: target.closure.callee,
            argc,
            completion: Completion::of(constructs, cross, false),
            forwards: false,
            kept: 0,
            environment: true,
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
    // The frame context was written with the window (`call_environment::bind_context`).
    if cross {
        enter_program_in_lane(cx, target.body.program());
    }
    vm.programs.record_js_jit_lane_entry(target.body.program(), target.body.function);
    Ok((callee_registers, base))
}
