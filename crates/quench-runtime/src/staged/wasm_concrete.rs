//! The interpreter's Wasm stage: integers are the zero-extended `u64` words of their width.

use super::machine::{Machine, MachineStage};
use super::wasm::Width;
use super::wasm_float::{
    Float, Truncation, copysign_bits, saturated, truncation_limits, unsigned_to_f32, unsigned_to_f64, widen,
};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use crate::wasm::numeric::{canonical_nan_bits, sign_mask};
use crate::WasmTrap;

pub struct ConcreteWasm;

/// `value`'s low `bits` bits, sign-extended: shifted to the top and back down arithmetically.
macro_rules! sign_extend {
    ($value:expr, $bits:expr, $signed:ty) => {
        $value
            .wrapping_shl(<$signed>::BITS - $bits)
            .wrapping_shr(<$signed>::BITS - $bits)
    };
}

/// The operators at one signed width: `$wide` and `$wide_unsigned` hold a product of two
/// `$signed` or `$unsigned` values. Division is plain: the staged traps have run.
macro_rules! integer_width {
    ($binary:ident, $unary:ident, $signed:ty, $unsigned:ty, $wide:ty, $wide_unsigned:ty) => {
        // Always inlined: a handler whose operator is a constant keeps only its arm (the match
        // left out of line cost about 10 Ir per i32 operator in the interpreter).
        #[inline(always)]
        fn $binary(operator: Binary, left: $signed, right: $signed) -> $signed {
            let (unsigned_left, unsigned_right) = (left as $unsigned, right as $unsigned);
            let amount = right as u32;
            match operator {
                Binary::Add => left.wrapping_add(right),
                Binary::Subtract => left.wrapping_sub(right),
                Binary::Multiply => left.wrapping_mul(right),
                Binary::DivideSigned => left.wrapping_div(right),
                Binary::DivideUnsigned => (unsigned_left / unsigned_right) as $signed,
                Binary::RemainderSigned => left.wrapping_rem(right),
                Binary::RemainderUnsigned => (unsigned_left % unsigned_right) as $signed,
                Binary::And => left & right,
                Binary::Or => left | right,
                Binary::Xor => left ^ right,
                Binary::ShiftLeft => left.wrapping_shl(amount),
                Binary::ShiftRightSigned => left.wrapping_shr(amount),
                Binary::ShiftRightUnsigned => unsigned_left.wrapping_shr(amount) as $signed,
                Binary::RotateLeft => left.rotate_left(amount),
                Binary::RotateRight => left.rotate_right(amount),
                Binary::Equal => <$signed>::from(left == right),
                Binary::NotEqual => <$signed>::from(left != right),
                Binary::LessSigned => <$signed>::from(left < right),
                Binary::LessUnsigned => <$signed>::from(unsigned_left < unsigned_right),
                Binary::GreaterSigned => <$signed>::from(left > right),
                Binary::GreaterUnsigned => <$signed>::from(unsigned_left > unsigned_right),
                Binary::LessEqualSigned => <$signed>::from(left <= right),
                Binary::LessEqualUnsigned => <$signed>::from(unsigned_left <= unsigned_right),
                Binary::GreaterEqualSigned => <$signed>::from(left >= right),
                Binary::GreaterEqualUnsigned => <$signed>::from(unsigned_left >= unsigned_right),
                Binary::MultiplyHighSigned => {
                    ((<$wide>::from(left) * <$wide>::from(right)) >> <$signed>::BITS) as $signed
                }
                Binary::MultiplyHighUnsigned => {
                    ((<$wide_unsigned>::from(unsigned_left)
                        * <$wide_unsigned>::from(unsigned_right))
                        >> <$unsigned>::BITS) as $signed
                }
            }
        }

        #[inline(always)]
        fn $unary(operator: Unary, value: $signed) -> $signed {
            match operator {
                Unary::EqualZero => <$signed>::from(value == 0),
                Unary::LeadingZeros => value.leading_zeros() as $signed,
                Unary::TrailingZeros => value.trailing_zeros() as $signed,
                Unary::PopulationCount => value.count_ones() as $signed,
                Unary::ExtendSigned8 => sign_extend!(value, i8::BITS, $signed),
                Unary::ExtendSigned16 => sign_extend!(value, i16::BITS, $signed),
                Unary::ExtendSigned32 => sign_extend!(value, i32::BITS, $signed),
            }
        }
    };
}

