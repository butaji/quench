//! A `throw` a handler of its own function covers, as the catch's jump in native code: the thrown
//! value goes to the handler's register and control to the handler's block, which is what
//! `Vm::exception_handler_target` does for a thrown value, so the interpreter never re-executes
//! the `throw` to find the handler. A `throw` no handler takes this way is `steps::lowering`'s
//! raising step.
use super::*;
use quench_runtime::bytecode::{Function, ThrownValueSlot};

/// Whether the exceptions of `function` reach its handlers by the interpreter's unwinding rule
/// alone: a generator, an async function and a dynamic scope have state only its own unwinding
/// settles.
fn unwinds_to_handlers(function: &Function) -> bool {
    !(function.is_generator || function.is_async || function.has_dynamic_scope_access())
}

/// The row of the plain `throw` at `pc` whose handler the function's code can jump to: the handler
/// opens after the `throw` with no `with` scope to unwind, and its binding, if any, is a register of
/// the activation (a local needs the binding rules, which only the interpreter makes). A catch that
/// binds nothing is the jump; one that binds is the `Step::Catch` row, which moves the thrown value
/// first. `None` for any other `throw`, which raises (`Step::Throw`).
pub(super) fn caught(function: &Function, (pc, registers): (u32, Register), instruction: WideInstruction) -> Option<Decoded> {
    if instruction.result_flags() != 0 || instruction.register_a() >= registers || !unwinds_to_handlers(function) {
        return None;
    }
    let handler = function.handler_at(pc).filter(|handler| handler.with_depth == 0 && handler.target > pc)?;
    match handler.slot {
        None => Some(Decoded::number(Kind::Jump, None, None, None, Some(handler.target))),
        Some(ThrownValueSlot::Register(slot)) if slot < registers => {
            Some(Decoded { a: Some(Source::Register(instruction.register_a())), target: Some(handler.target), ..Decoded::step(Step::Catch, Some(slot)) })
        }
        Some(ThrownValueSlot::Register(_) | ThrownValueSlot::Local(_)) => None,
    }
}
