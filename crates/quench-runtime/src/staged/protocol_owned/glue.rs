//! Runtime glue of `protocol_owned`: the decode of an activation-variable step from its record.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{InstructionField, Op, WideInstruction};
use crate::staged::Protocol;

/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
    let (step, destination, operands) = match instruction.op() {
        Op::LoadLocal => (
            OwnedVarStep::LoadLocal { slot: instruction.local_slot() },
            Some(instruction.result_register()),
            [None, None],
        ),
        Op::StoreVarBinding => (
            OwnedVarStep::StoreVarBinding { slot: instruction.local_slot() },
            None,
            [Some(instruction.register_a()), None],
        ),
        Op::StoreLocal => (
            OwnedVarStep::StoreLocal {
                slot: instruction.local_slot(),
                initialize: instruction.boolean_field(InstructionField::C),
            },
            instruction.optional_register_b(),
            [Some(instruction.register_a()), None],
        ),
        Op::InitializeTdz => (OwnedVarStep::InitializeTdz { slot: instruction.local_slot() }, None, [None, None]),
        Op::ArgumentGet => (
            OwnedVarStep::ArgumentGet { slot: instruction.local_slot() },
            Some(instruction.result_register()),
            [None, Some(instruction.register_b())],
        ),
        _ => return None,
    };
    Some((Protocol::Owned(step), destination, operands))
}
