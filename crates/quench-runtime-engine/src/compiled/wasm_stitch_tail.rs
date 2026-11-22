//! The tail calls of the b-jit's Wasm functions: a `return_call` (`Call::Known`) and a
//! `return_call_indirect` (`Call::Reference`) whose destination carries `RETURN_REGISTER`.
//!
//! The u-jit makes true tail calls (`wasm_call_stage.rs`: `return_call` / `return_call_indirect`
//! to a callee compiled natively, with the same result). Its other tail-call shapes are an ordinary
//! call that writes the result, then the return of that result. `wasm.rs`'s `emit_wasm_call`
//! marks the path dead without emitting a return, for both tiers. This row has no jump: the b-jit's
//! frame is a stencil's. It mirrors the ordinary call (`CALL` or `CALL_INDIRECT`, which write the
//! callee's result to the destination register), then the leave routine that returns that
//! register, as `Flow::Return` does (`leave_wasm_word` / `leave_wasm_void`,
//! `quench-bjit/stencils/wasm_entry.rs`). So the b-jit's native stack grows with each tail call.
//! No new stencil is needed.
//!
//! The leave is required, not an optimization: after a flagged call the bytecode's next
//! instruction is not necessarily a return. A `return_call` inside a nested block is followed by
//! the code after the block (`wasm/control.rs` emits no jump for a dead path), so falling through
//! would run that code. The explicit leave makes the tail call return from the function.
//!
//! Known deviation, b-jit only: the interpreter's Wasm tail call is a true tail call (`dispatch.rs`,
//! the `Op::CallKnown` arm's `prepare_user_tail` for `ProgramKind::Wasm && returns_from_frame()`),
//! so an unbounded tail-recursive loop runs in constant VM stack there. A stitched function grows
//! the native stack per tail call and reaches the entry's stack limit (`CallStackExhausted`). The
//! u-jit has no such growth for a call to a natively compiled callee (`return_call`). A true jump
//! here would tear the frame down and enter the callee's `WasmEntry` (its arguments copied out of
//! the frame first); that needs a new naked stencil and was not attempted here, since nothing could
//! be built or run.
//!
//! A child of `wasm_stitch` (`#[path]`), so it names `wasm_stitch`'s private items
//! through `super`.

use super::{Planned, World, known_call, slot};
use quench_runtime::bytecode::{ImmediateLayout, RETURN_REGISTER, WideInstruction};
use quench_runtime::staged::wasm_call::Call;
use quench_runtime::staged::wasm_step::Step;
use quench_bjit::padded_holes;

/// Whether `instruction` is a call whose destination is flagged as a return.
fn is_tail(instruction: WideInstruction) -> bool {
    instruction.result_flags() & RETURN_REGISTER != 0
}

/// The rows of the tail call at `pc` to a known function (`Call::Known`, flagged): the direct call
/// (`known_call`: `CALL` through the callee's cell, or `CALL_VM` where it is not stitched; the
/// result to the destination register), then the leave that returns that register. `None` where
/// `instruction` is not such a call, or the call has no row (`known_call`), as the plain call's.
///
/// `pc` is unused: a known tail call's rows depend on the instruction alone. The other tail
/// shape, `Call::Reference`, needs the instruction list (its target check), so it goes through
/// `indirect::reference` and `leave` at the plan arm.
pub(crate) fn tail(_pc: usize, instruction: WideInstruction, returns: bool, world: &World<'_>) -> Option<Vec<Planned>> {
    if !matches!(Step::of(instruction.op(), instruction.imm()), Some(Step::Call(Call::Known))) || !is_tail(instruction) {
        return None;
    }
    let window = ImmediateLayout::call_window_base(instruction.imm());
    let call = known_call(instruction.result_register(), window, usize::from(instruction.known_function_index()), world)?;
    Some(vec![call, leave(instruction, returns)])
}

/// The leave that ends a tail call: the same routine `Flow::Return` places for the function's
/// `returns`, reading the call's destination register (`leave_wasm_word`, its frame word; or
/// `leave_wasm_void`). Falls through nowhere: `taken` is `None` and the leave ends the function.
pub(crate) fn leave(instruction: WideInstruction, returns: bool) -> Planned {
    if returns {
        Planned { stencil: "leave_wasm_word", holes: padded_holes([slot(instruction.result_register())]), taken: None }
    } else {
        Planned { stencil: "leave_wasm_void", holes: padded_holes([]), taken: None }
    }
}
