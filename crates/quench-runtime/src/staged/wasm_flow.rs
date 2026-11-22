//! How control moves through a Wasm function, as data: which opcode jumps, dispatches on a
//! table, selects a value, traps or returns, and where each leaves to. A tier builds its
//! branches from `Leave`, never from an opcode.

use crate::WasmTrap;
use crate::bytecode::{Class, Op, WideInstruction, branch_table_entries};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    /// Continues at the jump target.
    Jump,
    /// Continues at the entry the selector in `a` picks; the last entry is the default.
    Table,
    /// `a = (condition ? b : c)`, the condition in the register pair.
    Select,
    Unreachable,
    Return,
}

/// Where control continues after an instruction that does not branch on a value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Leave {
    /// The next instruction.
    Next,
    Jump(usize),
    /// The targets of a table's entries, the default last.
    Table(Vec<usize>),
    Trap(WasmTrap),
    Return,
}

impl Flow {
    pub(crate) const fn of(op: Op) -> Option<Self> {
        match op.class() {
            Class::Jump => Some(Self::Jump),
            Class::WasmBranchTable => Some(Self::Table),
            Class::WasmSelect => Some(Self::Select),
            Class::WasmUnreachable => Some(Self::Unreachable),
            Class::Return | Class::WasmReturn64 => Some(Self::Return),
            _ => None,
        }
    }

    /// Where `instruction` at `pc` leaves to; a table's entries are `Jump`s, which `decode`
    /// reads at their positions.
    pub(crate) fn leave(
        self,
        pc: usize,
        instruction: WideInstruction,
        decode: impl Fn(usize) -> WideInstruction,
    ) -> Leave {
        match self {
            Self::Jump => Leave::Jump(instruction.jump_target() as usize),
            Self::Table => Leave::Table(
                branch_table_entries(pc, instruction)
                    .map(|entry| decode(entry).jump_target() as usize)
                    .collect(),
            ),
            Self::Select => Leave::Next,
            Self::Unreachable => Leave::Trap(WasmTrap::Unreachable),
            Self::Return => Leave::Return,
        }
    }
}
