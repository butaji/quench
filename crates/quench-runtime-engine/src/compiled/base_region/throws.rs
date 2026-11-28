//! A `throw` a handler of its own function covers, as the catch's jump in native code: the thrown
//! value goes to the handler's register and control to the handler's block, which is what
//! `Vm::exception_handler_target` does for a thrown value, so the interpreter never re-executes
//! the `throw` to find the handler. A `throw` no handler takes this way is `steps::lowering`'s
//! raising step.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::Function;
use quench_runtime::staged::{DecodeContext, HandlerEdge, Operation};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The row of the plain `throw` at `pc` whose handler the function's code can jump to, the edge `Operation::handler_edge`
/// names: a catch that binds nothing is the jump; one that binds is the `Step::Catch` row, which moves the thrown value
/// first. `None` for any other `throw`, which raises (`Step::Throw`).
pub(crate) fn caught(function: &Function, (pc, registers): (u32, Register), instruction: WideInstruction) -> Option<Decoded> {
    if instruction.result_flags() != 0 {
        return None;
    }
    Some(match Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY)?.handler_edge(function, pc, registers)? {
        HandlerEdge::Jump { target } => Decoded::number(Kind::Jump, None, None, None, Some(target)),
        HandlerEdge::Bind { slot, target } => Decoded { a: Some(Source::Register(instruction.register_a())), target: Some(target), ..Decoded::step(Step::Catch, Some(slot)) },
    })
}
