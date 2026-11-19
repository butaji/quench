//! Scalar conversions keep selector, input/output type, and semantics together.

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

            pub(crate) fn result_type(self) -> WasmType {
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

scalar_conversions! { value;
    WrapI64, I32WrapI64, I64 => I32, WasmValue::I32(value as i32);
    ExtendI32Signed, I64ExtendI32S, I32 => I64, WasmValue::I64(i64::from(value));
    ExtendI32Unsigned, I64ExtendI32U, I32 => I64, WasmValue::I64(i64::from(value as u32));

    TruncateF32SignedI32, I32TruncF32S, F32 => I32, WasmValue::I32(trunc_to_integer(f64::from(f32::from_bits(value)), i32::BITS, true, false)? as i32);
    TruncateF32UnsignedI32, I32TruncF32U, F32 => I32, WasmValue::I32(trunc_to_integer(f64::from(f32::from_bits(value)), i32::BITS, false, false)? as i32);
    TruncateF64SignedI32, I32TruncF64S, F64 => I32, WasmValue::I32(trunc_to_integer(f64::from_bits(value), i32::BITS, true, false)? as i32);
    TruncateF64UnsignedI32, I32TruncF64U, F64 => I32, WasmValue::I32(trunc_to_integer(f64::from_bits(value), i32::BITS, false, false)? as i32);
    TruncateF32SignedI64, I64TruncF32S, F32 => I64, WasmValue::I64(trunc_to_integer(f64::from(f32::from_bits(value)), i64::BITS, true, false)?);
    TruncateF32UnsignedI64, I64TruncF32U, F32 => I64, WasmValue::I64(trunc_to_integer(f64::from(f32::from_bits(value)), i64::BITS, false, false)?);
    TruncateF64SignedI64, I64TruncF64S, F64 => I64, WasmValue::I64(trunc_to_integer(f64::from_bits(value), i64::BITS, true, false)?);
    TruncateF64UnsignedI64, I64TruncF64U, F64 => I64, WasmValue::I64(trunc_to_integer(f64::from_bits(value), i64::BITS, false, false)?);

    SaturateF32SignedI32, I32TruncSatF32S, F32 => I32, WasmValue::I32(trunc_to_integer(f64::from(f32::from_bits(value)), i32::BITS, true, true)? as i32);
    SaturateF32UnsignedI32, I32TruncSatF32U, F32 => I32, WasmValue::I32(trunc_to_integer(f64::from(f32::from_bits(value)), i32::BITS, false, true)? as i32);
    SaturateF64SignedI32, I32TruncSatF64S, F64 => I32, WasmValue::I32(trunc_to_integer(f64::from_bits(value), i32::BITS, true, true)? as i32);
    SaturateF64UnsignedI32, I32TruncSatF64U, F64 => I32, WasmValue::I32(trunc_to_integer(f64::from_bits(value), i32::BITS, false, true)? as i32);
    SaturateF32SignedI64, I64TruncSatF32S, F32 => I64, WasmValue::I64(trunc_to_integer(f64::from(f32::from_bits(value)), i64::BITS, true, true)?);
    SaturateF32UnsignedI64, I64TruncSatF32U, F32 => I64, WasmValue::I64(trunc_to_integer(f64::from(f32::from_bits(value)), i64::BITS, false, true)?);
    SaturateF64SignedI64, I64TruncSatF64S, F64 => I64, WasmValue::I64(trunc_to_integer(f64::from_bits(value), i64::BITS, true, true)?);
    SaturateF64UnsignedI64, I64TruncSatF64U, F64 => I64, WasmValue::I64(trunc_to_integer(f64::from_bits(value), i64::BITS, false, true)?);

    ConvertI32SignedF32, F32ConvertI32S, I32 => F32, WasmValue::F32((value as f32).to_bits());
    ConvertI32UnsignedF32, F32ConvertI32U, I32 => F32, WasmValue::F32((value as u32 as f32).to_bits());
    ConvertI64SignedF32, F32ConvertI64S, I64 => F32, WasmValue::F32((value as f32).to_bits());
    ConvertI64UnsignedF32, F32ConvertI64U, I64 => F32, WasmValue::F32((value as u64 as f32).to_bits());
    ConvertI32SignedF64, F64ConvertI32S, I32 => F64, WasmValue::F64((value as f64).to_bits());
    ConvertI32UnsignedF64, F64ConvertI32U, I32 => F64, WasmValue::F64((value as u32 as f64).to_bits());
    ConvertI64SignedF64, F64ConvertI64S, I64 => F64, WasmValue::F64((value as f64).to_bits());
    ConvertI64UnsignedF64, F64ConvertI64U, I64 => F64, WasmValue::F64((value as u64 as f64).to_bits());
    PromoteF32, F64PromoteF32, F32 => F64, WasmValue::F64(f64::from(f32::from_bits(value)).to_bits());
    DemoteF64, F32DemoteF64, F64 => F32, WasmValue::F32((f64::from_bits(value) as f32).to_bits());

    ReinterpretF32I32, I32ReinterpretF32, F32 => I32, WasmValue::I32(value as i32);
    ReinterpretF64I64, I64ReinterpretF64, F64 => I64, WasmValue::I64(value as i64);
    ReinterpretI32F32, F32ReinterpretI32, I32 => F32, WasmValue::F32(value as u32);
    ReinterpretI64F64, F64ReinterpretI64, I64 => F64, WasmValue::F64(value as u64);
}
