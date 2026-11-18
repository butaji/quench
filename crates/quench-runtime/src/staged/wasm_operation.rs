//! The Wasm integer opcodes as data: what an opcode computes, at which width, and where its
//! operands and result go. Every tier reads this table; none names an integer opcode.

use super::wasm::{Width, binary_result, unary_result};
use super::wasm_float::{Float, is_float_comparison};
use crate::WasmType;
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{
    F32BinaryOperator, F32UnaryOperator, F64BinaryOperator as FloatBinary,
    F64UnaryOperator as FloatUnary,
};
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
    /// `a = b op c`, `c` being a 16-bit constant (the selector is in `imm`).
    Short,
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
            Self::Register | Self::Immediate | Self::Short => instruction.b(),
            Self::Branch | Self::BranchImmediate => instruction.a(),
        }
    }

    pub(crate) fn right(self, instruction: WideInstruction) -> Operand {
        match self {
            Self::Register => Operand::Register(instruction.c()),
            Self::Immediate => Operand::Constant(i64::from(instruction.imm() as i32)),
            Self::Short => Operand::Constant(i64::from(instruction.c() as i16)),
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
    /// `a = b op c` on floats; a comparison yields an i32 flag.
    FloatBinary {
        operator: FloatBinary,
        float: Float,
    },
    /// `a = op b` on a float.
    FloatUnary {
        operator: FloatUnary,
        float: Float,
    },
    /// `a = op b`, a conversion between scalar types.
    Convert {
        operator: ScalarConversionOperator,
    },
    /// `a = (b >> c) & imm` on i32: a bit-field extract.
    BitField,
    /// `a = b + c` on i32, jumping where the sum is nonzero.
    AddBranch,
    /// Jumps where the i32 `a` is nonzero (or zero).
    Test {
        nonzero: bool,
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
            .or_else(|| Self::floating(op.class(), tag))
    }

    /// The float and conversion operations, whose operator is a selector operand.
    fn floating(class: Class, tag: u32) -> Option<Self> {
        match class {
            Class::WasmFloatBinary32 => {
                F32BinaryOperator::from_tag(tag).map(|operator| Self::FloatBinary {
                    operator: operator.widen(),
                    float: Float::F32,
                })
            }
            Class::WasmFloatBinary64 => {
                FloatBinary::from_tag(tag).map(|operator| Self::FloatBinary {
                    operator,
                    float: Float::F64,
                })
            }
            Class::WasmFloatUnary32 => {
                F32UnaryOperator::from_tag(tag).map(|operator| Self::FloatUnary {
                    operator: operator.widen(),
                    float: Float::F32,
                })
            }
            Class::WasmFloatUnary64 => FloatUnary::from_tag(tag).map(|operator| Self::FloatUnary {
                operator,
                float: Float::F64,
            }),
            Class::WasmScalarConvert => {
                ScalarConversionOperator::from_tag(tag).map(|operator| Self::Convert { operator })
            }
            _ => None,
        }
    }

    /// The integer operations whose operator is a selector operand.
    fn selected(class: Class, tag: u32) -> Option<Self> {
        match class {
            Class::WasmI64Binary | Class::WasmI64BinaryRaw => Binary::from_tag(tag).map(|operator| Self::Binary {
                operator,
                width: Width::I64,
                form: Form::Register,
            }),
            Class::WasmI64BinaryImmediate | Class::WasmI64BinaryImmediateRaw => Binary::from_tag(tag).map(|operator| Self::Binary {
                operator,
                width: Width::I64,
                form: Form::Short,
            }),
            Class::WasmI64Unary | Class::WasmI64UnaryRaw => Unary::from_tag(tag).map(|operator| Self::Unary {
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

    /// The type its operands have.
    pub(crate) fn operand_type(self) -> WasmType {
        match self {
            Self::Binary { width, .. } | Self::Unary { width, .. } => width.ty(),
            Self::BitField | Self::AddBranch | Self::Test { .. } => WasmType::I32,
            Self::FloatBinary { float, .. } | Self::FloatUnary { float, .. } => float.ty(),
            Self::Convert { operator } => operator.source_type(),
        }
    }

    /// The type of the value it computes.
    pub(crate) fn result_type(self) -> WasmType {
        match self {
            Self::Binary {
                operator, width, ..
            } => binary_result(operator, width).ty(),
            Self::Unary { operator, width } => unary_result(operator, width).ty(),
            Self::BitField | Self::AddBranch | Self::Test { .. } => WasmType::I32,
            Self::FloatBinary { operator, .. } if is_float_comparison(operator) => WasmType::I32,
            Self::FloatBinary { float, .. } | Self::FloatUnary { float, .. } => float.ty(),
            Self::Convert { operator } => operator.result_type(),
        }
    }

    /// The registers it reads: the left operand, and the right where that is a register.
    pub(crate) fn reads(self, instruction: WideInstruction) -> (u16, Option<u16>) {
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
