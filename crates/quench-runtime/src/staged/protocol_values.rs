//! The by-name, environment, string and iteration-cleanup opcodes as staged call-out rows (`Operation::Protocol`): each is one
//! definition every tier runs, and its step is the interpreter's own helper, which the host makes
//! (`Vm::call_out` -> `Vm::value_step`). The decode of a step from its record, and the strict-reference
//! test, are in `protocol_values/glue.rs`.
use crate::bytecode::{Atom, Register, RegisterWindow};

/// A by-name, environment, string and iteration-cleanup step the host makes by the interpreter's own definition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueStep {
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
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
