//! Runtime glue of `wasm_jump`: the decode of the i32 jumps and the branch table from their
//! records, which read the runtime's opcodes and instructions.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Op, WideInstruction};
use crate::wasm::integer::I32BinaryOperator;

/// The decode of a `WasmJump` from an i32 jump record.
pub trait WasmJumpWasmJumpExt: Sized {
    /// The i32 jump `instruction` is, from its record's operands (`a`, `b`, `c`): a comparison's
    /// right operand is `b` (a register, or a signed constant for an immediate form), and an add's
    /// addend is `c`.
    fn decode(instruction: WideInstruction) -> Option<Self>;
}

impl WasmJumpWasmJumpExt for WasmJump {
    fn decode(instruction: WideInstruction) -> Option<Self> {
        let op = instruction.op();
        if let Some(operator) = I32BinaryOperator::from_jump_op(op) {
            return Some(Self::Compare {
                relation: WasmRelation::of(operator)?,
                left: instruction.a(),
                right: WasmRight::Register(instruction.b()),
            });
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
            return Some(Self::Compare {
                relation: WasmRelation::of(operator)?,
                left: instruction.a(),
                right: WasmRight::Constant(i32::from(instruction.b() as i16)),
            });
        }
        Some(match op {
            Op::WasmJumpI32Zero => Self::Test { register: instruction.a(), nonzero: false },
            Op::WasmJumpI32NonZero => Self::Test { register: instruction.a(), nonzero: true },
            Op::WasmI32AddImmediateJumpNonZero => Self::AddNonZero {
                destination: instruction.a(),
                source: instruction.b(),
                addend: i32::from(instruction.c() as i16),
            },
            _ => return None,
        })
    }
}

/// The decode of a `BranchTable` from a `WasmBranchTable` record.
pub trait BranchTableWasmJumpExt: Sized {
    /// The table `instruction` is: its selector is `a`, and its immediate counts the cases before
    /// the default (`branch_table_entries`), so the entries are that count plus one.
    fn decode(instruction: WideInstruction) -> Option<Self>;
}

impl BranchTableWasmJumpExt for BranchTable {
    fn decode(instruction: WideInstruction) -> Option<Self> {
        if instruction.op() != Op::WasmBranchTable {
            return None;
        }
        Some(Self { index: instruction.a(), entries: instruction.imm().checked_add(1)? })
    }
}
