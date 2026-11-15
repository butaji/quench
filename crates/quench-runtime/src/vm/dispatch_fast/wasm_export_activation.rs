//! Exported Wasm calls from JavaScript: the activation or frame a call of an export makes, and its return.

use super::*;

/// A JavaScript `Call` of an exported Wasm function that `Vm::wasm_export_frame_call`
/// admits: its frame, over the instance environment, pushed with the pending call that
/// converts its result (`CallCompletion::WasmExport`) and entered in this lane, as a Wasm
/// call's frame is (`wasm_calls::lane_call_frame`), so the call nests no native frame and no
/// run of the general loop. Lane activations become frames first, as for any frame call.
/// `None` leaves the call to the native: a run that is not a frame's own JavaScript run
/// (`LaneContext::script_calls`), a callee native code may run, a call completing a caller
/// the lane does not complete (`lane_returning_call_admitted`), or another program the lane
/// does not admit.
#[inline(never)]
pub(super) fn lane_wasm_export_frame<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let returns = i.a & crate::bytecode::RETURN_REGISTER != 0;
    if !unsafe { (*cx).script_calls }
        || (returns && !lane_returning_call_admitted(cx))
        || vm.execution_mode == ExecutionMode::Compiled
    {
        return None;
    }
    // SAFETY: the call's window is registers of the running function, read before any
    // activation becomes a frame.
    let args = unsafe { lane_call_window(r, i) };
    let (program, function, env, encoded, result) = vm.wasm_export_frame_call(r.get(i.b), args)?;
    let callee_code = vm.programs.get(program)?;
    if !vm.lane_admits(&callee_code) || !vm.lane_call_may_derive(program, function) {
        return None;
    }
    if let Some(resumed) = lane_activate_wasm_export(cx, r, ip, program, function, env, &encoded, result) {
        return Some(resumed);
    }
    vm.materialize_lane_activations(cx);
    Some(push_wasm_export_frame(cx, vm, ip, (program, callee_code, function, env, &encoded), (i.a, result)))
}

/// Pushes the frame of an exported Wasm call once the lane made its activations frames: over the
/// caller's frame at `ip`, in the callee's program, as `lane_push_frame_call` does, and runs the
/// frame's Wasm body as a Wasm run until it returns (`lane_return_frame`). A trap while the frame
/// is pushed exits the lane with it.
pub(super) fn push_wasm_export_frame<H: Host>(cx: *mut LaneContext<H>, vm: &mut Vm<H>, ip: Ip,
    (program, callee_code, function, env, encoded): (ProgramId, Rc<ResidualProgram>, u32, Value, &[Value]),
    (destination, result): (u16, Option<crate::WasmType>)) -> Resumed {
    let caller = unsafe { (*cx).frame() };
    // SAFETY: the call record lies in the running view.
    let call_pc = unsafe { ip.offset_from((*cx).base) } as usize;
    vm.frames[caller].pc = call_pc + 1;
    vm.frames[caller].pc_is_resume = true;
    // The callee runs in its own program, which its frame belongs to and the loop's executing
    // one sets aside as a frame call into another program does (`lane_push_frame_call`).
    let caller_program = unsafe { (*cx).program };
    lane_enter_program(cx, program);
    let call_guard = match vm.push_wasm_frame_over(&callee_code, function, env, encoded) {
        Ok(guard) => guard,
        Err(error) => return trap_wasm_export_frame(cx, vm, ip, caller_program, error),
    };
    let saved = unsafe { swap_executing_program(cx, program, callee_code) };
    // The run goes on as a Wasm run (`LaneContext::inline_calls`) until the frame returns,
    // which completes on the general path, as it completes any call into another program
    // (`lane_return_frame`); the caller's lane run starts afresh there.
    unsafe {
        (*cx).inline_calls = true;
        (*cx).script_calls = false;
    }
    // SAFETY: the general loop's pending calls outlive the lane run.
    unsafe { &mut *(*cx).pending }.push(PendingGeneralCall {
        caller,
        caller_cursor: None,
        call_pc: call_pc as u32,
        destination: destination & crate::bytecode::REGISTER_MASK,
        completion: CallCompletion::WasmExport(result),
        returns: destination & crate::bytecode::RETURN_REGISTER != 0,
        call_guard: Some(call_guard),
        caller_program: Some(saved),
    });
    // The pushed frame's guard narrows the activations the thread admits.
    vm.lane_stack.refresh_activation_limit();
    lane_enter_frame(cx, caller + 1, 0)
}

