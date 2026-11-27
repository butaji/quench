//! The Wasm integer opcodes as data: what an opcode computes, at which width, and where its
//! operands and result go. Every tier reads this table; none names an integer opcode. The reads of
//! the table from an opcode and its record (`of`, `reads`, the operand positions of a `Form`) are in
//! `wasm_operation/glue.rs`.

use super::wasm::{Width, binary_result, unary_result};
use super::wasm_float::{Float, is_float_comparison};
use crate::WasmType;
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I32BinaryOperator, I64BinaryOperator as Binary, I64UnaryOperator as Unary};

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
pub const fn i32_binary(operator: I32BinaryOperator, form: Form) -> WasmOperation {
    WasmOperation::Binary {
        operator: operator.widen(),
        width: Width::I32,
        form,
    }
}

impl WasmOperation {
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

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
