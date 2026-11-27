//! The calls whose arguments the VM gathers, as call-out rows: a call with a spread argument
//! (`CallSpread`, `CallArray`) and `super(...)`. Their arguments are an iterable to spread (guest
//! code), an array to apply, or registers a construction passes; callee, receiver and argument
//! registers are operands (`CallOut::operands`), and the VM's call or construction (`Vm::call_out`)
//! is the one definition every tier runs.

use super::callout::calling;
use super::{NumericFrame, NumericStage, Operation, Step};
#[cfg(not(stencil_build))]
use super::Arguments;
#[cfg(not(stencil_build))]
use crate::bytecode::{ConstructArguments, Op, WideInstruction};

/// What the argument registers of a call-out call hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Passing {
    /// The values to pass, in the registers of the window.
    Values,
    /// One register holding an array whose elements are the arguments.
    Array,
    /// One register holding an iterable, whose values are spread into the arguments.
    Iterable,
    /// One register holding a rest parameter kept as a view (`CallRest`): its array, spread as
    /// `Iterable` spreads one, or `undefined`, for the values the activation kept past its fixed
    /// parameters.
    Rest,
}


/// Run a spread call or a super construction: the VM's own, defining `destination`.
#[inline(always)]
pub fn step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation.call_out() {
        Some((call, destination)) => calling(stage, frame, call, destination),
        None => Step::Unresolved,
    }
}

/// `step_of` for any operation.
#[inline(always)]
pub fn step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;

#[cfg(test)]
mod tests;
