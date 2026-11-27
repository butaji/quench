//! Float width projections share declarations while preserving IEEE bits.

// The runtime's list macros come from the crate root (`lib.rs`); a stencil build mounts its own.
#[cfg(stencil_build)]
#[macro_use]
#[path = "../staged/wasm_lists.rs"]
mod wasm_lists;

#[cfg(not(stencil_build))]
use super::WasmValue;
#[cfg(test)]
use super::numeric::NumericResult;
use super::numeric::selectors;
#[cfg(not(stencil_build))]
use crate::staged::ConcreteWasm;
#[cfg(not(stencil_build))]
use crate::staged::machine::MachineStage;
#[cfg(not(stencil_build))]
use crate::staged::wasm_float::{Float, float_binary, is_float_comparison};

macro_rules! binary_family {
    ($enum:ident, $float:ty, $bits:ty, $variant:ident,
     ($float_alias:ident, $bits_alias:ident, $sign:ident, $nan:ident), ($left:ident, $right:ident);
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!(@decoded $enum; $($name, $wasm;)+);
        impl $enum {
            /// The operator written out on its own: the oracle the staged rules are tested
            /// against (`staged::wasm_float_tests`); execution goes through `evaluate`.
            #[cfg(test)]
            #[inline(always)]
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
        impl F32BinaryOperator {
            /// The f64 operator of the same name: the f64 selectors extend the f32 ones.
            pub(crate) const fn widen(self) -> F64BinaryOperator {
                match self { $(Self::$name => F64BinaryOperator::$name,)+ }
            }
        }
    };
}

macro_rules! unary_family {
    ($enum:ident, $float:ty, $bits:ty, $variant:ident,
     ($float_alias:ident, $bits_alias:ident, $sign:ident), $value:ident;
     $($name:ident, $wasm:ident => $body:expr;)+) => {
        selectors!(@decoded $enum; $($name, $wasm;)+);
        impl $enum {
            /// The operator written out on its own: the oracle the staged rules are tested
            /// against (`staged::wasm_float_tests`); execution goes through `evaluate`.
            #[cfg(test)]
            #[inline(always)]
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
        impl F32UnaryOperator {
            /// The f64 operator of the same name: the f64 selectors extend the f32 ones.
            pub(crate) const fn widen(self) -> F64UnaryOperator {
                match self { $(Self::$name => F64UnaryOperator::$name,)+ }
            }
        }
    };
}

float_binary_list!(float_binary_operators);

float_unary_list!(float_unary_operators);

impl F64BinaryOperator {
    pub(crate) const fn widen(self) -> Self {
        self
    }

    /// Whether a NaN operand matters to the result only as a NaN (Wasm leaves the payload
    /// of such a result open): any word of an f64 register reads as its operand.
    pub(crate) const fn nan_agnostic(self) -> bool {
        matches!(
            self,
            Self::Add | Self::Subtract | Self::Multiply | Self::Divide | Self::Equal | Self::NotEqual
                | Self::Less | Self::Greater | Self::LessEqual | Self::GreaterEqual
        )
    }
}

impl F64UnaryOperator {
    pub(crate) const fn widen(self) -> Self {
        self
    }

    /// `F64BinaryOperator::nan_agnostic` of a unary operator.
    pub(crate) const fn nan_agnostic(self) -> bool {
        matches!(self, Self::SquareRoot | Self::Ceiling | Self::Floor | Self::Truncate | Self::Nearest)
    }
}

/// The interpreter's execution of the operators of one float width, through the staged rules
/// (`staged::wasm_float`); `apply` stays the declaration the tests compare them to.
#[cfg(not(stencil_build))]
macro_rules! float_evaluation {
    ($binary:ident, $unary:ident, $float:ty, $bits:ty, $variant:ident, $width:ident) => {
        impl $binary {
            #[inline(always)]
            pub(crate) fn evaluate(self, left: $float, right: $float) -> WasmValue {
                let operator = self.widen();
                let word = float_binary(
                    &mut ConcreteWasm,
                    operator,
                    Float::$width,
                    left.to_bits() as u64,
                    right.to_bits() as u64,
                );
                if is_float_comparison(operator) {
                    WasmValue::I32(word as i32)
                } else {
                    WasmValue::$variant(word as $bits)
                }
            }
        }

        impl $unary {
            #[inline(always)]
            pub(crate) fn evaluate(self, value: $float) -> WasmValue {
                let word = ConcreteWasm.operate_float_unary(
                    self.widen(),
                    Float::$width,
                    value.to_bits() as u64,
                );
                WasmValue::$variant(word as $bits)
            }
        }
    };
}

#[cfg(not(stencil_build))]
float_evaluation!(F32BinaryOperator, F32UnaryOperator, f32, u32, F32, F32);
#[cfg(not(stencil_build))]
float_evaluation!(F64BinaryOperator, F64UnaryOperator, f64, u64, F64, F64);
