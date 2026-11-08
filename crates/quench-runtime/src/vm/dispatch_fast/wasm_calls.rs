//! Wasm calls the lane enters as activations: statically known calls and reference calls.
use super::*;

/// The known call's handler, for the shared handler table.
pub(super) const fn known_call_handler<H: Host>() -> Handler {
    lane_call_known::<H>
}

/// Enters `$function` of the running program and instance as a Wasm call: out of the lane when
/// native code may run it, else as an activation pushed as the general loop's inline call would
/// be; `$no_room` takes the call when the lane has no room. Shared by the known and reference calls.
macro_rules! enter_wasm_function {
    ($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident, $function:expr, $no_room:expr) => {{
        let (cx, r, ip, i, view) = ($cx, $r, $ip, $i, $view);
        if !unsafe { (*cx).inline_calls } {
            return exit(ip);
        }
        let vm = unsafe { vm(cx) };
        // Keep eligible calls on the general dispatcher so they can enter
        // native code. Unsupported calls stay on the fast interpreter lane.
        if vm.execution_mode == super::ExecutionMode::Compiled
            && i.a & crate::bytecode::RETURN_REGISTER == 0
        {
            let program = unsafe { (*cx).program };
            let function = usize::from($function);
            if vm
                .programs
                .has_compiled_function_or_note_fallback(program, function)
                || vm.programs.has_wasm_tier_containing(program, function)
            {
                return exit(ip);
            }
            // `run_compiled_inner` is the one-shot promotion claimant. Count
            // lane-only entries here, but leave the threshold entry for that
            // general-path hook so no handoff token or duplicate count is
            // needed. Terminal calls stay on their tail-call path above the
            // tier gate and are deliberately not observed here.
            if vm
                .programs
                .note_wasm_tier_lane_entry(program, u32::from($function))
            {
                return exit(ip);
            }
        }
        // SAFETY: the program's views outlive the lane run.
        let views = unsafe { &*(*cx).views };
        let stack = &mut vm.lane_stack;
        if let Some(lane_view) = views.get(usize::from($function)).and_then(super::program_store::derived_lane_view)
            && let Some(shape) = lane_view.activation
            && usize::from(shape.registers)
                .saturating_mul(std::mem::size_of::<Value>())
                <= crate::stack::GuestCallGuard::remaining_storage_bytes()
                    .saturating_sub(stack.wasm_storage_bytes)
            && stack.activations.len() < stack.activations.capacity()
            && vm.frames.len() + stack.activations.len()
                < super::dispatch_frame::WASM_ACTIVATION_LIMIT
            && stack.values.capacity() - stack.values.len()
                >= fill_room::<ACTIVATION_FILL_BLOCK>(usize::from(shape.registers))
        {
            let window = stack.values.len();
            let count = usize::from(shape.registers);
            let frame_storage_bytes = count.saturating_mul(std::mem::size_of::<Value>());
            let arguments = ImmediateLayout::call_window_base(i.imm());
            let passed = ImmediateLayout::argument_count(i.imm()).min(shape.params);
            // SAFETY: capacity holds the window; each slot is written before
            // the length covers it, and no activation's window moves.
            let callee_registers = unsafe {
                let registers = stack.values.as_mut_ptr().add(window);
                fill_window(registers, count, shape.initial);
                let mut offset = 0;
                while offset < passed {
                    registers
                        .add(usize::from(shape.parameters + offset))
                        .write(r.get(arguments + offset));
                    offset += 1;
                }
                stack.values.set_len(window + count);
                Registers(registers)
            };
            // SAFETY: capacity holds the activation.
            unsafe {
                stack.push_activation(Activation {
                    function: u32::from($function),
                    call: ip,
                    caller_base: (*cx).base,
                    caller_registers: r,
                    window,
                    caller_program: (*cx).program,
                    kind: ActivationKind::Wasm { memory: view.memory, frame_storage_bytes },
                });
            }
            // A fresh activation's memory register holds no memory until
            // the function's instance binding loads it.
            let base = lane_view.records();
            unsafe {
                (*cx).registers = callee_registers;
                (*cx).base = base;
                (*cx).memory = MemoryView::NONE;
            }
            let view = Carried {
                memory: MemoryView::NONE,
                integers: crate::value::IntegerEncoding::TAG,
                acc: Slot::of(Value::UNDEFINED),
            };
            next!(cx, callee_registers, base, view)
        }
        $no_room
    }};
}

lane_handler! {
    /// A Wasm call to a function of the same program, pushed as the general
    /// loop's inline call would be.
    fn lane_call_known<>(cx, r, ip, i, view) {
        enter_wasm_function!(cx, r, ip, i, view, i.b, resume!(cx, lane_call_frame::<H>(ip)))
    }
}

/// A Wasm view's handler for `op` where it differs from a JavaScript view's.
pub(super) fn wasm_lane_handler<H: Host>(op: Op) -> Option<Option<LaneHandler>> {
    match op {
        // A Wasm reference call enters Wasm functions, never a frameless script call.
        Op::Call => Some(Some(LaneHandler { handler: lane_call_wasm_reference::<H> as Handler, produces: false })),
        // Its handler reads the context a JavaScript view carries where a Wasm view carries
        // its memory; Wasm stores a capture only for the rare `data.drop` and `elem.drop`.
        Op::StoreCapture => Some(None),
        _ => None,
    }
}

lane_handler! {
    /// A Wasm reference call (`call_ref`, or `call_indirect` past its target check) of a
    /// function of the running program and instance, entered as `lane_call_known` enters its
    /// callee. Validation proved the callee's type. Any other callee leaves for the general loop.
    fn lane_call_wasm_reference<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let running = (unsafe { (*cx).program }, vm.frames.last().map(|frame| frame.env));
        let callee = match vm.heap.function(r.get(i.b)) {
            Some(FunctionRecord { kind: FunctionKind::User(program, function), env, .. })
                if (*program, Some(*env)) == running => u16::try_from(*function).ok(),
            _ => None,
        };
        let Some(function) = callee else { return exit(ip) };
        enter_wasm_function!(cx, r, ip, i, view, function, exit(ip))
    }
}
