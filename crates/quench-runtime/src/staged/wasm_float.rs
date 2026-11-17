//! Wasm float and conversion semantics written once over `WasmStage`: the NaN rule of the
//! extrema and the traps of a truncation with the order in which it checks them. Every other
//! float operator is a stage primitive (`operate_float`, `operate_float_unary`).

use super::wasm::{WasmStage, Width};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::F64BinaryOperator as FloatBinary;
use crate::wasm::numeric::canonical_nan_bits;
use crate::{WasmTrap, WasmType};

/// The float width an operation runs at; an f32 operator is its f64 namesake at `F32`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Float {
    F32,
    F64,
}

impl Float {
    pub(crate) const fn of(ty: WasmType) -> Option<Self> {
        match ty {
            WasmType::F32 => Some(Self::F32),
            WasmType::F64 => Some(Self::F64),
            _ => None,
        }
    }

    pub(crate) const fn ty(self) -> WasmType {
        match self {
            Self::F32 => WasmType::F32,
            Self::F64 => WasmType::F64,
        }
    }

    /// The IEEE bits of the NaN every float operation produces from an unordered pair.
    #[inline]
    pub(crate) const fn canonical_nan(self) -> u64 {
        match self {
            Self::F32 => canonical_nan_bits(f32::MANTISSA_DIGITS, f32::INFINITY.to_bits() as u64),
            Self::F64 => canonical_nan_bits(f64::MANTISSA_DIGITS, f64::INFINITY.to_bits()),
        }
    }
}

/// A trapping float-to-integer conversion: the float it reads and the integers it may produce.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Truncation {
    pub(crate) float: Float,
    pub(crate) width: Width,
    pub(crate) signed: bool,
}

/// Whether `operator` compares: its result is an `I32` flag at either float width.
#[inline]
pub(crate) const fn is_float_comparison(operator: FloatBinary) -> bool {
    matches!(
        operator,
        FloatBinary::Equal
            | FloatBinary::NotEqual
            | FloatBinary::Less
            | FloatBinary::Greater
            | FloatBinary::LessEqual
            | FloatBinary::GreaterEqual
    )
}

/// `operator` on `left` and `right`: an extremum with a NaN operand is the canonical NaN. The
/// one place that rule is written.
#[inline(always)]
pub(crate) fn float_binary<S: WasmStage>(
    stage: &mut S,
    operator: FloatBinary,
    float: Float,
    left: S::Word,
    right: S::Word,
) -> S::Word {
    let value = stage.operate_float(operator, float, left, right);
    if !matches!(operator, FloatBinary::Minimum | FloatBinary::Maximum) {
        return value;
    }
    let unordered = stage.float_unordered(float, left, right);
    let nan = stage.float_constant(float, float.canonical_nan());
    stage.select(unordered, nan, value)
}

/// `operator` on `value`: a truncation to integer traps on a NaN, then outside the integers'
/// range. The one place these rules are written.
#[inline(always)]
pub(crate) fn convert<S: WasmStage>(
    stage: &mut S,
    operator: ScalarConversionOperator,
    value: S::Word,
) -> Result<S::Word, S::Exit> {
    if let Some(truncation) = operator.truncation() {
        let unordered = stage.float_unordered(truncation.float, value, value);
        stage.trap_when(unordered, WasmTrap::InvalidConversionToInteger)?;
        let overflows = stage.truncation_overflows(truncation, value);
        stage.trap_when(overflows, WasmTrap::IntegerOverflow)?;
    }
    Ok(stage.convert(operator, value))
}
