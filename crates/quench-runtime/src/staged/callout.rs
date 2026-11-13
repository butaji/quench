//! The VM operations a staged row calls out for when its own words do not decide it, named as
//! data. A frame answers one in place (`Frame::vm_step`): the interpreter's frame calls the VM
//! method, the lane specializer's frame calls the lane helper that decodes the record and makes
//! the same step (`dispatch_fast::lane_vm_step`), and a frame with no host declines.

use super::{NumericFrame, NumericStage, Operation, Protocol, Register, Step};

/// The tag byte of each `CallOut`, which a helper-call row passes as its immediate.
pub(crate) const TAG_EQUALITY: u8 = 0;
pub(crate) const TAG_BINARY: u8 = 1;
pub(crate) const TAG_PROPERTY_KEY: u8 = 2;
pub(crate) const TAG_PROTOCOL: u8 = 3;
pub(crate) const TAG_FOR_IN_NEXT: u8 = 4;
pub(crate) const TAG_LOAD_ENV_LOCAL: u8 = 5;
pub(crate) const TAG_STORE_ENV_LOCAL: u8 = 6;
pub(crate) const TAG_ARGUMENTS_LENGTH: u8 = 7;
pub(crate) const TAG_GLOBAL_WRITE: u8 = 8;
pub(crate) const TAG_GLOBAL_TYPEOF: u8 = 9;

/// A VM operation and the registers and immediates it reads: plain data, whose tag byte
/// (`repr(u8)`) and fields a helper-call row passes as immediates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum CallOut {
    /// `left == right`, or `===` where `strict`, negated for `!=` and `!==`, by the VM's full
    /// equality: strings and BigInts compare by contents, a conversion may run guest code.
    Equality { left: Register, right: Register, strict: bool, negated: bool } = TAG_EQUALITY,
    /// `left operator right` by the VM's `Vm::binary`: a string or BigInt relation or operator,
    /// `instanceof`, `in`.
    Binary { left: Register, right: Register, operator: u32 } = TAG_BINARY,
    /// ToPropertyKey of `source` (`Vm::to_property_key`): a Number allocates, an object runs
    /// guest code.
    PropertyKey { source: Register } = TAG_PROPERTY_KEY,
    /// An iteration or spread step over its operand registers (`Protocol`).
    Protocol { step: Protocol, operands: [Option<Register>; 2] } = TAG_PROTOCOL,
    /// The next key of the for-in state in `state`; `Value::DELETED`, which no key is, once
    /// none is left.
    ForInNext { state: Register } = TAG_FOR_IN_NEXT,
    /// The environment local `slot` of the running frame, by the binding rules.
    LoadEnvLocal { slot: u16 } = TAG_LOAD_ENV_LOCAL,
    /// `source` to the environment local `slot` of the running frame, by the binding rules (a
    /// `with` scope, TDZ, a constant), `initialize` where it declares the binding.
    StoreEnvLocal { source: Register, slot: u16, initialize: bool } = TAG_STORE_ENV_LOCAL,
    /// The length of the arguments binding in local `slot`.
    ArgumentsLength { slot: u16 } = TAG_ARGUMENTS_LENGTH,
    /// `source` to the global name `atom` through cache `site`, `initialize` for a declaration.
    GlobalWrite { source: Register, atom: u32, site: u16, initialize: bool } = TAG_GLOBAL_WRITE,
    /// The `typeof` name of the global `atom` through cache `site`.
    GlobalTypeof { atom: u32, site: u16 } = TAG_GLOBAL_TYPEOF,
}

impl CallOut {
    /// Whether the step reads or writes the locals of the running function's own frame.
    pub(crate) fn reads_frame_locals(self) -> bool {
        matches!(self, Self::LoadEnvLocal { .. } | Self::StoreEnvLocal { .. } | Self::ArgumentsLength { .. })
    }

    /// The registers the VM reads, in the order its operation takes them: the first operand
    /// then the second, which is the only one an operation of a single operand has.
    pub(crate) fn operands(self) -> [Option<Register>; 2] {
        match self {
            Self::Equality { left, right, .. } | Self::Binary { left, right, .. } => [Some(left), Some(right)],
            Self::PropertyKey { source: operand } | Self::ForInNext { state: operand } => [None, Some(operand)],
            Self::StoreEnvLocal { source: operand, .. } | Self::GlobalWrite { source: operand, .. } => [Some(operand), None],
            Self::LoadEnvLocal { .. } | Self::ArgumentsLength { .. } | Self::GlobalTypeof { .. } => [None, None],
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
