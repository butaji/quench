//! Emit functions for the i32, i64 and scalar-conversion operators.

use super::Emission;
use crate::bytecode::{Op, Register};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::integer::{I32UnaryOperator, I64BinaryOperator};

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
