//! Wasm GC operations the lane makes in place with the interpreter's own step for them
//! (`Vm::wasm_gc_value_step`), rather than leaving for the general loop at each.
use super::*;

lane_handler! {
    /// The `wasm_gc_value_op` numbered `OP` of the running function's registers, made by the
    /// interpreter's step; a trap, which writes nothing, takes the general path to raise it.
    /// One handler per opcode: the record holds the instruction's operands, so the step needs
    /// no decode, and the opcode is a constant its step folds.
    #[inline(never)]
    fn lane_wasm_gc_step<OP: u16>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        // SAFETY: the context outlives the lane run.
        let program = unsafe { (*cx).program };
        let op = const { Op::from_index(OP as usize).expect("a Wasm GC opcode") };
        let instruction = WideInstruction::new(op, i.a, i.b, i.c, i.imm());
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
    macro_rules! handlers {
        ($($name:ident)*) => { match op { $(Op::$name => Some(lane_wasm_gc_step::<H, { Op::$name as u16 }>),)* _ => None } };
    }
    super::super::dispatch_wasm::wasm_gc_value_ops!(handlers)
}
