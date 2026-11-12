//! The VM operations a staged row calls out for when its own words do not decide it, named as
//! data. A frame answers one in place (`Frame::vm_step`): the interpreter's frame calls the VM
//! method, the lane specializer's frame calls the lane helper that decodes the record and makes
//! the same step (`dispatch_fast::lane_vm_step`), and a frame with no host declines.

use super::{Operation, Protocol, Register};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CallOut {
    /// `left == right`, or `===` where `strict`, negated for `!=` and `!==`, by the VM's full
    /// equality: strings and BigInts compare by contents, a conversion may run guest code.
    Equality { left: Register, right: Register, strict: bool, negated: bool },
    /// ToPropertyKey of `source` (`Vm::to_property_key`): a Number allocates, an object runs
    /// guest code.
    PropertyKey { source: Register },
    /// An iteration or spread step over its operand registers (`Protocol`).
    Protocol { step: Protocol, operands: [Option<Register>; 2] },
    /// The next key of the for-in state in `state`; `Value::DELETED`, which no key is, once
    /// none is left.
    ForInNext { state: Register },
}

impl CallOut {
    /// The registers the VM reads, in the order its operation takes them: the first operand
    /// then the second, which is the only one an operation of a single operand has.
    pub(crate) fn operands(self) -> [Option<Register>; 2] {
        match self {
            Self::Equality { left, right, .. } => [Some(left), Some(right)],
            Self::PropertyKey { source: operand } | Self::ForInNext { state: operand } => [None, Some(operand)],
            Self::Protocol { operands, .. } => operands,
        }
    }
}

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
