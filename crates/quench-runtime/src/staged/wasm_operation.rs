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
pub enum Form {
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
pub enum Operand {
    Register(u16),
    Constant(i64),
}

impl Form {
    pub fn left(self, instruction: WideInstruction) -> u16 {
        match self {
            Self::Register | Self::Immediate | Self::Short => instruction.b(),
            Self::Branch | Self::BranchImmediate => instruction.a(),
        }
    }

    pub fn right(self, instruction: WideInstruction) -> Operand {
        match self {
            Self::Register => Operand::Register(instruction.c()),
            Self::Immediate => Operand::Constant(i64::from(instruction.imm() as i32)),
            Self::Short => Operand::Constant(i64::from(instruction.c() as i16)),
            Self::Branch => Operand::Register(instruction.b()),
            Self::BranchImmediate => Operand::Constant(i64::from(instruction.b() as i16)),
        }
    }

    /// Whether the value is a jump condition, not a register.
    pub const fn branches(self) -> bool {
        matches!(self, Self::Branch | Self::BranchImmediate)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WasmOperation {
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

/// An i32 binary operation, widened to the i64 operator of the same name.
const fn i32_binary(operator: I32BinaryOperator, form: Form) -> WasmOperation {
    WasmOperation::Binary {
        operator: operator.widen(),
        width: Width::I32,
        form,
    }
}

impl WasmOperation {
    /// Whether `op` is a raw form: its operands are the registers' words as they are, with no
    /// tag checks.
    pub(crate) const fn is_raw(op: Op) -> bool {
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
    pub const fn of(op: Op, tag: u32) -> Option<Self> {
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
        if let Some(selected) = Self::selected(op.class(), tag) {
            return Some(selected);
        }
        Self::floating(op.class(), tag)
    }

    /// The float and conversion operations, whose operator is a selector operand.
    const fn floating(class: Class, tag: u32) -> Option<Self> {
        match class {
            Class::WasmFloatBinary32 => match F32BinaryOperator::from_tag(tag) {
                Some(operator) => Some(Self::FloatBinary {
                    operator: operator.widen(),
                    float: Float::F32,
                }),
                None => None,
            },
            Class::WasmFloatBinary64 | Class::WasmFloatBinary64Raw => match FloatBinary::from_tag(tag) {
                Some(operator) => Some(Self::FloatBinary {
                    operator,
                    float: Float::F64,
                }),
                None => None,
            },
            Class::WasmFloatUnary32 => match F32UnaryOperator::from_tag(tag) {
                Some(operator) => Some(Self::FloatUnary {
                    operator: operator.widen(),
                    float: Float::F32,
                }),
                None => None,
            },
            Class::WasmFloatUnary64 | Class::WasmFloatUnary64Raw => match FloatUnary::from_tag(tag) {
                Some(operator) => Some(Self::FloatUnary {
                    operator,
                    float: Float::F64,
                }),
                None => None,
            },
            Class::WasmScalarConvert => match ScalarConversionOperator::from_tag(tag) {
                Some(operator) => Some(Self::Convert { operator }),
                None => None,
            },
            _ => None,
        }
    }

    /// The integer operations whose operator is a selector operand.
    const fn selected(class: Class, tag: u32) -> Option<Self> {
        match class {
            Class::WasmI64Binary | Class::WasmI64BinaryRaw => match Binary::from_tag(tag) {
                Some(operator) => Some(Self::Binary {
                    operator,
                    width: Width::I64,
                    form: Form::Register,
                }),
                None => None,
            },
            Class::WasmI64BinaryImmediate | Class::WasmI64BinaryImmediateRaw => match Binary::from_tag(tag) {
                Some(operator) => Some(Self::Binary {
                    operator,
                    width: Width::I64,
                    form: Form::Short,
                }),
                None => None,
            },
            Class::WasmI64Unary | Class::WasmI64UnaryRaw => match Unary::from_tag(tag) {
                Some(operator) => Some(Self::Unary {
                    operator,
                    width: Width::I64,
                }),
                None => None,
            },
            Class::WasmI32Unary => match I32UnaryOperator::from_tag(tag) {
                Some(operator) => Some(Self::Unary {
                    operator: operator.widen(),
                    width: Width::I32,
                }),
                None => None,
            },
            Class::WasmI32ShiftRightUnsignedAnd => Some(Self::BitField),
            Class::WasmI32AddJump => Some(Self::AddBranch),
            Class::WasmJumpI32NonZero => Some(Self::Test { nonzero: true }),
            Class::WasmJumpI32Zero => Some(Self::Test { nonzero: false }),
            _ => None,
        }
    }

    /// The type its operands have.
    pub fn operand_type(self) -> WasmType {
        match self {
            Self::Binary { width, .. } | Self::Unary { width, .. } => width.ty(),
            Self::BitField | Self::AddBranch | Self::Test { .. } => WasmType::I32,
            Self::FloatBinary { float, .. } | Self::FloatUnary { float, .. } => float.ty(),
            Self::Convert { operator } => operator.source_type(),
        }
    }

    /// The type of the value it computes.
    pub fn result_type(self) -> WasmType {
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
    pub fn reads(self, instruction: WideInstruction) -> (u16, Option<u16>) {
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
    pub const fn writes(self) -> bool {
        !matches!(self, Self::Test { .. })
            && !matches!(self, Self::Binary { form, .. } if form.branches())
    }

    /// Whether it ends the block with a jump on its value.
    pub const fn jumps(self) -> bool {
        matches!(self, Self::AddBranch) || !self.writes()
    }
}
