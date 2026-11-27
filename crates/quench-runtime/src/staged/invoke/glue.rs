//! Runtime glue of `invoke`: the general call form of a call-family opcode (`general_call_form`),
//! which names the runtime's opcode type.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::Op;

#[cfg(not(stencil_build))]
/// The general form of the call-family opcode `op`, else `None`: the records the call runner answers
/// (`vm::dispatch`'s `Runner::Call`).
pub const fn general_call_form(op: Op) -> Option<CallForm> {
    Some(match op {
        Op::Call => CallForm::Plain,
        Op::CallForwardingArguments => CallForm::Forwarding,
        Op::CallArray => CallForm::Array,
        Op::CallSpread => CallForm::Spread,
        Op::CallKnown => CallForm::Known,
        Op::CallMethod => CallForm::Method,
        Op::CallThisMethod => CallForm::ThisMethod,
        Op::Construct => CallForm::Construct,
        Op::CallFirstChunk => CallForm::FirstChunk,
        _ => return None,
    })
}