integer_width!(operate_i32, operate_unary_i32, i32, u32, i64, u64);
integer_width!(operate_i64, operate_unary_i64, i64, u64, i128, u128);

/// A rounding or root of a `$float`: the standard library's method in the runtime, `core`'s float
/// math in the stencil build, which has no standard library.
#[cfg(not(stencil_build))]
macro_rules! float_math {
    ($float:ident, $operation:ident, $value:expr) => {
        $value.$operation()
    };
}
#[cfg(stencil_build)]
macro_rules! float_math {
    ($float:ident, $operation:ident, $value:expr) => {
        core::$float::math::$operation($value)
    };
}

/// The float operators at one width, on the IEEE bits `$bits` of a `$float`, zero-extended to a
/// word; a comparison's word is its flag. The extrema are the machine's.
macro_rules! float_width {
    ($binary:ident, $unary:ident, $float:ident, $bits:ty) => {
        // Inlined: a stencil row's operator is a constant, so its one arm stays and nothing is called.
        #[inline(always)]
        fn $binary(operator: FloatBinary, left: $float, right: $float) -> u64 {
            let value: Result<$float, bool> = match operator {
                FloatBinary::Add => Ok(left + right),
                FloatBinary::Subtract => Ok(left - right),
                FloatBinary::Multiply => Ok(left * right),
                FloatBinary::Divide => Ok(left / right),
                FloatBinary::Minimum if left == right => {
                    Ok(<$float>::from_bits(left.to_bits() | right.to_bits()))
                }
                FloatBinary::Minimum => Ok(left.min(right)),
                FloatBinary::Maximum if left == right => {
                    Ok(<$float>::from_bits(left.to_bits() & right.to_bits()))
                }
                FloatBinary::Maximum => Ok(left.max(right)),
                FloatBinary::CopySign => unreachable!("CopySign is `copysign_bits`"),
                FloatBinary::Equal => Err(left == right),
                FloatBinary::NotEqual => Err(left != right),
                FloatBinary::Less => Err(left < right),
                FloatBinary::Greater => Err(left > right),
                FloatBinary::LessEqual => Err(left <= right),
                FloatBinary::GreaterEqual => Err(left >= right),
            };
            match value {
                Ok(value) => value.to_bits() as u64,
                Err(flag) => u64::from(flag),
            }
        }

        #[inline(always)]
        fn $unary(operator: FloatUnary, value: $float) -> u64 {
            const SIGN: $bits = sign_mask(<$bits>::BITS) as $bits;
            const INFINITY: $bits = <$float>::INFINITY.to_bits();
            const QUIET_NAN_BIT: $bits =
                canonical_nan_bits(<$float>::MANTISSA_DIGITS, INFINITY as u64) as $bits ^ INFINITY;
            let quieted = #[inline(always)] |value: $float| {
                if value.is_nan() {
                    value.to_bits() | QUIET_NAN_BIT
                } else {
                    value.to_bits()
                }
            };
            let bits = match operator {
                FloatUnary::Absolute => value.to_bits() & !SIGN,
                FloatUnary::Negate => value.to_bits() ^ SIGN,
                FloatUnary::Ceiling => quieted(float_math!($float, ceil, value)),
                FloatUnary::Floor => quieted(float_math!($float, floor, value)),
                FloatUnary::Truncate => quieted(float_math!($float, trunc, value)),
                FloatUnary::Nearest => quieted(float_math!($float, round_ties_even, value)),
                FloatUnary::SquareRoot => quieted(float_math!($float, sqrt, value)),
            };
            bits as u64
        }
    };
}

