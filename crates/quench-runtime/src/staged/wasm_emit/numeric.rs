//! Emit functions for the i32, i64 and scalar-conversion operators, and the families the
//! lowering emits by selector.

use super::Emission;
use crate::bytecode::{Op, Register};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator, I64BinaryOperator};

/// `destination = operator(source)` for an i32 unary operator.
pub(crate) fn i32_unary(
    destination: Register,
    source: Register,
    operator: I32UnaryOperator,
) -> Emission {
    Emission::new(Op::WasmI32Unary, destination, source, 0, operator as u32)
}

/// `destination = left operator right` for an i64 operator.
pub(crate) fn i64_binary(
    operator: I64BinaryOperator,
    destination: Register,
    left: Register,
    right: Register,
) -> Emission {
    Emission::new(Op::WasmI64Binary, destination, left, right, operator as u32)
}

/// `destination = convert(source)` for a scalar conversion.
pub(crate) fn scalar_convert(
    destination: Register,
    source: Register,
    operator: ScalarConversionOperator,
) -> Emission {
    Emission::new(
        Op::WasmScalarConvert,
        destination,
        source,
        0,
        operator as u32,
    )
}

/// `destination = left operator right` for an i32 operator in its register form.
#[cfg(not(stencil_build))]
pub(crate) fn i32_binary(
    operator: I32BinaryOperator,
    destination: Register,
    left: Register,
    right: Register,
) -> Emission {
    Emission::new(operator.register_op(), destination, left, right, 0)
}

/// `destination = left operator constant` for an i32 operator in its immediate form.
#[cfg(not(stencil_build))]
pub(crate) fn i32_binary_immediate(
    operator: I32BinaryOperator,
    destination: Register,
    left: Register,
    constant: u32,
) -> Emission {
    Emission::new(operator.immediate_op(), destination, left, 0, constant)
}

/// `destination = (source >>> amount) & mask`, the bit-field extract a logical shift and a mask
/// fuse into.
pub(crate) fn bit_field_extract(
    destination: Register,
    source: Register,
    amount: Register,
    mask: u32,
) -> Emission {
    Emission::new(
        Op::WasmI32ShiftRightUnsignedAndImmediate,
        destination,
        source,
        amount,
        mask,
    )
}

/// `destination = left operator constant` on i64, the constant a signed 16-bit value and the
/// operator the selector.
pub(crate) fn i64_binary_immediate(
    destination: Register,
    left: Register,
    constant: i16,
    selector: u32,
) -> Emission {
    Emission::new(
        Op::WasmI64BinaryImmediate,
        destination,
        left,
        constant as u16,
        selector,
    )
}

/// The families the lowering emits by selector: each names one opcode, and the operator is the
/// selector. Each decodes from its Wasm operators (`wasm.rs` `selector_numeric_operator`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectorFamily {
    I32Unary,
    I64Binary,
    I64Unary,
    ScalarConvert,
    F32Binary,
    F32Unary,
    F64Binary,
    F64Unary,
}

impl SelectorFamily {
    /// Whether the family's operator reads a right operand: its opcode's third field is a register.
    pub(crate) const fn takes_right(self) -> bool {
        matches!(self, Self::I64Binary | Self::F32Binary | Self::F64Binary)
    }

    /// `destination = operator(left, right)` for the operator `selector` names.
    pub(crate) fn emit(
        self,
        destination: Register,
        left: Register,
        right: Register,
        selector: u32,
    ) -> Emission {
        let op = match self {
            Self::I32Unary => Op::WasmI32Unary,
            Self::I64Binary => Op::WasmI64Binary,
            Self::I64Unary => Op::WasmI64Unary,
            Self::ScalarConvert => Op::WasmScalarConvert,
            Self::F32Binary => Op::WasmF32Binary,
            Self::F32Unary => Op::WasmF32Unary,
            Self::F64Binary => Op::WasmF64Binary,
            Self::F64Unary => Op::WasmF64Unary,
        };
        Emission::new(op, destination, left, right, selector)
    }
}
