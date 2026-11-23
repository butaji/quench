//! Wasm GC operations the lane makes in place with the interpreter's own step for them
//! (`Vm::wasm_gc_value_step`), rather than leaving for the general loop at each.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::dispatch_wasm::{wasm_gc_allocates, wasm_gc_reads_instance};
use quench_runtime::staged::wasm_call::Call;

/// The running program's signature pool, cached on the lane for the program it was found for
/// (`LaneContext::signatures`). `None` where the program has none; the general path raises that.
#[inline(always)]
fn lane_signatures(cx: *mut LaneContext) -> Option<*const quench_runtime::wasm::WasmSignatures> {
    // SAFETY: the context outlives the lane run; the cache's pool is kept by the context until
    // the program changes, and no step in a lane target's check changes the program.
    unsafe {
        let program = (*cx).program;
        if let (cached, Some(pool)) = &(*cx).signatures {
            if *cached == program {
                return Some(std::rc::Rc::as_ptr(pool));
            }
        }
        let pool = vm(cx).programs.wasm_signatures_shared(program)?;
        let pointer = std::rc::Rc::as_ptr(&pool);
        (*cx).signatures = (program, Some(pool));
        Some(pointer)
    }
}

/// The lane's indirect target step (`Vm::wasm_indirect_row_with`, the interpreter's own check):
/// the reference it writes to its result register, and the row it matched, which the reference
/// call right after the target reads its callee from (`IndirectSlot`).
#[inline(always)]
fn lane_indirect_target(cx: *mut LaneContext, vm: &Vm, i: WideInstruction, ip: Ip, r: Registers) -> Option<Value> {
    // SAFETY: `lane_signatures` returned a pool the context holds for this step.
    let pool = unsafe { lane_signatures(cx)?.as_ref() };
    let (value, row) = vm.wasm_indirect_row_with(pool, r.get(i.register_b()), r.get(i.register_c()), i.imm()).ok()?;
    // SAFETY: the context outlives the lane run.
    unsafe {
        (*cx).indirect = IndirectSlot {
            // Lowering invariant: IndirectTarget is emitted only on the call_indirect path, and
            // `emit_wasm_call` then emits exactly one load_constant (the void receiver, in a register
            // above the target, so it never clobbers register a) followed by the call_through. So the
            // call is two records on. A mismatch (e.g. a fused record) is a safe miss: the reference
            // call's record, register and reference checks decide.
            next: following(following(ip)),
            register: i.result_register(),
            reference: value.to_bits(),
            entry: row.entry,
            instance: row.instance,
        };
    }
    Some(value)
}

lane_handler! {
    /// An instance read of the running function's registers (`Call::IndirectTarget` where `INDIRECT`, else
    /// `Call::RefFunc`), made by the interpreter's step; a trap, which writes nothing, takes the general
    /// path to raise it. The record holds the instruction's operands, so the step needs no decode.
    #[inline(never)]
    fn lane_wasm_gc_step<INDIRECT: bool>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let op = const { (if INDIRECT { Call::IndirectTarget } else { Call::RefFunc }).instance_read_op().expect("an instance read") };
        // SAFETY: the context outlives the lane run.
        let program = unsafe { (*cx).program };
        let env = instance_env_in_lane(cx);
        let instruction = WideInstruction::new(op, i.a, i.b, i.c, i.imm());
        // Every GC step is the interpreter's own (`Vm::wasm_gc_value_step`, inlined, so this handler keeps
        // only its opcode's arm); a trap or miss, which writes nothing, takes the general path to raise it. An
        // indirect target also records the row the reference call after it reads (`lane_indirect_target`).
        let value = if INDIRECT {
            match lane_indirect_target(cx, vm, instruction, ip, r) {
                Some(value) => Some(value),
                None => return exit(ip),
            }
        } else {
            let Ok(value) = vm.wasm_gc_value_step(program, env, instruction, |_, register| r.get(register)) else {
                return exit(ip);
            };
            value
        };
        if let Some(value) = value {
            r.set(instruction.result_register(), value);
        }
        // An allocating step can make a collection due.
        if wasm_gc_allocates(op) {
            note_allocation(vm);
        }
        next!(cx, r, following(ip), view)
    }
}

/// The handler of a Wasm GC value opcode the catalog gives no row (`lane_wasm_gc_step`): the indirect
/// target and `ref.func`, which read the running instance. Every other GC value opcode is its staged
/// row (`ROW_HANDLER`), whose call-out the lane makes in place (`wasm_gc_in_lane`).
pub(crate) const fn wasm_gc_handler(op: Op) -> Option<Handler> {
    match Call::of(op) {
        Some(Call::IndirectTarget) => Some(lane_wasm_gc_step::<true>),
        Some(Call::RefFunc) => Some(lane_wasm_gc_step::<false>),
        _ => None,
    }
}

/// The Wasm GC value step `instruction` of the running function's registers `r`, made by the
/// interpreter's step (`Vm::wasm_gc_value_step`, inlined, so a row of one opcode keeps only its arm):
/// the value it defines, or `undefined` where it defines none. `None` on a trap, which writes nothing
/// and leaves to the row's miss, whose general step raises it.
///
/// # Safety
/// `cx` is the running lane's context and `r` its running register file.
#[inline(always)]
pub(crate) unsafe fn wasm_gc_in_lane(cx: *mut LaneContext, r: Registers, instruction: WideInstruction) -> Option<Value> {
    debug_assert!(!wasm_gc_reads_instance(instruction.op()), "an instance read has its own handler");
    let vm = unsafe { vm(cx) };
    // SAFETY: the context outlives the lane run.
    let program = unsafe { (*cx).program };
    let value = vm.wasm_gc_value_step(program, Value::UNDEFINED, instruction, |_, register| r.get(register)).ok()?;
    // An allocating step can make a collection due.
    if wasm_gc_allocates(instruction.op()) {
        note_allocation(vm);
    }
    Some(value.unwrap_or(Value::UNDEFINED))
}