float_width!(operate_f32, operate_unary_f32, f32, u32);
float_width!(operate_f64, operate_unary_f64, f64, u64);

impl Machine for ConcreteWasm {
    type Word = u64;
}

crate::projection!(ConcreteWasm => Concrete);

impl MachineStage for ConcreteWasm {
    type Exit = WasmTrap;

    #[inline(always)]
    fn constant(&mut self, width: Width, value: i64) -> u64 {
        match width {
            Width::I32 => u64::from(value as u32),
            Width::I64 => value as u64,
        }
    }

    #[inline(always)]
    fn operate(&mut self, operator: Binary, width: Width, left: u64, right: u64) -> u64 {
        match width {
            Width::I32 => u64::from(operate_i32(operator, left as i32, right as i32) as u32),
            Width::I64 => operate_i64(operator, left as i64, right as i64) as u64,
        }
    }

    #[inline(always)]
    fn operate_unary(&mut self, operator: Unary, width: Width, value: u64) -> u64 {
        match width {
            Width::I32 => u64::from(operate_unary_i32(operator, value as i32) as u32),
            Width::I64 => operate_unary_i64(operator, value as i64) as u64,
        }
    }

    #[inline(always)]
    fn trap_when(&mut self, flag: u64, trap: WasmTrap) -> Result<(), WasmTrap> {
        if flag == 0 { Ok(()) } else { Err(trap) }
    }

    #[inline(always)]
    fn operate_float(&mut self, operator: FloatBinary, float: Float, left: u64, right: u64) -> u64 {
        match (operator, float) {
            (FloatBinary::CopySign, _) => copysign_bits(float, left, right),
            (_, Float::F32) => operate_f32(
                operator,
                f32::from_bits(left as u32),
                f32::from_bits(right as u32),
            ),
            (_, Float::F64) => operate_f64(operator, f64::from_bits(left), f64::from_bits(right)),
        }
    }

    #[inline(always)]
    fn operate_float_unary(&mut self, operator: FloatUnary, float: Float, value: u64) -> u64 {
        match float {
            Float::F32 => operate_unary_f32(operator, f32::from_bits(value as u32)),
            Float::F64 => operate_unary_f64(operator, f64::from_bits(value)),
        }
    }

    #[inline(always)]
    fn float_unordered(&mut self, float: Float, left: u64, right: u64) -> u64 {
        u64::from(widen(float, left).is_nan() || widen(float, right).is_nan())
    }

    #[inline(always)]
    fn float_constant(&mut self, _: Float, bits: u64) -> u64 {
        bits
    }

    #[inline(always)]
    fn select(&mut self, flag: u64, taken: u64, other: u64) -> u64 {
        if flag == 0 { other } else { taken }
    }

    #[inline(always)]
    fn truncation_overflows(&mut self, truncation: Truncation, value: u64) -> u64 {
        let truncated = float_math!(f64, trunc, widen(truncation.float, value));
        let (lower, limit) = truncation_limits(truncation.width.bits(), truncation.signed);
        u64::from(truncated < lower || truncated >= limit)
    }

