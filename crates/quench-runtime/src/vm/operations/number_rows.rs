//! The Number operation rows with no runtime dependency: each operation's enum, its stage trait
//! and the stages that compute it on machine numbers. The interpreter, the native emitters and
//! the b-jit stencils (`quench-bjit/build.rs` compiles this file too) run these same rows;
//! `operations.rs` adds what needs the parser's operator names and the bytecode.

/// An int32 shift count is its low five bits (ECMAScript ToUint32 of the count, modulo 32).
pub const WORD32_SHIFT_MASK: u32 = u32::BITS - 1;

/// A stage supplies the machine operation for each semantic Number relation.
/// The relation table below is shared by interpreter values and native emitters.
pub trait NumberRelationStage<T>: crate::projection::Sealed {
    type Output;

    fn equal(&mut self, left: T, right: T) -> Self::Output;
    fn not_equal(&mut self, left: T, right: T) -> Self::Output;
    fn less_than(&mut self, left: T, right: T) -> Self::Output;
    fn less_equal(&mut self, left: T, right: T) -> Self::Output;
    fn greater_than(&mut self, left: T, right: T) -> Self::Output;
    fn greater_equal(&mut self, left: T, right: T) -> Self::Output;
}

macro_rules! number_relations {
    ($($relation:ident => [$($operator:ident $symbol:literal),+ $(,)?], $stage_method:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum NumberRelation { $($relation),+ }

        impl NumberRelation {
            #[inline(always)]
            pub fn apply<T, S: NumberRelationStage<T>>(
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
#[macro_export]
#[doc(hidden)]
macro_rules! number_relation_rows {
    ($consumer:ident) => { $consumer! {
        Equal => [Equality "==", StrictEquality "==="], equal;
        NotEqual => [Inequality "!=", StrictInequality "!=="], not_equal;
        LessThan => [LessThan "<"], less_than;
        LessEqual => [LessEqualThan "<="], less_equal;
        GreaterThan => [GreaterThan ">"], greater_than;
        GreaterEqual => [GreaterEqualThan ">="], greater_equal;
    }};
}
#[allow(unused_imports)] // the stencil build's supernode rules (`supernode_rules.rs`) name it
pub(crate) use number_relation_rows;
number_relation_rows!(number_relations);

/// The four IEEE Number arithmetic operations have one operator-to-method
/// mapping. Interpreter values and native Cranelift values supply the stage.
pub trait NumberArithmeticStage<T>: crate::projection::Sealed {
    type Output;

    fn add(&mut self, left: T, right: T) -> Self::Output;
    fn subtract(&mut self, left: T, right: T) -> Self::Output;
    fn multiply(&mut self, left: T, right: T) -> Self::Output;
    fn divide(&mut self, left: T, right: T) -> Self::Output;
}

macro_rules! number_arithmetic {
    ($($operation:ident => $operator:ident, $stage_method:ident, $specialized_instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum NumberArithmetic { $($operation),+ }

        impl NumberArithmeticStage<f64> for NumberF64Stage {
            type Output = f64;
            $(#[inline(always)]
            fn $stage_method(&mut self, left: f64, right: f64) -> f64 { left $symbol right })+
        }
        crate::projection!(CheckedInt32Stage => Concrete);
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
            pub fn apply<T, S: NumberArithmeticStage<T>>(
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
#[macro_export]
#[doc(hidden)]
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

pub trait NumberUnaryStage<T>: crate::projection::Sealed {
    type Output;

    fn plus(&mut self, value: T) -> Self::Output;
    fn negate(&mut self, value: T) -> Self::Output;
}

macro_rules! number_unary {
    ($($operation:ident => $operator:ident, $stage_method:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum NumberUnary { $($operation),+ }

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
self::number_unary_rows!(number_unary);

/// The int32 operations used by JavaScript bitwise expressions. Keeping their
/// operator mapping and operation selection in one stage lets the interpreter
/// and native emitters share the same signed/unsigned shift decisions.
pub trait NumberBitwiseStage<T>: crate::projection::Sealed {
    type Output;

    fn and(&mut self, left: T, right: T) -> Self::Output;
    fn or(&mut self, left: T, right: T) -> Self::Output;
    fn xor(&mut self, left: T, right: T) -> Self::Output;
    fn shift_left(&mut self, left: T, right: T) -> Self::Output;
    fn shift_right(&mut self, left: T, right: T) -> Self::Output;
    fn shift_right_unsigned(&mut self, left: T, right: T) -> Self::Output;
}

macro_rules! number_bitwise {
    ($($operation:ident => $operator:ident, $stage_method:ident, $symbol:literal;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum NumberBitwise { $($operation),+ }

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
#[macro_export]
#[doc(hidden)]
macro_rules! number_bitwise_rows {
    ($consumer:ident) => { $consumer! {
        And => BitwiseAnd, and, "&";
        Or => BitwiseOR, or, "|";
        Xor => BitwiseXOR, xor, "^";
        ShiftLeft => ShiftLeft, shift_left, "<<";
        ShiftRight => ShiftRight, shift_right, ">>";
        ShiftRightUnsigned => ShiftRightZeroFill, shift_right_unsigned, ">>>";
    }};
}
pub(crate) use number_bitwise_rows;
number_bitwise_rows!(number_bitwise);

pub(crate) struct I32BitwiseStage;

crate::projection!(I32BitwiseStage => Concrete);

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
pub fn int32_bitwise_bits(operation: NumberBitwise, left: i32, right: i32) -> i32 {
    operation.apply(&mut I32BitwiseStage, left, right)
}

#[inline(always)]
pub(crate) fn number_bitwise_f64_result(operation: NumberBitwise, left: i32, right: i32) -> f64 {
    let bits = operation.apply(&mut I32BitwiseStage, left, right);
    if operation.keeps_int32(bits) { f64::from(bits) } else { f64::from(bits as u32) }
}

pub struct NumberF64Stage;

crate::projection!(NumberF64Stage => Concrete);

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
pub trait NumberTruthinessStage<T>: crate::projection::Sealed {
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

pub struct PartialOrderStage;

crate::projection!(PartialOrderStage => Concrete);

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

/// The magnitude up to which every integer is a binary64 exactly: 2^53. The integral remainder
/// takes this bound in every tier (Emit's `number_machine::integral_remainder_operands` too), so
/// they admit the same operands.
pub const SAFE_INTEGER_LIMIT: i64 = 1 << 53;

/// The truncation of a binary64 word to an i64, `i64::MIN` where it is out of range or NaN. `cvttsd2si` (SSE2, the x86-64
/// baseline) is one instruction with no constant pool, so a stencil may hold it, as it may not a saturating `as`.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn truncate_i64(value: f64) -> i64 {
    // SAFETY: SSE2 is the x86-64 baseline; the conversion touches no memory.
    unsafe { core::arch::x86_64::_mm_cvttsd_si64(core::arch::x86_64::_mm_set_sd(value)) }
}

/// The truncation of a binary64 word to an i64, `i64::MIN` where it is out of range or NaN (the portable form).
#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
pub(crate) fn truncate_i64(value: f64) -> i64 {
    if value.is_nan() || value.abs() >= 9.2e18 { i64::MIN } else { value as i64 }
}

/// The integer the Number `value` is, of magnitude at most `SAFE_INTEGER_LIMIT` (`-0` as `0`): its
/// truncation converts back equal, which a fraction, NaN and the infinities fail, and a larger
/// magnitude is tested apart (`i64::MIN`, the truncation of what is out of range, is past it).
/// Integer tests and one truncation, no float constant, so a stencil may hold it. The one home of
/// "this Number is a safe integer": the remainder row, the loop kernel's int32 columns and the
/// TypeScript enum folder read it.
#[inline(always)]
pub fn safe_integer(value: f64) -> Option<i64> {
    let integer = truncate_i64(value);
    (integer as f64 == value && integer.unsigned_abs() <= SAFE_INTEGER_LIMIT as u64).then_some(integer)
}

/// The remainder of two integral Numbers of magnitude at most `SAFE_INTEGER_LIMIT`, the
/// divisor nonzero: the exact i64 remainder, which has the dividend's sign, and a zero with the
/// dividend's sign (`-0` for a negative one). `None` for any other pair (fractions, larger
/// magnitudes, NaN, infinities, a zero divisor), which only the float remainder answers. Integer
/// tests and one truncation only, no float constant, so a stencil of the Concrete stage holds it.
#[inline(always)]
pub fn integral_remainder(left: f64, right: f64) -> Option<f64> {
    let (dividend, divisor) = (safe_integer(left)?, safe_integer(right)?);
    if divisor == 0 {
        return None;
    }
    let remainder = dividend % divisor;
    Some(if remainder == 0 { f64::from_bits(left.to_bits() & (1 << 63)) } else { remainder as f64 })
}

/// Number remainder after operand conversion, the slow path compiled code calls
/// (`RuntimeSlowPath::NumberRemainder`): no VM state, allocation, collection, guest effects, or
/// integer traps. Never inlined, so it is one symbol, and its address is the same wherever it is
/// taken: an image names a slow path by matching that address against the lane helpers'
/// (`compiled::slow_path_symbol`), which an inlined function's per-unit copies defeat. Rust
/// callers take `remainder`, which inlines.
#[inline(never)]
pub extern "C" fn number_remainder(left: f64, right: f64) -> f64 {
    remainder(left, right)
}

/// `number_remainder`'s value, inlined into its Rust callers. Floating remainder preserves
/// negative zero and the Number NaN/infinity rules.
#[inline]
pub fn remainder(left: f64, right: f64) -> f64 {
    // Integral operands take the integer remainder (`integral_remainder`, ~15 instructions where
    // `fmod` costs ~170); everything else the float remainder.
    integral_remainder(left, right).unwrap_or_else(#[inline(always)] || left % right)
}

/// The parser's binary and unary operator codes, as an instruction's operator field holds them
/// (oxc's `BinaryOperator` and `UnaryOperator` discriminants), named as the parser names them.
/// Plain numbers, so the rows that build or test an operator field read them in the stencil build
/// too; `vm::operations` asserts each equals the parser's.
#[allow(non_upper_case_globals)]
pub mod binary_operator {
    pub const Equality: u32 = 0;
    pub const Inequality: u32 = 1;
    pub const StrictEquality: u32 = 2;
    pub const StrictInequality: u32 = 3;
    pub const LessThan: u32 = 4;
    pub const LessEqualThan: u32 = 5;
    pub const GreaterThan: u32 = 6;
    pub const GreaterEqualThan: u32 = 7;
    pub const Addition: u32 = 8;
    pub const Subtraction: u32 = 9;
    pub const Multiplication: u32 = 10;
    pub const Division: u32 = 11;
    pub const Remainder: u32 = 12;
    pub(crate) const Exponential: u32 = 13;
    pub const ShiftLeft: u32 = 14;
    pub const ShiftRight: u32 = 15;
    pub const ShiftRightZeroFill: u32 = 16;
    pub const BitwiseOR: u32 = 17;
    pub const BitwiseXOR: u32 = 18;
    pub const BitwiseAnd: u32 = 19;
    pub(crate) const In: u32 = 20;
    pub(crate) const Instanceof: u32 = 21;
}

/// The unary operator codes (`binary_operator`'s).
#[allow(non_upper_case_globals)]
pub(crate) mod unary_operator {
    pub(crate) const UnaryPlus: u32 = 0;
    pub(crate) const UnaryNegation: u32 = 1;
    pub(crate) const LogicalNot: u32 = 2;
    pub(crate) const BitwiseNot: u32 = 3;
    pub(crate) const Typeof: u32 = 4;
    pub(crate) const Void: u32 = 5;
    pub(crate) const Delete: u32 = 6;
}

/// The relations' operator codes: the first of each group (`Equal` and `NotEqual` name two operators each,
/// `==` and `===`, so a caller that needs one of them must not ask), and the relation a code denotes.
macro_rules! number_relation_codes {
    ($($relation:ident => [$($operator:ident $symbol:literal),+ $(,)?], $stage_method:ident;)+) => {
        impl NumberRelation {
            #[inline]
            pub fn binary_operator(self) -> u32 {
                match self {
                    $(Self::$relation => [$(binary_operator::$operator),+][0],)+
                }
            }

            #[inline(always)]
            pub fn from_operator(op: u32) -> Option<Self> {
                $(
                    if $(op == binary_operator::$operator)||+ {
                        return Some(Self::$relation);
                    }
                )+
                None
            }
        }
    };
}
number_relation_rows!(number_relation_codes);

/// The arithmetic operations' operator codes, both ways.
macro_rules! number_arithmetic_codes {
    ($($operation:ident => $operator:ident, $stage_method:ident, $specialized_instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        impl NumberArithmetic {
            /// The binary operator this arithmetic operation denotes (`from_operator`'s inverse).
            #[inline]
            pub fn binary_operator(self) -> u32 {
                match self {
                    $(Self::$operation => binary_operator::$operator,)+
                }
            }

            #[inline(always)]
            pub fn from_operator(operator: u32) -> Option<Self> {
                $(
                    if operator == binary_operator::$operator {
                        return Some(Self::$operation);
                    }
                )+
                None
            }
        }
    };
}
number_arithmetic_rows!(number_arithmetic_codes);

impl NumberArithmetic {
    /// Whether the result of two integers is an integer: all but a division.
    #[inline(always)]
    pub const fn integral(self) -> bool {
        !matches!(self, Self::Divide)
    }
}

/// The bitwise operations' operator codes, both ways.
macro_rules! number_bitwise_codes {
    ($($operation:ident => $operator:ident, $stage_method:ident, $symbol:literal;)+) => {
        impl NumberBitwise {
            /// The binary operator this bitwise operation denotes (`from_operator`'s inverse).
            #[inline]
            pub fn binary_operator(self) -> u32 {
                match self {
                    $(Self::$operation => binary_operator::$operator,)+
                }
            }

            #[inline]
            pub fn from_operator(operator: u32) -> Option<Self> {
                $(
                    if operator == binary_operator::$operator {
                        return Some(Self::$operation);
                    }
                )+
                None
            }
        }
    };
}
number_bitwise_rows!(number_bitwise_codes);

/// Whether an equality operator is strict and whether it is negated; `None` for any other.
#[inline(always)]
pub fn equality_operator(op: u32) -> Option<(bool, bool)> {
    const EQUALITY: u32 = binary_operator::Equality;
    // The four operators are consecutive from an aligned first, each negation right after
    // its test and the strict pair after the loose one, so two bits of the offset decide
    // them, with no table of branches.
    const NEGATED: u32 = binary_operator::Inequality - EQUALITY;
    const STRICT: u32 = binary_operator::StrictEquality - EQUALITY;
    const _: () = assert!(
        NEGATED == 1
            && STRICT == 2
            && binary_operator::StrictInequality - EQUALITY == STRICT | NEGATED
            && EQUALITY.is_multiple_of(STRICT << 1)
    );
    let offset = op.wrapping_sub(EQUALITY);
    (offset <= STRICT | NEGATED).then_some((offset & STRICT != 0, offset & NEGATED != 0))
}

/// The equality operator code of `(strict, negated)` (`equality_operator`'s inverse).
#[inline(always)]
pub fn equality_binary_operator(strict: bool, negated: bool) -> u32 {
    match (strict, negated) {
        (false, false) => binary_operator::Equality,
        (false, true) => binary_operator::Inequality,
        (true, false) => binary_operator::StrictEquality,
        (true, true) => binary_operator::StrictInequality,
    }
}
