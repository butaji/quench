//! The interpreter's Wasm stage: integers are the zero-extended `u64` words of their width.

use super::wasm::{WasmStage, Width};
use crate::WasmTrap;
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

pub(crate) struct ConcreteWasm;

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

impl WasmStage for ConcreteWasm {
    type Word = u64;
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
}
