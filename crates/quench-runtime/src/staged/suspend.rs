//! The suspensions of an activation (`Await`, `Yield`, `YieldStar`) as staged rows: the row names
//! what suspends and the registers it reads and resumes into, and its step is the frame's
//! (`CallFrame::suspend`). A frame that cannot suspend in place leaves the row unresolved, so the
//! tier exits at its exact PC and the interpreter's own suspension runs it: the lane's
//! `lane_await`, `lane_yield` and `lane_yield_star` (`dispatch_fast::generator_yields`), or the
//! general step's `StepResult::Await` and `StepResult::Yield` (`Vm::step`).
use super::{CallFrame, Operation, Register, Stage, Step};
use crate::bytecode::{Op, WideInstruction};

/// What suspends the activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Suspension {
    /// `await`: the awaited value, converted as PromiseResolve converts it.
    Await,
    /// `yield`: the yielded value, awaited first in an async generator.
    Yield,
    /// `yield*`: delegation to the iterator `iterator` holds (undefined before the first step).
    YieldStar { iterator: Register },
}

impl<W, P> Operation<W, P> {
    /// The suspension row of `instruction` (`decode_flow`); `None` for any other opcode.
    pub(super) fn decode_suspend(instruction: WideInstruction) -> Option<Self> {
        let kind = match instruction.op() {
            Op::Await => Suspension::Await,
            Op::Yield => Suspension::Yield,
            Op::YieldStar => Suspension::YieldStar { iterator: instruction.register_c() },
            _ => return None,
        };
        Some(Self::Suspend { kind, destination: instruction.result_register(), value: instruction.register_b() })
    }
}

/// Suspend the activation at `kind` with the value of `value`, resuming into `destination`,
/// where the frame can; otherwise the interpreter's suspension, by the tier's exit.
#[inline(always)]
pub(crate) fn suspend<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, kind: Suspension, destination: Register,
    value: Register) -> Step<S::Word, F::Control> {
    frame.suspend(stage, kind, destination, value).map_or(Step::Unresolved, Step::Control)
}
