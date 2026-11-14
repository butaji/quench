//! Wasm GC operations the lane makes in place with the interpreter's own step for them
//! (`Vm::wasm_gc_value_step`), rather than leaving for the general loop at each.
use super::*;
use super::super::dispatch_wasm::{wasm_gc_allocates, wasm_gc_field_op, wasm_gc_reads_instance};

/// The `wasm_gc_value_op` numbered `OP`.
const fn gc_op<const OP: u16>() -> Op {
    Op::from_index(OP as usize).expect("a Wasm GC opcode")
}

lane_handler! {
    /// The `wasm_gc_value_op` numbered `OP` of the running function's registers, made by the
    /// interpreter's step; a trap, which writes nothing, takes the general path to raise it.
    /// One handler per opcode: the record holds the instruction's operands, so the step needs
    /// no decode, and the opcode is a constant its step folds.
    #[inline(never)]
    fn lane_wasm_gc_step<OP: u16>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let op = const { gc_op::<OP>() };
        // SAFETY: the context outlives the lane run.
        let (program, frame) = unsafe { ((*cx).program, (*cx).frame) };
        let env = if const { wasm_gc_reads_instance(gc_op::<OP>()) } { vm.frames[frame].env } else { Value::UNDEFINED };
        let instruction = WideInstruction::new(op, i.a, i.b, i.c, i.imm());
        // A field access allocates nothing and reads no instance state: a miss, which traps,
        // takes the general path, which raises it.
        if const { wasm_gc_field_op(gc_op::<OP>()) } {
            let Ok(Some(value)) = vm.wasm_gc_field_hit(instruction, |_, register| r.get(register)) else {
                return exit(ip);
            };
            if let Some(value) = value {
                r.set(instruction.result_register(), value);
            }
            next!(cx, r, following(ip), view)
        }
        let Ok(value) = vm.wasm_gc_value_step(program, env, instruction, |_, register| r.get(register)) else {
            return exit(ip);
        };
        if let Some(value) = value {
            r.set(instruction.result_register(), value);
        }
        // An allocating step can make a collection due.
        if const { wasm_gc_allocates(gc_op::<OP>()) } {
            note_allocation(vm);
        }
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
