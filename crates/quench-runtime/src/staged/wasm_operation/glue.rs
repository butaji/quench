//! Runtime glue of `wasm_operation`: the reads of the operation table from an opcode and its
//! record. `Form::left`/`right` and `WasmOperation::reads` take the record; `of` and `is_raw` are
//! `const fn`s of the opcode (free functions, since a const fn cannot be a trait method).
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::float::{F32BinaryOperator, F32UnaryOperator};
use crate::wasm::integer::I32UnaryOperator;

/// The operand positions of a `Form` in its record.
pub trait FormWasmOperationExt: Sized {
    /// The register the left operand is read from.
    fn left(self, instruction: WideInstruction) -> u16;
    /// The right operand: a register, or the constant the form carries.
    fn right(self, instruction: WideInstruction) -> Operand;
}

impl FormWasmOperationExt for Form {
    fn left(self, instruction: WideInstruction) -> u16 {
        match self {
            Self::Register | Self::Immediate | Self::Short => instruction.b(),
            Self::Branch | Self::BranchImmediate => instruction.a(),
        }
    }

    fn right(self, instruction: WideInstruction) -> Operand {
        match self {
            Self::Register => Operand::Register(instruction.c()),
            Self::Immediate => Operand::Constant(i64::from(instruction.imm() as i32)),
            Self::Short => Operand::Constant(i64::from(instruction.c() as i16)),
            Self::Branch => Operand::Register(instruction.b()),
            Self::BranchImmediate => Operand::Constant(i64::from(instruction.b() as i16)),
        }
    }
}

/// The registers an operation reads from its record.
pub trait WasmOperationWasmOperationExt: Sized {
    /// The registers it reads: the left operand, and the right where that is a register.
    fn reads(self, instruction: WideInstruction) -> (u16, Option<u16>);
}

impl WasmOperationWasmOperationExt for WasmOperation {
    fn reads(self, instruction: WideInstruction) -> (u16, Option<u16>) {
        match self {
            Self::Binary { form, .. } => match form.right(instruction) {
                Operand::Register(register) => (form.left(instruction), Some(register)),
                Operand::Constant(_) => (form.left(instruction), None),
            },
            Self::FloatBinary { .. } => (instruction.b(), Some(instruction.c())),
            Self::Test { .. } => (instruction.a(), None),
            _ => (instruction.b(), None),
        }
    }
}

/// Whether `op` is a raw form: its operands are the registers' words as they are, with no
/// tag checks.
pub const fn wasm_operation_is_raw(op: Op) -> bool {
    matches!(
        op.class(),
        Class::WasmI64BinaryRaw
            | Class::WasmI64BinaryImmediateRaw
            | Class::WasmI64UnaryRaw
            | Class::WasmFloatBinary64Raw
            | Class::WasmFloatUnary64Raw
    )
}

/// The integer operation `op` is, with `tag` its operator selector where the opcode takes one.
pub const fn wasm_operation_of(op: Op, tag: u32) -> Option<WasmOperation> {
    if let Some(operator) = I32BinaryOperator::from_register_op(op) {
        return Some(i32_binary(operator, Form::Register));
    }
    if let Some(operator) = I32BinaryOperator::from_immediate_op(op) {
        return Some(i32_binary(operator, Form::Immediate));
    }
    if let Some(operator) = I32BinaryOperator::from_jump_op(op) {
        return Some(i32_binary(operator, Form::Branch));
    }
    if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
        return Some(i32_binary(operator, Form::BranchImmediate));
    }
    if let Some(selected) = wasm_operation_selected(op.class(), tag) {
        return Some(selected);
    }
    wasm_operation_floating(op.class(), tag)
}

/// The float and conversion operations, whose operator is a selector operand.
const fn wasm_operation_floating(class: Class, tag: u32) -> Option<WasmOperation> {
    match class {
        Class::WasmFloatBinary32 => match F32BinaryOperator::from_tag(tag) {
            Some(operator) => Some(WasmOperation::FloatBinary {
                operator: operator.widen(),
                float: Float::F32,
            }),
            None => None,
        },
        Class::WasmFloatBinary64 | Class::WasmFloatBinary64Raw => match FloatBinary::from_tag(tag) {
            Some(operator) => Some(WasmOperation::FloatBinary {
                operator,
                float: Float::F64,
            }),
            None => None,
        },
        Class::WasmFloatUnary32 => match F32UnaryOperator::from_tag(tag) {
            Some(operator) => Some(WasmOperation::FloatUnary {
                operator: operator.widen(),
                float: Float::F32,
            }),
            None => None,
        },
        Class::WasmFloatUnary64 | Class::WasmFloatUnary64Raw => match FloatUnary::from_tag(tag) {
            Some(operator) => Some(WasmOperation::FloatUnary {
                operator,
                float: Float::F64,
            }),
            None => None,
        },
        Class::WasmScalarConvert => match Conversion::from_tag(tag) {
            Some(operator) => Some(WasmOperation::Convert { operator }),
            None => None,
        },
        _ => None,
    }
}

/// The integer operations whose operator is a selector operand.
const fn wasm_operation_selected(class: Class, tag: u32) -> Option<WasmOperation> {
    match class {
        Class::WasmI64Binary | Class::WasmI64BinaryRaw => match Binary::from_tag(tag) {
            Some(operator) => Some(WasmOperation::Binary {
                operator,
                width: Width::I64,
                form: Form::Register,
            }),
            None => None,
        },
        Class::WasmI64BinaryImmediate | Class::WasmI64BinaryImmediateRaw => match Binary::from_tag(tag) {
            Some(operator) => Some(WasmOperation::Binary {
                operator,
                width: Width::I64,
                form: Form::Short,
            }),
            None => None,
        },
        Class::WasmI64Unary | Class::WasmI64UnaryRaw => match Unary::from_tag(tag) {
            Some(operator) => Some(WasmOperation::Unary {
                operator,
                width: Width::I64,
            }),
            None => None,
        },
        Class::WasmI32Unary => match I32UnaryOperator::from_tag(tag) {
            Some(operator) => Some(WasmOperation::Unary {
                operator: operator.widen(),
                width: Width::I32,
            }),
            None => None,
        },
        Class::WasmI32ShiftRightUnsignedAnd => Some(WasmOperation::BitField),
        Class::WasmI32AddJump => Some(WasmOperation::AddBranch),
        Class::WasmJumpI32NonZero => Some(WasmOperation::Test { nonzero: true }),
        Class::WasmJumpI32Zero => Some(WasmOperation::Test { nonzero: false }),
        _ => None,
    }
}
