//! The Wasm integer opcodes as data: what an opcode computes, at which width, and where its
//! operands and result go. Every tier reads this table; none names an integer opcode.

use super::wasm::{Width, binary_result, unary_result};
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::integer::{
    I32BinaryOperator, I32UnaryOperator, I64BinaryOperator as Binary, I64UnaryOperator as Unary,
};

/// Where an integer operation reads its operands and what it does with its value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Form {
    /// `a = b op c`.
    Register,
    /// `a = b op imm`.
    Immediate,
    /// Jumps to the target where `a op b` holds.
    Branch,
    /// Jumps to the target where `a op b` holds, `b` being a 16-bit constant.
    BranchImmediate,
}

/// An operand: a register, or a constant of the operation's width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Operand {
    Register(u16),
    Constant(i64),
}

impl Form {
    pub(crate) fn left(self, instruction: WideInstruction) -> u16 {
        match self {
            Self::Register | Self::Immediate => instruction.b(),
            Self::Branch | Self::BranchImmediate => instruction.a(),
        }
    }

    pub(crate) fn right(self, instruction: WideInstruction) -> Operand {
        match self {
            Self::Register => Operand::Register(instruction.c()),
            Self::Immediate => Operand::Constant(i64::from(instruction.imm() as i32)),
            Self::Branch => Operand::Register(instruction.b()),
            Self::BranchImmediate => Operand::Constant(i64::from(instruction.b() as i16)),
        }
    }

    /// Whether the value is a jump condition, not a register.
    pub(crate) const fn branches(self) -> bool {
        matches!(self, Self::Branch | Self::BranchImmediate)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WasmOperation {
    Binary {
        operator: Binary,
        width: Width,
        form: Form,
    },
    Unary {
        operator: Unary,
        width: Width,
    },
}

impl WasmOperation {
    /// The integer operation `op` is, with `tag` its operator selector where the opcode takes one.
    pub(crate) fn of(op: Op, tag: u32) -> Option<Self> {
        let i32_binary = |operator: I32BinaryOperator, form| Self::Binary {
            operator: operator.widen(),
            width: Width::I32,
            form,
        };
        I32BinaryOperator::from_register_op(op)
            .map(|operator| i32_binary(operator, Form::Register))
            .or_else(|| {
                I32BinaryOperator::from_immediate_op(op)
                    .map(|operator| i32_binary(operator, Form::Immediate))
            })
            .or_else(|| {
                I32BinaryOperator::from_jump_op(op)
                    .map(|operator| i32_binary(operator, Form::Branch))
            })
            .or_else(|| {
                I32BinaryOperator::from_immediate_jump_op(op)
                    .map(|operator| i32_binary(operator, Form::BranchImmediate))
            })
            .or_else(|| Self::selected(op.class(), tag))
    }

    /// The operations whose operator is a selector operand.
    fn selected(class: Class, tag: u32) -> Option<Self> {
        match class {
            Class::WasmI64Binary => Binary::from_tag(tag).map(|operator| Self::Binary {
                operator,
                width: Width::I64,
                form: Form::Register,
            }),
            Class::WasmI64Unary => Unary::from_tag(tag).map(|operator| Self::Unary {
                operator,
                width: Width::I64,
            }),
            Class::WasmI32Unary => I32UnaryOperator::from_tag(tag).map(|operator| Self::Unary {
                operator: operator.widen(),
                width: Width::I32,
            }),
            Class::WasmI32ShiftRightUnsignedAnd => Some(Self::BitField),
            Class::WasmI32AddJump => Some(Self::AddBranch),
            Class::WasmJumpI32NonZero => Some(Self::Test { nonzero: true }),
            Class::WasmJumpI32Zero => Some(Self::Test { nonzero: false }),
            _ => None,
        }
    }

    /// The width its operands have.
    pub(crate) const fn width(self) -> Width {
        match self {
            Self::Binary { width, .. } | Self::Unary { width, .. } => width,
            Self::BitField | Self::AddBranch | Self::Test { .. } => Width::I32,
        }
    }

    /// The width of the value it computes.
    pub(crate) const fn result_width(self) -> Width {
        match self {
            Self::Binary {
                operator, width, ..
            } => binary_result(operator, width),
            Self::Unary { operator, width } => unary_result(operator, width),
            Self::BitField | Self::AddBranch | Self::Test { .. } => Width::I32,
        }
    }

    /// The registers it reads: the left operand, and the right where that is a register.
    pub(crate) fn reads(self, instruction: WideInstruction) -> (u16, Option<u16>) {
        match self {
            Self::Binary { form, .. } => match form.right(instruction) {
                Operand::Register(register) => (form.left(instruction), Some(register)),
                Operand::Constant(_) => (form.left(instruction), None),
            },
            Self::Test { .. } => (instruction.a(), None),
            _ => (instruction.b(), None),
        }
    }

    /// Whether it defines register `a`: a comparison that only jumps does not.
    pub(crate) const fn writes(self) -> bool {
        !matches!(self, Self::Test { .. })
            && !matches!(self, Self::Binary { form, .. } if form.branches())
    }

    /// Whether it ends the block with a jump on its value.
    pub(crate) const fn jumps(self) -> bool {
        matches!(self, Self::AddBranch) || !self.writes()
    }
}