/// The trap of an exported Wasm call whose frame cannot be pushed: an exhausted guest stack traps,
/// which reaches the caller as a `RuntimeError`, in the caller's program again.
pub(super) fn trap_wasm_export_frame<H: Host>(cx: *mut LaneContext<H>, vm: &mut Vm<H>, ip: Ip, caller_program: ProgramId, error: JsError) -> Resumed {
    lane_enter_program(cx, caller_program);
    // SAFETY: the context's program, the caller's again, outlives the lane run.
    vm.lane_raised = Some(vm.trap_raised_in_javascript(unsafe { &*(*cx).code }, error));
    Resumed::Exit(exit(ip))
}

/// Makes `program` the general loop's executing program, its residual `code` retained, and
/// returns the program it replaces.
///
/// # Safety
/// The general loop's executing program outlives the lane run.
pub(super) unsafe fn swap_executing_program<H: Host>(cx: *mut LaneContext<H>, program: ProgramId, code: Rc<ResidualProgram>) -> super::dispatch_frame::SavedProgram {
    let executing = unsafe { &mut *(*cx).executing };
    let saved = super::dispatch_frame::SavedProgram {
        id: executing.id,
        retained: std::mem::replace(&mut executing.retained, Some(code)),
    };
    executing.id = program;
    saved
}

/// `lane_wasm_export_frame`'s call as a frameless Wasm activation of the export
/// (`ActivationKind::WasmEntry`) over its instance environment, pushed as a Wasm call's
/// activation is (`wasm_calls::enter_wasm_function`) beside the caller's activations, which
/// stay activations. The run goes on as a Wasm run (`LaneContext::inline_calls`) until the
/// activation returns (`lane_complete_activation`). `None`, which changes nothing, where the
/// callee has no lane view yet or the lane no room for it: the frame call takes it.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn lane_activate_wasm_export<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    ip: Ip,
    program: ProgramId,
    function: u32,
    instance: Value,
    encoded: &[Value],
    result: Option<crate::WasmType>,
) -> Option<Resumed> {
    let vm = unsafe { vm(cx) };
    let (shape, base) = wasm_export_shape(vm, program, function)?;
    let stack = &mut vm.lane_stack;
    let count = usize::from(shape.registers);
    let frame_storage_bytes = count.saturating_mul(std::mem::size_of::<Value>());
    if !wasm_export_fits(stack, frame_storage_bytes, count) || vm.frames.len() + stack.activations.len() >= super::dispatch_frame::WASM_ACTIVATION_LIMIT {
        return None;
    }
    let window = stack.values.len();
    // SAFETY: capacity holds the window; each slot is written before the length covers it,
    // and no activation's window moves.
    let callee_registers = unsafe { fill_wasm_export_window(stack, r, encoded, shape) };
    let (caller_program, caller_base) = unsafe { ((*cx).program, (*cx).base) };
    // SAFETY: capacity holds the activation.
    unsafe { push_wasm_export_activation(stack, window, (ip, caller_base, r, caller_program), (function, instance, result, frame_storage_bytes)) };
    lane_enter_program(cx, program);
    // A fresh activation's memory register holds no memory until the function's instance
    // binding loads it.
    unsafe { enter_wasm_export_context(cx, (callee_registers, base), instance) };
    let view = Carried { memory: MemoryView::NONE, integers: crate::value::IntegerEncoding::TAG, acc: Slot::of(Value::UNDEFINED) };
    Some(Resumed::At(callee_registers, base, view))
}

