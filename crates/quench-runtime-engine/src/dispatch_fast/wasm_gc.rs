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
fn signatures_in_lane(cx: *mut LaneContext) -> Option<*const quench_runtime::wasm::WasmSignatures> {
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
/// call at `next` reads its callee from (`IndirectSlot`).
#[inline(always)]
fn indirect_target_in_lane(cx: *mut LaneContext, vm: &Vm, i: WideInstruction, next: Ip, r: Registers) -> Option<Value> {
    // SAFETY: `signatures_in_lane` returned a pool the context holds for this step.
    let pool = unsafe { signatures_in_lane(cx)?.as_ref() };
    let (value, row) = vm.wasm_indirect_row_with(pool, r.get(i.register_b()), r.get(i.register_c()), i.imm()).ok()?;
    // SAFETY: the context outlives the lane run.
    unsafe {
        (*cx).indirect = IndirectSlot {
            next,
            register: i.result_register(),
            reference: value.to_bits(),
            entry: row.entry,
            instance: row.instance,
        };
    }
    Some(value)
}

/// The Wasm GC value step `instruction` of the running function's registers `r`, made by the
/// interpreter's step (`Vm::wasm_gc_value_step`, inlined, so a row of one opcode keeps only its arm):
/// the value it defines, or `undefined` where it defines none; `OP` is the row's opcode. An instance read names a function of
/// the running instance (`instance_env_in_lane`); an indirect target also records the row it matched
/// for the reference call at `next` (`indirect_target_in_lane`). `None` on a trap, which writes
/// nothing and leaves to the row's miss, whose general step raises it.
///
/// # Safety
/// `cx` is the running lane's context and `r` its running register file.
#[inline(always)]
pub(crate) unsafe fn wasm_gc_in_lane<const OP: u16>(cx: *mut LaneContext, r: Registers, instruction: WideInstruction, next: Ip) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // The row's opcode, a constant of the instantiation, so the step keeps only its arm.
    let op = const { opcode(OP) };
    let instruction = WideInstruction::new(op, instruction.a(), instruction.b(), instruction.c(), instruction.imm());
    let value = if const { matches!(Call::of(opcode(OP)), Some(Call::IndirectTarget)) } {
        Some(indirect_target_in_lane(cx, vm, instruction, next, r)?)
    } else {
        // SAFETY: the context outlives the lane run.
        let program = unsafe { (*cx).program };
        let env = if const { wasm_gc_reads_instance(opcode(OP)) } { instance_env_in_lane(cx) } else { Value::UNDEFINED };
        vm.wasm_gc_value_step(program, env, instruction, |_, register| r.get(register)).ok()?
    };
    // An allocating step can make a collection due.
    if const { wasm_gc_allocates(opcode(OP)) } {
        note_allocation(vm);
    }
    Some(value.unwrap_or(Value::UNDEFINED))
}

/// The opcode of a row's constant `op` index. A chain's absent link (`staged::NO_LINK`) instantiates
/// the row too, never to run it, and stands for its placeholder opcode.
const fn opcode(op: u16) -> Op {
    match Op::from_index(op as usize) {
        Some(op) => op,
        None if op == quench_runtime::staged::NO_LINK => quench_runtime::staged::NO_LINK_OP,
        None => panic!("no such opcode"),
    }
}
