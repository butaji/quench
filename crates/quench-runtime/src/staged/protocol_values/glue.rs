//! Runtime glue of `protocol_values`: the decode of a by-name step from its record, and the test of a
//! strict code's by-name reference, which both read the runtime's instructions.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{InstructionField, Op, WideInstruction};
use crate::staged::Protocol;

/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
    let (step, destination, operands) = match instruction.op() {
        Op::ResolveName => (
            ValueStep::ResolveName { atom: instruction.atom_index(), strict: instruction.boolean_field(InstructionField::B)? },
            Some(instruction.result_register()),
            [None, None],
        ),
        Op::DeleteName => (ValueStep::DeleteName { atom: instruction.atom_index() }, Some(instruction.result_register()), [None, None]),
        Op::IteratorCleanupPush => (
            ValueStep::IteratorCleanupPush { iterator: instruction.register_a(), done: instruction.register_b() },
            None,
            [None, None],
        ),
        Op::IteratorCleanupPop => (ValueStep::IteratorCleanupPop, None, [None, None]),
        Op::LoadResolvedName => (
            ValueStep::LoadResolvedName { atom: instruction.atom_index(), strict: instruction.boolean_field(InstructionField::C)? },
            Some(instruction.result_register()),
            [None, Some(instruction.register_b())],
        ),
        Op::StoreResolvedName => (
            ValueStep::StoreResolvedName { atom: instruction.atom_index(), strict: instruction.boolean_field(InstructionField::C)? },
            None,
            [Some(instruction.register_a()), Some(instruction.register_b())],
        ),
        Op::CloneEnv => (ValueStep::CloneEnv { plan: instruction.environment_clone_index() }, None, [None, None]),
        Op::GetAsyncIterator => (ValueStep::GetAsyncIterator, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::RequireIteratorResult => (ValueStep::RequireIteratorResult, None, [None, Some(instruction.register_b())]),
        Op::StringConcat => (
            ValueStep::StringConcat { window: instruction.register_window() },
            Some(instruction.result_register()),
            [None, None],
        ),
        Op::StringCaseIndex => (
            ValueStep::StringCaseIndex { start: instruction.constant_index(), count: instruction.element_count() },
            Some(instruction.result_register()),
            [None, Some(instruction.register_c())],
        ),
        _ => return None,
    };
    Some((Protocol::Value(step), destination, operands))
}

/// Whether `instruction` is a step of a strict code's by-name reference to a global (`ResolveName` and
/// `StoreResolvedName` with their strict flags, `x = e` of an undeclared `x` in strict code): a call-out
/// whose own step a loop region makes in place (`Vm::call_out`), as it does a sloppy store's.
pub fn strict_reference_step(instruction: WideInstruction) -> bool {
    matches!(decode(instruction), Some((Protocol::Value(ValueStep::ResolveName { strict: true, .. } | ValueStep::StoreResolvedName { strict: true, .. }), ..)))
}