/// The activation shape of `function` of `program` and its records, when its lane view has a shape.
pub(super) fn wasm_export_shape<H: Host>(vm: &Vm<H>, program: ProgramId, function: u32) -> Option<(ActivationShape, Ip)> {
    let lane_view = vm
        .programs
        .lane_views(program)?
        .get(function as usize)
        .and_then(super::program_store::derived_lane_view)?;
    Some((lane_view.activation?, lane_view.records()))
}

/// Whether the stacks have room for a Wasm export's activation of `count` registers and
/// `frame_storage_bytes`: its storage within the guest stack's, a limit below its activations,
/// and the value stack's room for its window.
#[inline(always)]
pub(super) fn wasm_export_fits(stack: &LaneStack, frame_storage_bytes: usize, count: usize) -> bool {
    frame_storage_bytes <= crate::stack::GuestCallGuard::remaining_storage_bytes().saturating_sub(stack.wasm_storage_bytes)
        && stack.activations.len() < stack.activation_limit.get()
        && stack.values.room() >= fill_room::<ACTIVATION_FILL_BLOCK>(count)
}

/// Fills the window a Wasm export's activation takes at the top of the value stack: its
/// registers initialized, its parameters from `encoded`, and the caller's receiver and environment.
///
/// # Safety
/// The value stack has room for the window, which no activation's window moves.
#[inline(always)]
pub(super) unsafe fn fill_wasm_export_window(stack: &mut LaneStack, r: Registers, encoded: &[Value], shape: ActivationShape) -> Registers {
    let count = usize::from(shape.registers);
    let window = stack.values.len();
    let passed = encoded.len().min(usize::from(shape.params));
    let registers = unsafe { stack.values.as_mut_ptr().add(window) };
    unsafe {
        fill_window(registers, count, shape.initial);
        for (offset, &value) in encoded[..passed].iter().enumerate() {
            registers.add(usize::from(shape.parameters) + offset).write(value);
        }
        stack.values.set_len(window + count + lane_values::CONTEXT);
    }
    // A Wasm activation runs with its caller's receiver and environment (`ActivationState::of`).
    Registers(registers).set_this(r.this());
    Registers(registers).set_env(r.env());
    Registers(registers).set_capture(r.capture());
    Registers(registers)
}

/// Pushes the activation of a Wasm export over the window at the top of the value stack, called
/// from `ip` with the caller's registers `r` in `caller_program`.
///
/// # Safety
/// The activation's window is the top of the value stack, and the stack has capacity for it.
pub(super) unsafe fn push_wasm_export_activation(stack: &mut LaneStack, window: usize, (ip, caller_base, r, caller_program): (Ip, *mut Value, Registers, ProgramId), (function, instance, result, frame_storage_bytes): (u32, Value, Option<crate::WasmType>, usize)) {
    let at = unsafe { stack.values.as_mut_ptr().wrapping_add(window) };
    unsafe {
        stack.push_activation(Activation {
            native: super::compiled::ShadowFrame::EMPTY,
            function,
            call: ip,
            caller_base,
            caller_registers: r,
            caller_program,
            kind: ActivationKind::WasmEntry { instance, result, frame_storage_bytes },
        }, at);
    }
}

/// Makes the context run a Wasm export's activation: its registers and base, a fresh memory
/// register, its instance, and a Wasm run (`LaneContext::inline_calls`).
///
/// # Safety
/// The context is the lane's, whose run the activation continues.
pub(super) unsafe fn enter_wasm_export_context<H: Host>(cx: *mut LaneContext<H>, (registers, base): (Registers, Ip), instance: Value) {
    unsafe {
        (*cx).registers = registers;
        (*cx).base = base;
        (*cx).memory = MemoryView::NONE;
        (*cx).instance = instance;
        (*cx).inline_calls = true;
        (*cx).script_calls = false;
    }
}
