//! Scalar conversions keep selector, input/output type, and semantics together.

#[macro_use]
#[path = "../staged/wasm_lists.rs"]
mod wasm_lists;

use super::numeric::selectors;
use super::WasmType;
#[cfg(not(stencil_build))]
use super::WasmTrap;
#[cfg(not(stencil_build))]
use super::WasmValue;
#[cfg(not(stencil_build))]
use crate::staged::ConcreteWasm;
use crate::staged::wasm::Width;
use crate::staged::wasm_float::{Float, Truncation};
#[cfg(not(stencil_build))]
use crate::staged::wasm_float::convert;

macro_rules! scalar_conversions {
    ($value:ident; $($name:ident, $wasm:ident, $source:ident => $result:ident, $body:expr;)+) => {
        selectors!(@decoded ScalarConversionOperator; $($name, $wasm;)+);

        impl ScalarConversionOperator {
            pub(crate) fn source_type(self) -> WasmType {
                match self { $(Self::$name => WasmType::$source,)+ }
            }

            pub fn result_type(self) -> WasmType {
                match self { $(Self::$name => WasmType::$result,)+ }
            }

            /// The conversion written out on its own: the oracle `ConcreteWasm::convert` is tested
            /// against; execution goes through `evaluate`.
            #[cfg(test)]
            pub(crate) fn apply(self, value: WasmValue) -> Result<WasmValue, WasmTrap> {
                match (self, value) {
                    $((Self::$name, WasmValue::$source($value)) => Ok($body),)+
                    _ => unreachable!("validated Wasm scalar conversion operand"),
                }
            }
        }
    };
}

impl ScalarConversionOperator {
    /// The integers a trapping float-to-integer conversion may produce; `None` for every other
    /// conversion, which cannot trap.
    pub(crate) fn truncation(self) -> Option<Truncation> {
        self.float_to_integer().filter(|&(_, saturating)| !saturating).map(|(truncation, _)| truncation)
    }

    /// The integers a float-to-integer conversion produces, and whether it saturates instead of
    /// trapping outside them; `None` for every other conversion.
    pub(crate) fn float_to_integer(self) -> Option<(Truncation, bool)> {
        let (signed, saturating) = match self {
            Self::TruncateF32SignedI32
            | Self::TruncateF64SignedI32
            | Self::TruncateF32SignedI64
            | Self::TruncateF64SignedI64 => (true, false),
            Self::TruncateF32UnsignedI32
            | Self::TruncateF64UnsignedI32
            | Self::TruncateF32UnsignedI64
            | Self::TruncateF64UnsignedI64 => (false, false),
            Self::SaturateF32SignedI32
            | Self::SaturateF64SignedI32
            | Self::SaturateF32SignedI64
            | Self::SaturateF64SignedI64 => (true, true),
            Self::SaturateF32UnsignedI32
            | Self::SaturateF64UnsignedI32
            | Self::SaturateF32UnsignedI64
            | Self::SaturateF64UnsignedI64 => (false, true),
            _ => return None,
        };
        let truncation = Truncation {
            float: Float::of(self.source_type())?,
            width: Width::of(self.result_type())?,
            signed,
        };
        Some((truncation, saturating))
    }

    /// `apply` through the staged rules (`staged::wasm_float`).
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub(crate) fn evaluate(self, value: WasmValue) -> Result<WasmValue, WasmTrap> {
        let word = convert(&mut ConcreteWasm, self, value.word())?;
        Ok(WasmValue::from_bits(self.result_type(), word))
    }
}

/// The declared float-to-integer conversion, trapping or saturating, written from the spec alone:
/// the oracle the staged rules (`staged::wasm_float::convert`, `saturated`) are tested against.
#[cfg(test)]
fn trunc_to_integer(value: f64, bits: u32, signed: bool, saturating: bool) -> Result<i64, WasmTrap> {
    let (min, max): (i128, i128) = if signed {
        (-(1_i128 << (bits - 1)), (1_i128 << (bits - 1)) - 1)
    } else {
        (0, (1_i128 << bits) - 1)
    };
    if value.is_nan() {
        return if saturating { Ok(0) } else { Err(WasmTrap::InvalidConversionToInteger) };
    }
    let value = value.trunc();
    if value < min as f64 {
        return if saturating { Ok(min as i64) } else { Err(WasmTrap::IntegerOverflow) };
    }
    if value >= (max + 1) as f64 {
        return if saturating { Ok(max as i64) } else { Err(WasmTrap::IntegerOverflow) };
    }
    Ok(if signed { value as i64 } else { value as u64 as i64 })
}

scalar_conversion_list!(scalar_conversions);
