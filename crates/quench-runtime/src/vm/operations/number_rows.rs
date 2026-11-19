//! The Number operation rows with no runtime dependency: each operation's enum, its stage trait
//! and the stages that compute it on machine numbers. The interpreter, the native emitters and
//! the b-jit stencils (`quench-bjit/build.rs` compiles this file too) run these same rows;
//! `operations.rs` adds what needs the parser's operator names and the bytecode.

/// An int32 shift count is its low five bits (ECMAScript ToUint32 of the count, modulo 32).
pub(crate) const WORD32_SHIFT_MASK: u32 = u32::BITS - 1;

/// A stage supplies the machine operation for each semantic Number relation.
/// The relation table below is shared by interpreter values and native emitters.
pub(crate) trait NumberRelationStage<T> {
    type Output;

    fn equal(&mut self, left: T, right: T) -> Self::Output;
    fn not_equal(&mut self, left: T, right: T) -> Self::Output;
    fn less_than(&mut self, left: T, right: T) -> Self::Output;
    fn less_equal(&mut self, left: T, right: T) -> Self::Output;
    fn greater_than(&mut self, left: T, right: T) -> Self::Output;
    fn greater_equal(&mut self, left: T, right: T) -> Self::Output;
}

macro_rules! number_relations {
    ($($relation:ident => [$($operator:ident),+ $(,)?], $stage_method:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum NumberRelation { $($relation),+ }

        impl NumberRelation {
            #[inline(always)]
            pub(crate) fn apply<T, S: NumberRelationStage<T>>(
                self,
                stage: &mut S,
                left: T,
                right: T,
            ) -> S::Output {
                match self {
                    $(Self::$relation => stage.$stage_method(left, right)),+
                }
            }

            /// A relational operator's answer for totally ordered operands; `None` for equality.
            pub(crate) fn of_ordering(self, ordering: std::cmp::Ordering) -> Option<bool> {
                Some(match self {
                    Self::LessThan => ordering.is_lt(),
                    Self::LessEqual => ordering.is_le(),
                    Self::GreaterThan => ordering.is_gt(),
                    Self::GreaterEqual => ordering.is_ge(),
                    Self::Equal | Self::NotEqual => return None,
                })
            }
        }
    };
}

/// Each relation with the operators that denote it; consumers take the table.
macro_rules! number_relation_rows {
    ($consumer:ident) => { $consumer! {
        Equal => [Equality, StrictEquality], equal;
        NotEqual => [Inequality, StrictInequality], not_equal;
        LessThan => [LessThan], less_than;
        LessEqual => [LessEqualThan], less_equal;
        GreaterThan => [GreaterThan], greater_than;
        GreaterEqual => [GreaterEqualThan], greater_equal;
    }};
}
pub(crate) use number_relation_rows;
number_relation_rows!(number_relations);

/// The four IEEE Number arithmetic operations have one operator-to-method
/// mapping. Interpreter values and native Cranelift values supply the stage.
pub(crate) trait NumberArithmeticStage<T> {
    type Output;

    fn add(&mut self, left: T, right: T) -> Self::Output;
    fn subtract(&mut self, left: T, right: T) -> Self::Output;
    fn multiply(&mut self, left: T, right: T) -> Self::Output;
    fn divide(&mut self, left: T, right: T) -> Self::Output;
}

macro_rules! number_arithmetic {
    ($($operation:ident => $operator:ident, $stage_method:ident, $specialized_instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub(crate) enum NumberArithmetic { $($operation),+ }

        impl NumberArithmeticStage<f64> for NumberF64Stage {
            type Output = f64;
            $(#[inline(always)]
            fn $stage_method(&mut self, left: f64, right: f64) -> f64 { left $symbol right })+
        }
        pub(crate) struct CheckedInt32Stage;
        impl NumberArithmeticStage<i32> for CheckedInt32Stage {
            type Output = Option<i32>;
            $(#[inline(always)]
            fn $stage_method(&mut self, left: i32, right: i32) -> Self::Output {
                number_arithmetic!(@checked left, right, $checked)
            })+
        }

        impl NumberArithmetic {
            #[inline(always)]
            pub(crate) fn apply<T, S: NumberArithmeticStage<T>>(
                self,
                stage: &mut S,
                left: T,
                right: T,
            ) -> S::Output {
                match self {
                    $(Self::$operation => stage.$stage_method(left, right)),+
                }
            }
        }
    };
    (@checked $left:ident, $right:ident, None) => {
        { let _ = ($left, $right); unreachable!("division has a Float64 result") }
    };
    (@checked $left:ident, $right:ident, $method:ident) => { $left.$method($right) };
}

// The primitive views of each Number operation, including checked signed-int32
// arithmetic. Backends consume these rows rather than maintain another mapping.
macro_rules! number_arithmetic_rows {
    ($consumer:ident) => { $consumer! {
        Add => Addition, add, NumericAdd, +, fadd, checked_add, sadd_overflow;
        Subtract => Subtraction, subtract, None, -, fsub, checked_sub, ssub_overflow;
        Multiply => Multiplication, multiply, NumericMultiply, *, fmul, checked_mul, smul_overflow;
        Divide => Division, divide, None, /, fdiv, None, None;
    }};
}
pub(crate) use number_arithmetic_rows;
number_arithmetic_rows!(number_arithmetic);

pub(crate) trait NumberUnaryStage<T> {
    type Output;

    fn plus(&mut self, value: T) -> Self::Output;
    fn negate(&mut self, value: T) -> Self::Output;
}

macro_rules! number_unary {
    ($($operation:ident => $operator:ident, $stage_method:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum NumberUnary { $($operation),+ }

        impl NumberUnary {
            #[inline(always)]
            pub(crate) fn apply<T, S: NumberUnaryStage<T>>(
                self,
                stage: &mut S,
                value: T,
            ) -> S::Output {
                match self {
                    $(Self::$operation => stage.$stage_method(value)),+
                }
            }
        }
    };
}

/// Each unary operation with the operator that denotes it; consumers take the table.
macro_rules! number_unary_rows {
    ($consumer:ident) => { $consumer! {
        Plus => UnaryPlus, plus;
        Negate => UnaryNegation, negate;
    }};
}
pub(crate) use number_unary_rows;
number_unary_rows!(number_unary);

/// The int32 operations used by JavaScript bitwise expressions. Keeping their
/// operator mapping and operation selection in one stage lets the interpreter
/// and native emitters share the same signed/unsigned shift decisions.
pub(crate) trait NumberBitwiseStage<T> {
    type Output;

    fn and(&mut self, left: T, right: T) -> Self::Output;
    fn or(&mut self, left: T, right: T) -> Self::Output;
    fn xor(&mut self, left: T, right: T) -> Self::Output;
    fn shift_left(&mut self, left: T, right: T) -> Self::Output;
    fn shift_right(&mut self, left: T, right: T) -> Self::Output;
    fn shift_right_unsigned(&mut self, left: T, right: T) -> Self::Output;
}

macro_rules! number_bitwise {
    ($($operation:ident => $operator:ident, $stage_method:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum NumberBitwise { $($operation),+ }

        impl NumberBitwise {
            #[inline(always)]
            pub(crate) fn apply<T, S: NumberBitwiseStage<T>>(
                self,
                stage: &mut S,
                left: T,
                right: T,
            ) -> S::Output {
                match self {
                    $(Self::$operation => stage.$stage_method(left, right)),+
                }
            }
        }
    };
}

/// Each bitwise operation with the operator that denotes it; consumers take the table.
macro_rules! number_bitwise_rows {
    ($consumer:ident) => { $consumer! {
        And => BitwiseAnd, and;
        Or => BitwiseOR, or;
        Xor => BitwiseXOR, xor;
        ShiftLeft => ShiftLeft, shift_left;
        ShiftRight => ShiftRight, shift_right;
        ShiftRightUnsigned => ShiftRightZeroFill, shift_right_unsigned;
    }};
}
pub(crate) use number_bitwise_rows;
number_bitwise_rows!(number_bitwise);

pub(crate) struct I32BitwiseStage;

impl NumberBitwiseStage<i32> for I32BitwiseStage {
    type Output = i32;

    #[inline(always)]
    fn and(&mut self, left: i32, right: i32) -> Self::Output {
        left & right
    }
    #[inline(always)]
    fn or(&mut self, left: i32, right: i32) -> Self::Output {
        left | right
    }
    #[inline(always)]
    fn xor(&mut self, left: i32, right: i32) -> Self::Output {
        left ^ right
    }
    #[inline(always)]
    fn shift_left(&mut self, left: i32, right: i32) -> Self::Output {
        ((left as u32).wrapping_shl((right as u32) & WORD32_SHIFT_MASK)) as i32
    }
    #[inline(always)]
    fn shift_right(&mut self, left: i32, right: i32) -> Self::Output {
        left >> ((right as u32) & WORD32_SHIFT_MASK)
    }
    #[inline(always)]
    fn shift_right_unsigned(&mut self, left: i32, right: i32) -> Self::Output {
        ((left as u32) >> ((right as u32) & WORD32_SHIFT_MASK)) as i32
    }
}

impl NumberBitwise {
    /// Whether the int32 `bits` an operation computed are its Number: all but a zero-fill
    /// shift above `i32::MAX`, whose Number is its unsigned bits.
    #[inline(always)]
    pub(crate) fn keeps_int32(self, bits: i32) -> bool {
        self != Self::ShiftRightUnsigned || bits >= 0
    }
}

/// The int32 bits `operation` makes of two int32 operands.
#[inline(always)]
pub(crate) fn int32_bitwise_bits(operation: NumberBitwise, left: i32, right: i32) -> i32 {
    operation.apply(&mut I32BitwiseStage, left, right)
}

#[inline(always)]
pub(crate) fn number_bitwise_f64_result(operation: NumberBitwise, left: i32, right: i32) -> f64 {
    let bits = operation.apply(&mut I32BitwiseStage, left, right);
    if operation.keeps_int32(bits) { f64::from(bits) } else { f64::from(bits as u32) }
}

pub(crate) struct NumberF64Stage;

impl NumberUnaryStage<f64> for NumberF64Stage {
    type Output = f64;

    #[inline(always)]
    fn plus(&mut self, value: f64) -> Self::Output {
        value
    }

    #[inline(always)]
    fn negate(&mut self, value: f64) -> Self::Output {
        -value
    }
}

/// A stage supplies predicate operations for the shared JavaScript Number ToBoolean
/// denotation. Interpreter, stencil and codegen stages use the same nonzero-and-not-NaN formula.
pub(crate) trait NumberTruthinessStage<T> {
    type Predicate;

    fn zero(&mut self) -> T;
    fn equal(&mut self, left: T, right: T) -> Self::Predicate;
    fn not_equal(&mut self, left: T, right: T) -> Self::Predicate;
    fn and(&mut self, left: Self::Predicate, right: Self::Predicate) -> Self::Predicate;
}

#[inline(always)]
pub(crate) fn number_truthiness<T: Copy, S: NumberTruthinessStage<T>>(
    stage: &mut S,
    value: T,
) -> S::Predicate {
    let zero = stage.zero();
    let nonzero = stage.not_equal(value, zero);
    let not_nan = stage.equal(value, value);
    stage.and(nonzero, not_nan)
}

impl NumberTruthinessStage<f64> for NumberF64Stage {
    type Predicate = bool;

    #[inline(always)]
    fn zero(&mut self) -> f64 {
        0.0
    }

    #[inline(always)]
    fn equal(&mut self, left: f64, right: f64) -> Self::Predicate {
        left == right
    }

    #[inline(always)]
    fn not_equal(&mut self, left: f64, right: f64) -> Self::Predicate {
        left != right
    }

    #[inline(always)]
    fn and(&mut self, left: Self::Predicate, right: Self::Predicate) -> Self::Predicate {
        left && right
    }
}

#[inline(always)]
pub(crate) fn number_is_truthy(value: f64) -> bool {
    number_truthiness(&mut NumberF64Stage, value)
}

pub(crate) struct PartialOrderStage;

impl<T: PartialOrd> NumberRelationStage<T> for PartialOrderStage {
    type Output = bool;

    #[inline(always)]
    fn equal(&mut self, left: T, right: T) -> Self::Output {
        left == right
    }
    #[inline(always)]
    fn not_equal(&mut self, left: T, right: T) -> Self::Output {
        left != right
    }
    #[inline(always)]
    fn less_than(&mut self, left: T, right: T) -> Self::Output {
        left < right
    }
    #[inline(always)]
    fn less_equal(&mut self, left: T, right: T) -> Self::Output {
        left <= right
    }
    #[inline(always)]
    fn greater_than(&mut self, left: T, right: T) -> Self::Output {
        left > right
    }
    #[inline(always)]
    fn greater_equal(&mut self, left: T, right: T) -> Self::Output {
        left >= right
    }
}

/// `number_remainder` on lane words (each a binary64's bits): the shape a linked body calls
/// through the helper table, where an address of this process's function cannot be named.
#[inline]
pub(crate) extern "C" fn number_remainder_words(_vm: i64, _residual: i64, left: i64, right: i64) -> i64 {
    number_remainder(f64::from_bits(left as u64), f64::from_bits(right as u64)).to_bits() as i64
}

/// Number remainder after operand conversion. Also callable by the shared native
/// emitter: no VM state, allocation, collection, guest effects, or integer traps.
/// Floating remainder preserves negative zero and the Number NaN/infinity rules.
#[inline]
pub(crate) extern "C" fn number_remainder(left: f64, right: f64) -> f64 {
    // Integral operands inside i64 (the common `(a * k) % m` past int32) take the integer
    // remainder, which is exact: a double remainder is always representable and has the
    // dividend's sign, as `%` on i64 does. `fmod` costs ~170 instructions; this ~15. A zero
    // remainder keeps the dividend's sign (`-0`). Everything else (fractions, magnitudes
    // past 2^63, NaN, infinities, a zero divisor) is the float remainder.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if left.abs() < LIMIT && right.abs() < LIMIT {
        let (dividend, divisor) = (left as i64, right as i64);
        if dividend as f64 == left && divisor as f64 == right && divisor != 0 {
            let remainder = dividend % divisor;
            return if remainder == 0 { if left.is_sign_negative() { -0.0 } else { 0.0 } } else { remainder as f64 };
        }
    }
    left % right
}
