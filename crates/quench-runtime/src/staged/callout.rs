//! The staged rows that call the VM out (`CallOut`, defined with the frame that answers it).

use super::{CallOut, Operation, Register};

impl<W, P> Operation<W, P> {
    /// The VM operation of a row that calls out, and the register it defines, if any; `None`
    /// for a row that never does.
    pub(crate) fn call_out(&self) -> Option<(CallOut, Option<Register>)> {
        Some(match *self {
            Self::PropertyKey { destination, source } => (CallOut::PropertyKey { source }, Some(destination)),
            Self::Equality { destination, left, right, strict, negated } =>
                (CallOut::Equality { left, right, strict, negated }, Some(destination)),
            Self::Protocol { step, destination, operands } => (CallOut::Protocol { step, operands }, destination),
            Self::ForInNext { destination, state } => (CallOut::ForInNext { state }, Some(destination)),
            _ => return None,
        })
    }
}
