//! The classification of call and construction records the tiers ask of an instruction.
//!
//! A tier that needs to know which call form an instruction is asks here, with the instruction
//! itself, rather than naming the opcode. Each answer reads the catalog's family of the record
//! (`Op::class`, one family per call form), the same column the call rows of `Operation::decode`
//! (`Operation::Call`, `SpreadCall`, `SuperCall`) are built from.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt};
use crate::bytecode::{Class, WideInstruction};
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

/// A call through a callee register and a window of arguments (`Op::Call`, with its eval flags).
#[inline(always)]
pub(crate) fn plain_call(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::Call
}

/// A call whose argument is one materialized array (`Op::CallArray`).
#[inline(always)]
pub(crate) fn array_call(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::CallArray
}

/// A call the frame pushes from its argument window or its argument array: the ordinary call
/// forms the loop can inline (`Operation::Call` and `Operation::SpreadCall` with `Passing::Array`).
#[inline(always)]
pub(crate) fn ordinary_call(instruction: WideInstruction) -> bool {
    plain_call(instruction) || array_call(instruction)
}

/// A call that may forward the caller's `arguments` (`Op::Call`, `Op::CallForwardingArguments`).
#[inline(always)]
pub(crate) fn call_or_forwarding(instruction: WideInstruction) -> bool {
    plain_call(instruction) || instruction.op().class() == Class::CallForwardingArguments
}

/// A construction (`Op::Construct`), plain or super: `Operation::Call` or `Operation::SuperCall`.
#[inline(always)]
pub(crate) fn construction(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::Construct
}

/// A spread call (`Op::CallSpread`): its one argument register holds an iterable to spread.
#[inline(always)]
pub(crate) fn spread_call(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::CallSpread
}

/// A call whose callee is a method looked up on its receiver (`Op::CallMethod`).
#[inline(always)]
pub(crate) fn method_call(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::CallMethod
}

/// A call that forwards the caller's arguments (`Op::CallForwardingArguments`).
#[inline(always)]
pub(crate) fn forwarding_call(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::CallForwardingArguments
}

/// The call-site printer's view of a call-family record: `Some(true)` for a plain construction
/// (not `super(...)`), `Some(false)` for a call of any form that names a callee, `None` for
/// every other record.
#[inline(always)]
pub(crate) fn named_callee_is_construction(instruction: WideInstruction) -> Option<bool> {
    if construction(instruction) {
        return (!instruction.is_super_construct()).then_some(true);
    }
    (plain_call(instruction) || array_call(instruction) || spread_call(instruction)
        || forwarding_call(instruction) || method_call(instruction)).then_some(false)
}
