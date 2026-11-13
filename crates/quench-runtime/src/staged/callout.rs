//! The staged rows that call the VM out (`CallOut`, defined with the frame that answers it).

use super::{CallOut, NumericFrame, NumericStage, Operation, Register, Step};

impl<W, P> Operation<W, P> {
    /// The VM operation of a row that calls out, and the register it defines, if any; `None`
    /// for a row that never does.
    pub(crate) fn call_out(&self) -> Option<(CallOut, Option<Register>)> {
        Some(match *self {
            Self::PropertyKey { destination, source } => (CallOut::PropertyKey { source }, Some(destination)),
            Self::Equality { destination, left, right, strict, negated } =>
                (CallOut::Equality { left, right, strict, negated }, Some(destination)),
            Self::Binary { destination, left, right, operator } =>
                (CallOut::Binary { left, right, operator }, Some(destination)),
            Self::Protocol { step, destination, operands } => (CallOut::Protocol { step, operands }, destination),
            Self::ForInNext { destination, state } => (CallOut::ForInNext { state }, Some(destination)),
            Self::LoadEnvLocal { destination, slot } => (CallOut::LoadEnvLocal { slot }, Some(destination)),
            Self::StoreEnvLocal { source, slot, result, initialize } =>
                (CallOut::StoreEnvLocal { source, slot, initialize }, result),
            Self::ArgumentsLength { destination, slot } => (CallOut::ArgumentsLength { slot }, Some(destination)),
            Self::GlobalWrite { source, atom, site, initialize } =>
                (CallOut::GlobalWrite { source, atom, site, initialize }, None),
            Self::GlobalTypeof { destination, atom, site } => (CallOut::GlobalTypeof { atom, site }, Some(destination)),
            _ => return None,
        })
    }
}

/// A row whose operation is the VM's: the frame makes it, and the word it defines, if any, is
/// `destination`'s; a frame with no host leaves it to the interpreter.
#[inline(always)]
pub(super) fn calling<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, call: CallOut,
    destination: Option<Register>) -> Step<S::Word, F::Control> {
    match frame.vm_step(stage, call, destination) {
        Some((value, fact)) => {
            if let Some(destination) = destination { frame.write(stage, destination, value, fact); }
            Step::Value(value)
        }
        None => Step::Unresolved,
    }
}
