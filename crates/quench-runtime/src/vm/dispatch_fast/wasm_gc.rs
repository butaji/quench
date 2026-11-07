//! Wasm GC operations the lane makes in place with the interpreter's own step for them
//! (`Vm::wasm_gc_value_step`), rather than leaving for the general loop at each.
use super::*;

lane_handler! {
    /// A `wasm_gc_value_op` of the running function's registers, made by the interpreter's
    /// step; a trap, which writes nothing, takes the general path to raise it.
    #[inline(never)]
    fn lane_wasm_gc_step<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the context's program outlives the lane run, and `ip` is a record of the
        // running function's view.
        let (code, program, frame, pc) =
            unsafe { (&*(*cx).code, (*cx).program, (*cx).frame, ip.offset_from((*cx).base) as usize) };
        let function = vm.lane_stack.activations.last()
            .map_or_else(|| vm.frames[frame].function, |activation| activation.function);
        let body = code.function(function as usize);
        let instruction = decoded_instruction(body, body.code[pc]);
        let Ok(value) = vm.wasm_gc_value_step(program, instruction, |_, register| r.get(register)) else {
            return exit(ip);
        };
        if let Some(value) = value {
            r.set(instruction.result_register(), value);
        }
        // The step may allocate, which can make a collection due.
        note_allocation(vm);
        next!(cx, r, following(ip), view)
    }
}

/// The handler of a `wasm_gc_value_op` (`lane_wasm_gc_step`), for `op` if it is one.
pub(super) const fn wasm_gc_handler<H: Host>(op: Op) -> Option<Handler> {
    if super::super::dispatch_wasm::wasm_gc_value_op(op) { Some(lane_wasm_gc_step::<H>) } else { None }
}