    #[inline(always)]
    fn convert(&mut self, operator: ScalarConversionOperator, value: u64) -> u64 {
        use ScalarConversionOperator as Conversion;
        // A trapping truncation's operand is in range, where it truncates as it saturates.
        if let Some((truncation, _)) = operator.float_to_integer() {
            return saturated(widen(truncation.float, value), truncation.width.bits(), truncation.signed);
        }
        // A 32-bit word is its payload zero-extended, as `WasmValue::from_bits` reads it.
        let low = value as u32;
        let single = || f32::from_bits(low);
        let double = || f64::from_bits(value);
        match operator {
            Conversion::WrapI64 | Conversion::ExtendI32Unsigned => u64::from(low),
            Conversion::ExtendI32Signed => i64::from(low as i32) as u64,
            Conversion::ConvertI32SignedF32 => u64::from((low as i32 as f32).to_bits()),
            Conversion::ConvertI32UnsignedF32 => u64::from((low as f32).to_bits()),
            Conversion::ConvertI64SignedF32 => u64::from((value as i64 as f32).to_bits()),
            Conversion::ConvertI64UnsignedF32 => u64::from(unsigned_to_f32(value).to_bits()),
            Conversion::ConvertI32SignedF64 => f64::from(low as i32).to_bits(),
            Conversion::ConvertI32UnsignedF64 => f64::from(low).to_bits(),
            Conversion::ConvertI64SignedF64 => (value as i64 as f64).to_bits(),
            Conversion::ConvertI64UnsignedF64 => unsigned_to_f64(value).to_bits(),
            Conversion::PromoteF32 => f64::from(single()).to_bits(),
            Conversion::DemoteF64 => u64::from((double() as f32).to_bits()),
            Conversion::ReinterpretF32I32 | Conversion::ReinterpretI32F32 => u64::from(low),
            Conversion::ReinterpretF64I64 | Conversion::ReinterpretI64F64 => value,
            _ => unreachable!("float-to-integer conversions returned above"),
        }
    }
}

/// Methods that are `ConcreteWasm`'s: one definition, delegated.
#[macro_export]
#[doc(hidden)]
macro_rules! delegated {
    ($($name:ident($($argument:ident: $ty:ty),*) -> $result:ty;)+) => {$(
        #[inline(always)]
        fn $name(&mut self, $($argument: $ty),*) -> $result {
            $crate::staged::wasm_concrete::ConcreteWasm.$name($($argument),*)
        }
    )+};
}
#[allow(unused_imports)]
pub(crate) use delegated;

/// The operators of a `MachineStage` over `ConcreteWasm`: every method but `trap_when`, whose trap
/// the stage's own `Exit` makes. `scalar_stage` and the VM's heap stage both expand it.
#[macro_export]
#[doc(hidden)]
macro_rules! scalar_operators {
    () => {
        $crate::delegated! {
                constant(width: $crate::staged::wasm::Width, value: i64) -> u64;
                operate(operator: $crate::wasm::integer::I64BinaryOperator, width: $crate::staged::wasm::Width, left: u64, right: u64) -> u64;
                operate_unary(operator: $crate::wasm::integer::I64UnaryOperator, width: $crate::staged::wasm::Width, value: u64) -> u64;
                operate_float(operator: $crate::wasm::float::F64BinaryOperator, float: $crate::staged::wasm_float::Float, left: u64, right: u64) -> u64;
                operate_float_unary(operator: $crate::wasm::float::F64UnaryOperator, float: $crate::staged::wasm_float::Float, value: u64) -> u64;
                float_unordered(float: $crate::staged::wasm_float::Float, left: u64, right: u64) -> u64;
                float_constant(float: $crate::staged::wasm_float::Float, bits: u64) -> u64;
                select(flag: u64, taken: u64, other: u64) -> u64;
                truncation_overflows(truncation: $crate::staged::wasm_float::Truncation, value: u64) -> u64;
                convert(operator: $crate::wasm::conversion::ScalarConversionOperator, value: u64) -> u64;
            }
    };
}
pub(crate) use scalar_operators;

/// The scalar primitives of a stage over machine state: `scalar_operators`, with `trap_when` as
/// `ConcreteWasm` makes it.
macro_rules! scalar_stage {
    ($projection:ident => $($stage:ty),+) => {$(
        impl $crate::staged::machine::Machine for $stage {
            type Word = u64;
        }

        $crate::projection!($stage => $projection);

        impl $crate::staged::machine::MachineStage for $stage {
            type Exit = $crate::WasmTrap;

            $crate::scalar_operators! {}

            $crate::delegated! {
                trap_when(flag: u64, trap: $crate::WasmTrap) -> Result<(), $crate::WasmTrap>;
            }
        }
    )+};
}
pub(crate) use scalar_stage;
