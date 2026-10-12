//! Float width projections share declarations while preserving IEEE bits.

use super::WasmValue;
use super::numeric::{NumericResult, selectors};

macro_rules! binary_family {
    ($enum:ident, $float:ty, $bits:ty, $variant:ident,
     ($float_alias:ident, $bits_alias:ident, $sign:ident, $nan:ident), ($left:ident, $right:ident);
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!($enum; $($name, $wasm;)+);
        impl $enum {
            pub(crate) fn apply(self, $left: $float, $right: $float) -> WasmValue {
                type $float_alias = $float;
                type $bits_alias = $bits;
                const $sign: $bits_alias = super::numeric::sign_mask($bits_alias::BITS) as $bits_alias;
                const $nan: $float = <$float>::from_bits(
                    super::numeric::canonical_nan_bits(<$float>::MANTISSA_DIGITS, <$float>::INFINITY.to_bits() as u64) as $bits_alias);
                let result: NumericResult<$float> = match self { $(Self::$name => $body,)+ };
                result.into_wasm(|value| WasmValue::$variant(value.to_bits()))
            }
        }
    };
}

macro_rules! float_binary_operators {
    ($aliases:tt, $arguments:tt; $($name:ident, $f32:ident, $f64:ident => $body:expr;)+) => {
        binary_family!(F32BinaryOperator, f32, u32, F32, $aliases, $arguments; $($name, $f32 => $body;)+);
        binary_family!(F64BinaryOperator, f64, u64, F64, $aliases, $arguments; $($name, $f64 => $body;)+);
    };
}

macro_rules! unary_family {
    ($enum:ident, $float:ty, $bits:ty, $variant:ident,
     ($float_alias:ident, $bits_alias:ident, $sign:ident), $value:ident;
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!($enum; $($name, $wasm;)+);
        impl $enum {
            pub(crate) fn apply(self, $value: $float) -> WasmValue {
                type $float_alias = $float;
                type $bits_alias = $bits;
                const $sign: $bits_alias = super::numeric::sign_mask($bits_alias::BITS) as $bits_alias;
                let value: $float = match self { $(Self::$name => $body,)+ };
                const EXPONENT_MASK: $bits_alias = <$float_alias>::INFINITY.to_bits();
                const QUIET_NAN_BIT: $bits_alias =
                    (super::numeric::canonical_nan_bits(
                        <$float_alias>::MANTISSA_DIGITS,
                        <$float_alias>::INFINITY.to_bits() as u64,
                    ) as $bits_alias)
                        ^ EXPONENT_MASK;
                const MANTISSA_MASK: $bits_alias =
                    ((1 as $bits_alias) << (<$float_alias>::MANTISSA_DIGITS - 1)) - 1;
                let bits = value.to_bits();
                let bits = if matches!(
                    self,
                    Self::Ceiling
                        | Self::Floor
                        | Self::Truncate
                        | Self::Nearest
                        | Self::SquareRoot
                ) && bits & EXPONENT_MASK == EXPONENT_MASK
                    && bits & MANTISSA_MASK != 0
                {
                    bits | QUIET_NAN_BIT
                } else {
                    bits
                };
                WasmValue::$variant(bits)
            }
        }
    };
}

macro_rules! float_unary_operators {
    ($aliases:tt, $argument:ident; $($name:ident, $f32:ident, $f64:ident => $body:expr;)+) => {
        unary_family!(F32UnaryOperator, f32, u32, F32, $aliases, $argument; $($name, $f32 => $body;)+);
        unary_family!(F64UnaryOperator, f64, u64, F64, $aliases, $argument; $($name, $f64 => $body;)+);
    };
}

float_binary_operators! { (Float, Bits, SIGN_MASK, CANONICAL_NAN), (left, right);
    Add, F32Add, F64Add => NumericResult::Value(left + right);
    Subtract, F32Sub, F64Sub => NumericResult::Value(left - right);
    Multiply, F32Mul, F64Mul => NumericResult::Value(left * right);
    Divide, F32Div, F64Div => NumericResult::Value(left / right);
    Minimum, F32Min, F64Min => NumericResult::Value({
        if left.is_nan() || right.is_nan() { CANONICAL_NAN }
        else if left == right { Float::from_bits(left.to_bits() | right.to_bits()) }
        else { left.min(right) }
    });
    Maximum, F32Max, F64Max => NumericResult::Value({
        if left.is_nan() || right.is_nan() { CANONICAL_NAN }
        else if left == right { Float::from_bits(left.to_bits() & right.to_bits()) }
        else { left.max(right) }
    });
    CopySign, F32Copysign, F64Copysign => NumericResult::Value(Float::from_bits(
        (left.to_bits() & !SIGN_MASK) | (right.to_bits() & SIGN_MASK)));
    Equal, F32Eq, F64Eq => NumericResult::Comparison(left == right);
    NotEqual, F32Ne, F64Ne => NumericResult::Comparison(left != right);
    Less, F32Lt, F64Lt => NumericResult::Comparison(left < right);
    Greater, F32Gt, F64Gt => NumericResult::Comparison(left > right);
    LessEqual, F32Le, F64Le => NumericResult::Comparison(left <= right);
    GreaterEqual, F32Ge, F64Ge => NumericResult::Comparison(left >= right);
}

float_unary_operators! { (Float, Bits, SIGN_MASK), value;
    Absolute, F32Abs, F64Abs => Float::from_bits(value.to_bits() & !SIGN_MASK);
    Negate, F32Neg, F64Neg => Float::from_bits(value.to_bits() ^ SIGN_MASK);
    Ceiling, F32Ceil, F64Ceil => value.ceil();
    Floor, F32Floor, F64Floor => value.floor();
    Truncate, F32Trunc, F64Trunc => value.trunc();
    Nearest, F32Nearest, F64Nearest => value.round_ties_even();
    SquareRoot, F32Sqrt, F64Sqrt => value.sqrt();
}
