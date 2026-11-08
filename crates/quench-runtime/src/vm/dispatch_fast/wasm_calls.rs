//! Wasm function references and calls through them the lane makes in place: `ref.func`,
//! `call_indirect`'s table lookup, and the call of a function of the running instance, as
//! `lane_call_known` calls.
use super::*;

/// A Wasm call to `$function` of the running program (`u32`), pushed as the general loop's
/// inline call would be: the body `lane_call_known` and `lane_wasm_call_value` share.
macro_rules! lane_wasm_call {
    ($cx:ident, $r:ident, $ip:ident, $i:ident, $view:ident, $function:expr) => {{
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
            let function = $function as usize;
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
                .note_wasm_tier_lane_entry(program, $function)
            {
                return exit(ip);
            }
        }
        // SAFETY: the program's views outlive the lane run.
        let views = unsafe { &*(*cx).views };
        let stack = &mut vm.lane_stack;
        if let Some(lane_view) = views.get($function as usize).and_then(super::program_store::derived_lane_view)
            && let Some(shape) = lane_view.activation
            && usize::from(shape.registers)
                .saturating_mul(std::mem::size_of::<Value>())
                <= crate::stack::GuestCallGuard::remaining_storage_bytes()
                    .saturating_sub(stack.wasm_storage_bytes)
            && stack.activations.len() < stack.activation_limit.get()
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
                    function: $function,
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
        resume!(cx, lane_call_frame::<H>(ip, $function))
    }};
}

lane_handler! {
    /// A Wasm call to a function of the same program, pushed as the general
    /// loop's inline call would be.
    fn lane_call_known<>(cx, r, ip, i, view) {
        lane_wasm_call!(cx, r, ip, i, view, u32::from(i.b))
    }
}

lane_handler! {
    /// A Wasm call through a function reference (`call_indirect`, `call_ref`): when the callee
    /// is a function of the running instance, the activation `lane_call_known` pushes. Any
    /// other callee, and a tail call (`return_call_indirect`, `return_call_ref`), whose
    /// frame the general path reuses, take the general path.
    fn lane_wasm_call_value<>(cx, r, ip, i, view) {
        if i.a & crate::bytecode::RETURN_REGISTER != 0 {
            return exit(ip);
        }
        let Some(function) = lane_instance_function(cx, r.get(i.b)) else {
            return exit(ip);
        };
        lane_wasm_call!(cx, r, ip, i, view, function)
    }
}

/// The index of `callee` when it is a function of the running Wasm instance: the running
/// program's, closed over the frame's instance environment.
#[inline(always)]
fn lane_instance_function<H: Host>(cx: *mut LaneContext<H>, callee: Value) -> Option<u32> {
    let vm = unsafe { vm(cx) };
    let record = vm.heap.function(callee)?;
    let FunctionKind::User(program, function) = record.kind else {
        return None;
    };
    (program == unsafe { (*cx).program } && record.env == vm.frames[unsafe { (*cx).frame }].env).then_some(function)
}


/// The handler of `CallKnown`, a direct Wasm call (`lane_call_known`).
pub(super) const fn call_known_handler<H: Host>() -> Handler {
    lane_call_known::<H>
}

/// The handler of a Wasm view's `Call`, a call through a function reference, or of its
/// `StoreCapture`: the script handlers of both read the context a JavaScript view carries
/// where a Wasm view carries its memory, and Wasm stores a capture only for the rare
/// `data.drop` and `elem.drop`, which the general path makes.
pub(super) const fn wasm_view_handler<H: Host>(op: Op) -> Option<LaneHandler> {
    if op.is_ordinary_call() {
        Some(LaneHandler { handler: lane_wasm_call_value::<H>, produces: false })
    } else {
        None
    }
}
