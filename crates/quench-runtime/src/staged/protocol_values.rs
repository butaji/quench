//! The by-name, environment, string and iteration-cleanup opcodes as staged call-out rows (`Operation::Protocol`): each is one
//! definition every tier runs, and its step is the interpreter's own helper, which the host makes
//! (`Vm::call_out` -> `Vm::value_step`).
use super::Protocol;
use crate::bytecode::{Atom, Register, RegisterWindow};
#[cfg(not(stencil_build))]
use crate::bytecode::{InstructionField, Op, WideInstruction};

/// A by-name, environment, string and iteration-cleanup step the host makes by the interpreter's own definition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ValueStep {
    /// The resolved reference of the name `atom` (`ResolveName`); `strict` is its flag.
    ResolveName { atom: Atom, strict: bool },
    /// Delete the name `atom` (`DeleteName`), defining the deletion's result.
    DeleteName { atom: Atom },
    /// Push the iterator in register `iterator` and its done flag in register `done` onto the running frame's cleanup stack
    /// (`IteratorCleanupPush`); defines nothing.
    IteratorCleanupPush { iterator: Register, done: Register },
    /// Pop the running frame's innermost iterator cleanup record (`IteratorCleanupPop`); defines nothing.
    IteratorCleanupPop,
    /// The value of the name `atom` of the resolved reference in the second operand (`LoadResolvedName`).
    LoadResolvedName { atom: Atom, strict: bool },
    /// Store the first operand to the name `atom` of the resolved reference in the second operand (`StoreResolvedName`).
    StoreResolvedName { atom: Atom, strict: bool },
    /// Copy the running frame's environment under environment clone plan `plan` (`CloneEnv`); defines nothing.
    CloneEnv { plan: usize },
    /// The async iterator of the second operand (`GetAsyncIterator`).
    GetAsyncIterator,
    /// The second operand, an iterator's next result, required to be an object (`RequireIteratorResult`); defines nothing.
    RequireIteratorResult,
    /// The string of the running frame's register `window` (`StringConcat`), read from the frame itself.
    StringConcat { window: RegisterWindow },
    /// The case among `count` string constants from `start` the second operand selects (`StringCaseIndex`).
    StringCaseIndex { start: usize, count: u16 },
}

#[cfg(not(stencil_build))]
/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub(super) fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
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

#[cfg(not(stencil_build))]
/// Whether `instruction` is a step of a strict code's by-name reference to a global (`ResolveName` and
/// `StoreResolvedName` with their strict flags, `x = e` of an undeclared `x` in strict code): a call-out
/// whose own step a loop region makes in place (`Vm::call_out`), as it does a sloppy store's.
pub fn strict_reference_step(instruction: WideInstruction) -> bool {
    matches!(decode(instruction), Some((Protocol::Value(ValueStep::ResolveName { strict: true, .. } | ValueStep::StoreResolvedName { strict: true, .. }), ..)))
}
