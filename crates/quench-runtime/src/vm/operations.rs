use super::*;
use crate::value::NumericPair;
use oxc_ast::ast::{BinaryOperator, UnaryOperator};

mod number_operators;
mod primitive_comparison;
pub use number_operators::static_number_binary;
#[quench_macros::test_visible]
pub(super) use number_operators::immediate_number_pair;
use number_operators::*;
mod number_rows;
pub use number_rows::*;

// The operator codes the shared rows name (`number_rows::binary_operator`, `unary_operator`) are the parser's.
const _: () = assert!(binary_operator::Equality == BinaryOperator::Equality as u32
    && binary_operator::Inequality == BinaryOperator::Inequality as u32
    && binary_operator::StrictEquality == BinaryOperator::StrictEquality as u32
    && binary_operator::StrictInequality == BinaryOperator::StrictInequality as u32
    && binary_operator::LessThan == BinaryOperator::LessThan as u32
    && binary_operator::LessEqualThan == BinaryOperator::LessEqualThan as u32
    && binary_operator::GreaterThan == BinaryOperator::GreaterThan as u32
    && binary_operator::GreaterEqualThan == BinaryOperator::GreaterEqualThan as u32
    && binary_operator::Addition == BinaryOperator::Addition as u32
    && binary_operator::Subtraction == BinaryOperator::Subtraction as u32
    && binary_operator::Multiplication == BinaryOperator::Multiplication as u32
    && binary_operator::Division == BinaryOperator::Division as u32
    && binary_operator::Remainder == BinaryOperator::Remainder as u32
    && binary_operator::Exponential == BinaryOperator::Exponential as u32
    && binary_operator::ShiftLeft == BinaryOperator::ShiftLeft as u32
    && binary_operator::ShiftRight == BinaryOperator::ShiftRight as u32
    && binary_operator::ShiftRightZeroFill == BinaryOperator::ShiftRightZeroFill as u32
    && binary_operator::BitwiseOR == BinaryOperator::BitwiseOR as u32
    && binary_operator::BitwiseXOR == BinaryOperator::BitwiseXOR as u32
    && binary_operator::BitwiseAnd == BinaryOperator::BitwiseAnd as u32
    && binary_operator::In == BinaryOperator::In as u32
    && binary_operator::Instanceof == BinaryOperator::Instanceof as u32);
const _: () = assert!(unary_operator::UnaryPlus == UnaryOperator::UnaryPlus as u32
    && unary_operator::UnaryNegation == UnaryOperator::UnaryNegation as u32
    && unary_operator::LogicalNot == UnaryOperator::LogicalNot as u32
    && unary_operator::BitwiseNot == UnaryOperator::BitwiseNot as u32
    && unary_operator::Typeof == UnaryOperator::Typeof as u32
    && unary_operator::Void == UnaryOperator::Void as u32
    && unary_operator::Delete == UnaryOperator::Delete as u32);

/// Shared Number/BigInt content mismatch diagnostic.
pub(super) const MIXED_BIGINT_TYPES: &str =
    "Cannot mix BigInt and other types, use explicit conversions";

pub(super) enum ArrayLikeElementKind {
    Any,
    PropertyKey,
}

/// The comparison operators occupy the binary operator discriminants up to this one.
pub const LAST_COMPARISON_OPERATOR: u32 = BinaryOperator::GreaterEqualThan as u32;


macro_rules! number_arithmetic_operators {
    ($($operation:ident => $operator:ident, $stage_method:ident, $specialized_instruction:ident, $symbol:tt, $float:ident, $checked:ident, $overflow:ident;)+) => {
        impl NumberArithmetic {
            /// The Number arithmetic operation the decoded instruction denotes: the `Arithmetic`
            /// of `Operation::Number`, which `Binary` decodes its arithmetic operators to
            /// (`staged::numeric::binary_operation`). No record of another opcode denotes one.
            pub(crate) fn from_instruction(instruction: crate::bytecode::WideInstruction) -> Option<Self> {
                match crate::staged::Operation::<u64, ()>::of(instruction)? {
                    crate::staged::Operation::Number(crate::staged::NumberOperation::Arithmetic { operation, .. }) => Some(operation),
                    _ => None,
                }
            }

            /// `from_instruction` of an opcode and its operator field, for a caller that holds
            /// the opcode but no record: the record is built with the operator in its immediate.
            pub fn from_js_instruction(instruction: Op, operator: u32) -> Option<Self> {
                Self::from_instruction(crate::bytecode::WideInstruction::new(instruction, 0, 0, 0, operator))
            }
        }
    };
}
number_arithmetic_rows!(number_arithmetic_operators);

impl NumberArithmetic {
    /// For signed-int32 inputs, addition and subtraction are exact binary64
    /// integers; a following ToInt32 therefore keeps their wrapping low bits.
    /// Multiplication can round before ToInt32 and does not have this law.
    pub const fn int32_projection_is_wrapping(self) -> bool {
        matches!(self, Self::Add | Self::Subtract)
    }
}

#[cfg(any(test, feature = "test-support"))]
/// Whether swapping two already-evaluated operands preserves the canonical Number
/// result. This is not a law for evaluating/coercing guest operands or string addition.
#[inline(always)]
pub fn number_result_is_commutative(operator: u32) -> bool {
    matches!(NumberArithmetic::from_operator(operator), Some(NumberArithmetic::Add | NumberArithmetic::Multiply))
}

/// Sign of a non-overflowing zero product: at least one factor is zero, so
/// the result is -0 exactly when either factor is negative. The caller must
/// establish zero before interpreting this predicate; views supply primitives.
macro_rules! int32_zero_product_sign {
    ($left:expr, $right:expr, $or:expr, $negative:expr) => {{
        let signs = ($or)($left, $right);
        ($negative)(signs)
    }};
}

macro_rules! number_unary_operators {
    ($($operation:ident => $operator:ident, $stage_method:ident;)+) => {
        impl NumberUnary {
            #[inline(always)]
            pub(super) fn from_operator(operator: u32) -> Option<Self> {
                $(
                    if operator == UnaryOperator::$operator as u32 {
                        return Some(Self::$operation);
                    }
                )+
                None
            }

            /// The side-effect-free Number unary operation denoted by JavaScript bytecode.
            /// Other unary operators still require their normal coercion and effect paths.
            pub fn from_js_instruction(instruction: Op, operator: u32) -> Option<Self> {
                (instruction as u16 == crate::staged::UNARY_ROW)
                    .then(|| Self::from_operator(operator))
                    .flatten()
            }
        }
    };
}
number_unary_rows!(number_unary_operators);

/// `number_bitwise_rows` rows' operators: which binary operator denotes each operation.
macro_rules! number_bitwise_operators {
    ($($operation:ident => $operator:ident, $stage_method:ident, $symbol:literal;)+) => {
        impl NumberBitwise {
            /// `from_operator` of a decoded operator, as one match.
            #[inline(always)]
            pub(crate) fn from_binary_operator(operator: BinaryOperator) -> Option<Self> {
                match operator {
                    $(BinaryOperator::$operator => Some(Self::$operation),)+
                    _ => None,
                }
            }

            /// The signed int32 operation the decoded instruction denotes: the `Bitwise` of
            /// `Operation::Number`, which `Binary` decodes its bitwise operators to.
            pub(crate) fn from_instruction(instruction: crate::bytecode::WideInstruction) -> Option<Self> {
                match crate::staged::Operation::<u64, ()>::of(instruction)? {
                    crate::staged::Operation::Number(crate::staged::NumberOperation::Bitwise { operation, .. }) => Some(operation),
                    _ => None,
                }
            }

            /// `from_instruction` of an opcode and its operator field, for a caller that holds
            /// the opcode but no record.
            pub fn from_js_instruction(instruction: Op, operator: u32) -> Option<Self> {
                Self::from_instruction(crate::bytecode::WideInstruction::new(instruction, 0, 0, 0, operator))
            }
        }
    };
}
number_bitwise_rows!(number_bitwise_operators);

impl NumberBitwise {
    /// A nonzero effective zero-fill shift clears the sign bit. All other
    /// bitwise operations already return signed Word32 values.
    pub fn result_is_int32(self, right: Option<f64>) -> bool {
        self != Self::ShiftRightUnsigned || right.is_some_and(|right|
            crate::value::number_to_u32(right) & crate::value::WORD32_SHIFT_MASK != 0)
    }
}

#[cfg(test)]
/// `OP` on two int32 operands when its Number result is again an int32 (no overflow, no
/// -0, no `>>>` past 2^31): the raw i32, so a fused second operator reads it without a tag
/// test. `None` leaves the case to `integer_operator`.
#[inline(always)]
pub(super) fn int32_operator<const OP: u32>(left: i32, right: i32) -> Option<i32> {
    match numeric_binary_operator(OP)? {
        BinaryOperator::Addition => left.checked_add(right),
        BinaryOperator::Subtraction => left.checked_sub(right),
        BinaryOperator::Multiplication => {
            let product = left.checked_mul(right)?;
            // 0 from a negative factor is -0, not an int32.
            (product != 0 || (left | right) >= 0).then_some(product)
        }
        operator => {
            let operation = NumberBitwise::from_binary_operator(operator)?;
            let bits = operation.apply(&mut I32BitwiseStage, left, right);
            (operation != NumberBitwise::ShiftRightUnsigned || bits >= 0).then_some(bits)
        }
    }
}

/// `operation` of int32 operands as a value: the int32 result as it is, but for a zero-fill
/// shift above `i32::MAX`, which is the Number of its unsigned bits.
#[inline(always)]
pub fn number_bitwise_result(operation: NumberBitwise, left: i32, right: i32) -> Value {
    let bits = operation.apply(&mut I32BitwiseStage, left, right);
    if operation.keeps_int32(bits) {
        Value::integer(bits)
    } else {
        Value::number(f64::from(bits as u32))
    }
}

#[inline(always)]
pub fn number_unary_result(operator: u32, value: f64) -> Option<f64> {
    Some(NumberUnary::from_operator(operator)?.apply(&mut NumberF64Stage, value))
}

#[inline(always)]
fn number_arithmetic_result(operation: NumberArithmetic, left: f64, right: f64) -> f64 {
    operation.apply(&mut NumberF64Stage, left, right)
}

/// Binary operator `op` of two integers where the integers alone decide its result: a
/// comparison's boolean through the shared relation definition, or the integer operators'
/// result (`numeric_integer_result`); `None` for any other operator and for a result the
/// integer case leaves to Numbers.
#[inline(always)]
pub fn integer_binary(op: u32, left: i32, right: i32) -> Option<Value> {
    if let Some(relation) = NumberRelation::from_operator(op) {
        return Some(if relation.apply(&mut PartialOrderStage, left, right) {
            Value::TRUE
        } else {
            Value::FALSE
        });
    }
    numeric_integer_result(numeric_binary_operator(op)?, left, right)
}

/// A comparison operator on two Numbers, comparing integers as integers, with the value path
/// that decided it; `None` for any other operator or value.
#[inline(always)]
pub fn number_relation(
    op: u32,
    left: Value,
    right: Value,
) -> Option<(bool, crate::profile::BinaryValuePath)> {
    if op > LAST_COMPARISON_OPERATOR {
        return None;
    }
    let pair = Value::numeric_pair(left, right);
    if let NumericPair::Ints(a, b) = pair {
        return Some((number_comparison(op, a, b), crate::profile::BinaryValuePath::IntegerFastPath));
    }
    // Equality, strict equality and the relational operators agree with IEEE 754
    // comparison on numbers: NaN is unordered and unequal, and -0 equals +0.
    let (a, b) = pair.as_doubles()?;
    Some((number_comparison(op, a, b), crate::profile::BinaryValuePath::NumberFastPath))
}

#[cfg(test)]
mod numeric_instruction_tests {
    use super::{NumberArithmetic, NumberBitwise, NumberF64Stage, NumberUnary, Op, number_truthiness};
    use oxc_ast::ast::{BinaryOperator, UnaryOperator};

    #[test]
    fn arithmetic_bytecode_denotations_come_from_the_operation_table() {
        assert_eq!(
            NumberArithmetic::from_js_instruction(Op::Binary, BinaryOperator::Addition as u32),
            Some(NumberArithmetic::Add)
        );
        // The specialized opcodes decode to the Number operation of their own operator (`NumericAdd`
        // is `+`, `NumericMultiply` is `*`); any other operator is none.
        assert_eq!(
            NumberArithmetic::from_js_instruction(Op::NumericAdd, BinaryOperator::Addition as u32),
            Some(NumberArithmetic::Add)
        );
        assert_eq!(
            NumberArithmetic::from_js_instruction(
                Op::NumericMultiply,
                BinaryOperator::Multiplication as u32
            ),
            Some(NumberArithmetic::Multiply)
        );
        assert_eq!(
            NumberArithmetic::from_js_instruction(
                Op::NumericAdd,
                BinaryOperator::Multiplication as u32
            ),
            None
        );
        assert_eq!(
            NumberArithmetic::from_js_instruction(
                Op::NumericMultiply,
                BinaryOperator::Addition as u32
            ),
            None
        );
        assert_eq!(
            NumberArithmetic::from_js_instruction(Op::Binary, BinaryOperator::Subtraction as u32),
            Some(NumberArithmetic::Subtract)
        );
        assert_eq!(
            NumberArithmetic::from_js_instruction(Op::Binary, BinaryOperator::Division as u32),
            Some(NumberArithmetic::Divide)
        );
        assert_eq!(
            NumberArithmetic::from_js_instruction(Op::Return, BinaryOperator::Addition as u32),
            None
        );
    }

    #[test]
    fn unsigned_shift_result_range_uses_effective_ecmascript_shift_count() {
        for (right, fits) in [(0.0, false), (-0.0, false), (1.0, true), (17.9, true),
            (31.0, true), (32.0, false), (33.0, true), (-1.0, true), (-32.0, false),
            (4_294_967_297.0, true), (f64::NAN, false), (f64::INFINITY, false),
            (f64::NEG_INFINITY, false)] {
            let operation = NumberBitwise::ShiftRightUnsigned;
            assert_eq!(operation.result_is_int32(Some(right)), fits, "{right}");
            let result = super::number_bitwise_f64_result(operation, -1, crate::value::number_to_i32(right));
            assert_eq!(result <= f64::from(i32::MAX), fits, "{right}");
        }
        assert!(!NumberBitwise::ShiftRightUnsigned.result_is_int32(None));
    }

    #[test]
    fn int32_bitwise_bytecodes_are_selected_only_from_general_binary() {
        for (operator, operation) in [
            (BinaryOperator::BitwiseAnd, NumberBitwise::And),
            (BinaryOperator::BitwiseOR, NumberBitwise::Or),
            (BinaryOperator::BitwiseXOR, NumberBitwise::Xor),
            (BinaryOperator::ShiftLeft, NumberBitwise::ShiftLeft),
            (BinaryOperator::ShiftRight, NumberBitwise::ShiftRight),
            (
                BinaryOperator::ShiftRightZeroFill,
                NumberBitwise::ShiftRightUnsigned,
            ),
        ] {
            assert_eq!(
                NumberBitwise::from_js_instruction(Op::Binary, operator as u32),
                Some(operation)
            );
        }
        assert_eq!(
            NumberBitwise::from_js_instruction(Op::NumericAdd, BinaryOperator::BitwiseXOR as u32),
            None
        );
    }

    #[test]
    fn number_unary_denotations_select_only_plus_and_negation() {
        assert_eq!(
            NumberUnary::from_js_instruction(Op::Unary, UnaryOperator::UnaryPlus as u32),
            Some(NumberUnary::Plus)
        );
        assert_eq!(
            NumberUnary::from_js_instruction(Op::Unary, UnaryOperator::UnaryNegation as u32),
            Some(NumberUnary::Negate)
        );
        assert_eq!(
            NumberUnary::from_js_instruction(Op::Unary, UnaryOperator::BitwiseNot as u32),
            None
        );
        assert_eq!(
            NumberUnary::from_js_instruction(Op::Binary, UnaryOperator::UnaryNegation as u32),
            None
        );
    }

    #[test]
    fn number_to_boolean_uses_nonzero_and_not_nan_for_both_signs_and_infinities() {
        for (value, expected) in [
            (0.0, false),
            (-0.0, false),
            (f64::NAN, false),
            (f64::INFINITY, true),
            (f64::NEG_INFINITY, true),
            (f64::MIN_POSITIVE, true),
            (-1.0, true),
        ] {
            assert_eq!(number_truthiness(&mut NumberF64Stage, value), expected);
        }
    }
}

#[inline(always)]
fn numeric_integer_arithmetic_result(operation: NumberArithmetic, left: i32, right: i32) -> Value {
    match operation.apply(&mut CheckedInt32Stage, left, right) {
        Some(0) if operation == NumberArithmetic::Multiply
            && int32_zero_product_sign!(left, right, |l, r| l | r, |signs| signs < 0) =>
                Value::number(-0.0),
        Some(integer) => Value::integer(integer),
        None => Value::overflowed_integer(number_arithmetic_result(operation, f64::from(left), f64::from(right))),
    }
}

#[cfg(test)]
mod integer_arithmetic_tests {
    use super::*;

    #[test]
    fn checked_integer_results_match_general_number_arithmetic() {
        fn check<const OP: u32>(left: i32, right: i32) {
            let actual = integer_operator::<OP>(Value::integer(left), Value::integer(right)).unwrap();
            let expected =
                Value::number(number_operator::<OP>(f64::from(left), f64::from(right)).unwrap());
            assert_eq!(actual, expected, "operator={OP} left={left} right={right}");
            if let Some(raw) = int32_operator::<OP>(left, right) {
                assert_eq!(Value::integer(raw), actual, "int32 operator={OP} left={left} right={right}");
            } else {
                assert!(actual.as_int().is_none(), "int32 declined operator={OP} left={left} right={right}");
            }
        }
        fn pair(left: i32, right: i32) {
            check::<ADDITION>(left, right);
            check::<SUBTRACTION>(left, right);
            check::<MULTIPLICATION>(left, right);
        }
        let square_boundary = i32::MAX.isqrt();
        let edges = [
            i32::MIN,
            i32::MIN + 1,
            -square_boundary - 1,
            -square_boundary,
            -1,
            0,
            1,
            square_boundary,
            square_boundary + 1,
            i32::MAX - 1,
            i32::MAX,
        ];
        for left in edges {
            for right in edges {
                pair(left, right);
            }
        }
        const SAMPLE_COUNT: usize = 100_000;
        const SAMPLE_STEP: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut bits = 0_u64;
        for _ in 0..SAMPLE_COUNT {
            bits = bits.wrapping_add(SAMPLE_STEP);
            pair(bits as i32, (bits >> i32::BITS) as i32);
        }
    }
}

/// An arithmetic operator on two Numbers: `integer` when both are integers, else `number` on
/// their IEEE 754 values.
pub const ADDITION: u32 = BinaryOperator::Addition as u32;
pub const SUBTRACTION: u32 = BinaryOperator::Subtraction as u32;
pub const MULTIPLICATION: u32 = BinaryOperator::Multiplication as u32;
pub const DIVISION: u32 = BinaryOperator::Division as u32;

pub const REMAINDER: u32 = BinaryOperator::Remainder as u32;
pub const SHIFT_LEFT: u32 = BinaryOperator::ShiftLeft as u32;
pub const SHIFT_RIGHT: u32 = BinaryOperator::ShiftRight as u32;
pub const SHIFT_RIGHT_ZERO_FILL: u32 = BinaryOperator::ShiftRightZeroFill as u32;
pub const BITWISE_OR: u32 = BinaryOperator::BitwiseOR as u32;
pub const BITWISE_XOR: u32 = BinaryOperator::BitwiseXOR as u32;
pub const BITWISE_AND: u32 = BinaryOperator::BitwiseAnd as u32;

/// Operator `OP` on two Numbers through their double values, as ECMA-262 defines it (an
/// immediate primitive as its ToNumeric, `immediate_number_pair`); `None` for any other.
#[inline(always)]
pub fn number_value_operator<const OP: u32>(left: Value, right: Value) -> Option<Value> {
    let pair = Value::numeric_pair(left, right);
    if let Some(value) = bitwise_value_operator::<OP>(pair) {
        return Some(value);
    }
    let (left, right) = match pair.as_doubles() {
        Some(doubles) => doubles,
        None => immediate_number_pair(left, right)?,
    };
    number_operator::<OP>(left, right).map(Value::number)
}

#[quench_macros::test_visible]
/// Operator `OP` on two Number values, as `number_value_operator` computes it after reading
/// its operands' doubles; `None` for an operator that is not numeric.
#[inline(always)]
pub(super) fn number_operator<const OP: u32>(left: f64, right: f64) -> Option<f64> {
    Some(numeric_number_result(numeric_binary_operator(OP)?, left, right))
}

/// A bitwise operator `OP` of two Numbers, each taken to int32 from its own representation
/// (an integer as it is, a double by ToInt32), its result made as `number_bitwise_result`
/// makes it, without the round trip through doubles `numeric_number_result` takes. `None`
/// for any other operator, which folds away for a constant `OP`, and for other operands.
#[inline(always)]
fn bitwise_value_operator<const OP: u32>(pair: NumericPair) -> Option<Value> {
    let operation = NumberBitwise::from_binary_operator(numeric_binary_operator(OP)?)?;
    // An integer operand is its own ToInt32; a double is taken by `number_to_i32`. An int32
    // widened to a double is an integer again under `number_to_i32`, so `Numbers` agrees.
    let (left, right) = match pair {
        NumericPair::Ints(left, right) => (left, right),
        NumericPair::Numbers(left, right) => {
            (crate::value::number_to_i32(left), crate::value::number_to_i32(right))
        }
        NumericPair::Other => return None,
    };
    Some(number_bitwise_result(operation, left, right))
}

/// Integer operator `OP` (`numeric_integer_result`) when both operands are integers;
/// `None` otherwise, leaving other Numbers to `number_binary`.
#[inline(always)]
pub fn integer_operator<const OP: u32>(left: Value, right: Value) -> Option<Value> {
    // One operand at a time: each tag test needs a single scratch register, which keeps the
    // lane's integer handlers free of saved registers.
    let (left, right) = (left.as_int()?, right.as_int()?);
    numeric_integer_result(numeric_binary_operator(OP)?, left, right)
}

#[inline(always)]
fn number_arithmetic_values(
    operation: NumberArithmetic,
    pair: NumericPair,
) -> Option<(Value, crate::profile::BinaryValuePath)> {
    if operation != NumberArithmetic::Divide
        && let NumericPair::Ints(left, right) = pair
    {
        let value = numeric_integer_arithmetic_result(operation, left, right);
        return Some((value, crate::profile::BinaryValuePath::IntegerFastPath));
    }
    let (left, right) = pair.as_doubles()?;
    Some((
        Value::number(number_arithmetic_result(operation, left, right)),
        crate::profile::BinaryValuePath::NumberFastPath,
    ))
}

#[inline(always)]
fn numeric_integer_result(operator: BinaryOperator, left: i32, right: i32) -> Option<Value> {
    Some(match operator {
        BinaryOperator::Addition => numeric_integer_arithmetic_result(NumberArithmetic::Add, left, right),
        BinaryOperator::Subtraction => numeric_integer_arithmetic_result(NumberArithmetic::Subtract, left, right),
        BinaryOperator::Multiplication => numeric_integer_arithmetic_result(NumberArithmetic::Multiply, left, right),
        // The Number operations' own values: `Value::number` makes an integral quotient or remainder the
        // integer it is, and -0 the double it is.
        BinaryOperator::Division => Value::number(number_arithmetic_result(NumberArithmetic::Divide, f64::from(left), f64::from(right))),
        BinaryOperator::Remainder => Value::number(number_remainder(f64::from(left), f64::from(right))),
        BinaryOperator::Exponential => return None,
        operator => number_bitwise_result(
            NumberBitwise::from_binary_operator(operator)
                .expect("numeric operator table only contains numeric operators"),
            left,
            right,
        ),
    })
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum RelationalOperator {
    LessThan = BinaryOperator::LessThan as u32,
    LessEqual = BinaryOperator::LessEqualThan as u32,
    GreaterThan = BinaryOperator::GreaterThan as u32,
    GreaterEqual = BinaryOperator::GreaterEqualThan as u32,
}

impl RelationalOperator {
    pub(super) fn from_immediate(immediate: u32) -> Option<Self> {
        Some(match immediate {
            value if value == Self::LessThan as u32 => Self::LessThan,
            value if value == Self::LessEqual as u32 => Self::LessEqual,
            value if value == Self::GreaterThan as u32 => Self::GreaterThan,
            value if value == Self::GreaterEqual as u32 => Self::GreaterEqual,
            _ => return None,
        })
    }

    pub(super) fn matches(self, ordering: std::cmp::Ordering) -> bool {
        use std::cmp::Ordering::{Equal, Greater, Less};
        matches!(
            (self, ordering),
            (Self::LessThan, Less)
                | (Self::LessEqual, Less | Equal)
                | (Self::GreaterThan, Greater)
                | (Self::GreaterEqual, Greater | Equal)
        )
    }

    #[inline(always)]
    fn compare_numbers(self, left: f64, right: f64) -> bool {
        left.partial_cmp(&right)
            .is_some_and(|ordering| self.matches(ordering))
    }
}

pub(super) enum OperandCoercion {
    PrimitiveDefault,
    PrimitiveNumber,
    Numeric,
}

impl Vm {
    #[inline(always)]
    pub(super) fn record_binary_value_path(
        &mut self,
        operator: u32,
        left: Value,
        right: Value,
        path: crate::profile::BinaryValuePath,
    ) {
        #[cfg(feature = "profile-aggregate")]
        self.profile.binary_value_path(
            operator as usize,
            left.profile_kind(),
            right.profile_kind(),
            path,
        );
        #[cfg(not(feature = "profile-aggregate"))]
        let _ = (operator, left, right, path);
    }

    /// `native` called with `this` and `args`, rooting both for the native's run: the entry
    /// for callers whose arguments nothing else holds (the embedding API, host callbacks,
    /// natives calling natives, tests).
    #[inline(always)]
    pub(super) fn call_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.call_native_rooting::<true>(p, native, this, args)
    }

    /// `call_native` whose caller keeps every value of `args` reachable for the native's whole
    /// run, so only the receiver is rooted here: a frame parked at the call (`pc_is_resume`),
    /// which the collector scans at the call's own liveness (`Frame::source_instruction_pc`),
    /// holding them in its argument registers, or a caller that roots them itself
    /// (`call_value`, `spread_call`, a job's roots). The receiver stays rooted: a method
    /// site's receiver may be a value its path loaded (`CallThisMethod`), held by no register.
    #[inline(always)]
    pub(super) fn call_native_arguments_held(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.call_native_rooting::<false>(p, native, this, args)
    }

    #[inline(always)]
    fn call_native_rooting<const ROOT_ARGUMENTS: bool>(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.check_execution_interrupt()?;
        crate::clock::note_step();
        // A Number-only unary Math call neither allocates nor enters guest code. Its
        // ignored receiver and trailing arguments need no roots on that path.
        let result = if let Some(value) =
            native.is_math_function().then(|| super::number::math_unary_call(native, args)).flatten()
        {
            Ok(value)
        } else {
            // The receiver, and the arguments unless the caller holds them
            // (`call_native_arguments_held`), are rooted for the native's run: the values may
            // be a copy only this call holds. Pushed as a slice: the chained iterator's extend
            // cost each call a callback per value (`call_value_with_target`).
            let start = self.active_call_roots.len();
            if ROOT_ARGUMENTS {
                self.active_call_roots.reserve(args.len() + 2);
                self.active_call_roots.push(this);
                self.active_call_roots.extend_from_slice(args);
            } else {
                self.active_call_roots.push(this);
            }
            let result = self.string_method_receiver(p, native, this).and_then(|receiver| {
                if receiver != this {
                    // A boxed receiver only this call holds.
                    self.active_call_roots.push(receiver);
                }
                self.call_native_dispatch(p, native, receiver, args)
            });
            self.active_call_roots.truncate(start);
            result
        };
        // String natives make text on demand; charge it before the caller's next safepoint.
        self.heap.charge_materialized_text();
        if native.searches_regexps() {
            return self.regexp_search_outcome(result);
        }
        result
    }

    /// The family whose entry runs `native` (`NativeFamily`): the native alone decides it, so
    /// each native's is derived once, on its first call, and read after.
    #[inline(always)]
    fn native_family_of(&mut self, native: Native) -> NativeFamily {
        let index = native as usize;
        if let Some(&family) = self.native_families.get(index)
            && family != NativeFamily::Unresolved
        {
            return family;
        }
        self.resolve_native_family(native)
    }

    #[cold]
    #[inline(never)]
    fn resolve_native_family(&mut self, native: Native) -> NativeFamily {
        let family = Self::native_family(native);
        let index = native as usize;
        if self.native_families.len() <= index {
            self.native_families.resize(index + 1, NativeFamily::Unresolved);
        }
        self.native_families[index] = family;
        family
    }

    #[inline(always)]
    fn call_native_dispatch(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match self.native_family_of(native) {
            NativeFamily::WasmExport => self.call_wasm_exported_function(p, args),
            NativeFamily::Generator => self.generator_prototype_method(p, native, this, args),
            NativeFamily::ArrayCallback => self.array_callback_native(p, native, this, args),
            NativeFamily::StringHtml => self
                .string_html_method(p, native, this, args)
                .expect("an HTML method's family is its own"),
            NativeFamily::Finalization => self.call_finalization_registry_native(p, native, this, args),
            NativeFamily::Disposal => self.call_disposal_native(p, native, this, args),
            NativeFamily::Collection => self.call_collection_native(p, native, this, args),
            #[cfg(feature = "wasm")]
            NativeFamily::WasmJs => self.call_wasm_js_routed(p, native, this, args),
            #[cfg(not(feature = "wasm"))]
            NativeFamily::WasmJs => Err(self.type_error(p, "WebAssembly is unavailable in this build".into())),
            NativeFamily::ShadowRealm => self.shadow_realm_native(p, native, this, args),
            NativeFamily::ShadowRealmCalled => {
                Err(self.type_error(p, "ShadowRealm constructor requires 'new'".into()))
            }
            NativeFamily::WeakCalled => {
                let constructor = match native {
                    Native::WeakMap => "WeakMap",
                    Native::WeakSet => "WeakSet",
                    _ => "WeakRef",
                };
                Err(self.type_error(p, format!("Constructor {constructor} requires 'new'")))
            }
            NativeFamily::TypedArray => self
                .maybe_call_typed_array_native(p, native, this, args)
                .expect("a typed array method's family is its own"),
            NativeFamily::TypedArrayCalled => {
                Err(self.type_error(p, "typed array constructor requires new".into()))
            }
            NativeFamily::Atomics => self.atomics_native(p, native, args),
            NativeFamily::Test262Agent => self.call_test262_agent(p, args),
            NativeFamily::BigInt => self.bigint_native(p, native, this, args),
            NativeFamily::Promise => self.call_promise_native(p, native, this, args),
            NativeFamily::AsyncReaction => self.call_async_reaction_native(p, native, args),
            NativeFamily::ObjectStatic => self.call_object_native(p, native, args),
            NativeFamily::LazyDataProperty => self.lazy_data_property_access(p, this, args),
            NativeFamily::ProxyRevoke => {
                let callee = self
                    .active_native_callable()
                    .ok_or_else(|| JsError("proxy revoke requires an active callee".into()))?;
                self.proxy_revoke(callee)
            }
            NativeFamily::DataView => self.data_view_native(p, native, this, args),
            NativeFamily::Table | NativeFamily::Unresolved => self.call_native_table(p, native, this, args),
        }
    }

    /// The reaction natives of async iteration (`NativeFamily::AsyncReaction`), by kind.
    fn call_async_reaction_native(&mut self, p: &ResidualProgram, native: Native, args: &[Value]) -> Result<Value, JsError> {
        match native {
            Native::AsyncFromSyncValue => self.async_from_sync_value(args),
            Native::AsyncFromSyncValueRejected => self.async_from_sync_value_rejected(p, args),
            Native::AsyncGeneratorReturnFulfilled => self.async_generator_return_fulfilled(p, args),
            Native::AsyncGeneratorReturnRejected => self.async_generator_return_rejected(p, args),
            _ => self.async_generator_delegate_return_start(p, args),
        }
    }

    /// The natives no family above claims, by kind.
    fn call_native_table(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::MathInitialize => self.install_math(p).map(|()| Value::UNDEFINED),
            Native::AtomicsInitialize => self.install_atomics(p).map(|()| Value::UNDEFINED),
            #[cfg(feature = "wasm")]
            Native::WebAssemblyInitialize => self.install_webassembly_routed(p).map(|()| Value::UNDEFINED),
            #[cfg(not(feature = "wasm"))]
            Native::WebAssemblyInitialize => Err(self.type_error(p, "WebAssembly is unavailable in this build".into())),
            Native::IntlInitialize => {
                let (global, object_prototype) = (self.realm.globals, self.object_proto);
                layer_hooks::install_intl_for_realm(self, p, global, object_prototype).map(|()| Value::UNDEFINED)
            }
            Native::ReflectInitialize => self.install_reflect(p).map(|()| Value::UNDEFINED),
            Native::ProxyInitialize => self.install_proxy(p).map(|()| Value::UNDEFINED),
            Native::WeakRefInitialize => self.install_weak_ref(p).map(|()| Value::UNDEFINED),
            Native::FinalizationRegistryInitialize => {
                self.install_finalization_registry(p).map(|()| Value::UNDEFINED)
            }
            Native::DataViewInitialize => self.install_data_view(p).map(|()| Value::UNDEFINED),
            Native::TemporalInitialize
            | Native::IntlNumberFormat
            | Native::IntlNumberFormatSupportedLocalesOf
            | Native::IntlDisplayNamesSupportedLocalesOf
            | Native::IntlNumberFormatFormatGetter
            | Native::IntlNumberFormatFormat
            | Native::IntlNumberFormatFormatToParts
            | Native::IntlNumberFormatFormatRange
            | Native::IntlNumberFormatFormatRangeToParts
            | Native::IntlNumberFormatResolvedOptions
            | Native::IntlPluralRulesSupportedLocalesOf
            | Native::IntlPluralRulesSelect
            | Native::IntlPluralRulesSelectRange
            | Native::IntlPluralRulesResolvedOptions
            | Native::IntlGetCanonicalLocales
            | Native::IntlSupportedValuesOf
            | Native::IntlLocale
            | Native::IntlLocaleToString
            | Native::IntlLocaleMaximize
            | Native::IntlLocaleMinimize
            | Native::IntlLocaleGetCalendars
            | Native::IntlLocaleGetCollations
            | Native::IntlLocaleGetHourCycles
            | Native::IntlLocaleGetNumberingSystems
            | Native::IntlLocaleGetTimeZones
            | Native::IntlLocaleGetTextInfo
            | Native::IntlLocaleGetWeekInfo
            | Native::IntlLocaleBaseNameGetter
            | Native::IntlLocaleLanguageGetter
            | Native::IntlLocaleScriptGetter
            | Native::IntlLocaleRegionGetter
            | Native::IntlLocaleVariantsGetter
            | Native::IntlLocaleCalendarGetter
            | Native::IntlLocaleCollationGetter
            | Native::IntlLocaleHourCycleGetter
            | Native::IntlLocaleCaseFirstGetter
            | Native::IntlLocaleFirstDayOfWeekGetter
            | Native::IntlLocaleNumberingSystemGetter
            | Native::IntlLocaleNumericGetter
            | Native::IntlRelativeTimeFormat
            | Native::IntlRelativeTimeFormatResolvedOptions
            | Native::IntlRelativeTimeFormatSupportedLocalesOf
            | Native::IntlRelativeTimeFormatFormat
            | Native::IntlRelativeTimeFormatFormatToParts
            | Native::IntlSegmenterSupportedLocalesOf
            | Native::IntlSegmenterSegment
            | Native::IntlSegmenterResolvedOptions
            | Native::IntlSegmenterSegmentsIterator
            | Native::IntlSegmenterSegmentsContaining
            | Native::IntlSegmenterIteratorNext
            | Native::IntlCollator
            | Native::IntlCollatorSupportedLocalesOf
            | Native::IntlCollatorCompareGetter
            | Native::IntlCollatorCompare
            | Native::IntlCollatorResolvedOptions
            | Native::IntlDateTimeFormat
            | Native::IntlDisplayNamesOf
            | Native::IntlDisplayNamesResolvedOptions
            | Native::IntlDurationFormatSupportedLocalesOf
            | Native::IntlDurationFormatFormatGetter
            | Native::IntlDurationFormatFormat
            | Native::IntlDurationFormatFormatToParts
            | Native::IntlDurationFormatResolvedOptions
            | Native::IntlListFormatFormatGetter
            | Native::IntlListFormatFormat
            | Native::IntlListFormatFormatToParts
            | Native::IntlListFormatResolvedOptions
            | Native::IntlListFormatSupportedLocalesOf
            | Native::IntlDateTimeFormatFormatGetter
            | Native::IntlDateTimeFormatFormat
            | Native::IntlDateTimeFormatFormatToParts
            | Native::IntlDateTimeFormatFormatRange
            | Native::IntlDateTimeFormatFormatRangeToParts
            | Native::IntlDateTimeFormatSupportedLocalesOf
            | Native::IntlDateTimeFormatResolvedOptions
            | Native::TemporalToLocaleString
            | Native::TemporalDuration
            | Native::TemporalDurationFrom
            | Native::TemporalDurationCompare
            | Native::TemporalDurationAdd
            | Native::TemporalDurationSubtract
            | Native::TemporalDurationWith
            | Native::TemporalDurationAbs
            | Native::TemporalDurationNegated
            | Native::TemporalDurationTotal
            | Native::TemporalDurationRound
            | Native::TemporalDurationToString
            | Native::TemporalDurationToJSON
            | Native::TemporalDurationValueOf
            | Native::TemporalDurationYearsGetter
            | Native::TemporalDurationMonthsGetter
            | Native::TemporalDurationWeeksGetter
            | Native::TemporalDurationDaysGetter
            | Native::TemporalDurationHoursGetter
            | Native::TemporalDurationMinutesGetter
            | Native::TemporalDurationSecondsGetter
            | Native::TemporalDurationMillisecondsGetter
            | Native::TemporalDurationMicrosecondsGetter
            | Native::TemporalDurationNanosecondsGetter
            | Native::TemporalDurationSignGetter
            | Native::TemporalDurationBlankGetter
            | Native::TemporalPlainDate
            | Native::TemporalPlainDateFrom
            | Native::TemporalPlainDateCompare
            | Native::TemporalPlainDateCalendarIdGetter
            | Native::TemporalPlainDateYearGetter
            | Native::TemporalPlainDateMonthGetter
            | Native::TemporalPlainDateMonthCodeGetter
            | Native::TemporalPlainDateDayGetter
            | Native::TemporalPlainDateEraGetter
            | Native::TemporalPlainDateEraYearGetter
            | Native::TemporalPlainDateDayOfWeekGetter
            | Native::TemporalPlainDateDayOfYearGetter
            | Native::TemporalPlainDateWeekOfYearGetter
            | Native::TemporalPlainDateYearOfWeekGetter
            | Native::TemporalPlainDateDaysInWeekGetter
            | Native::TemporalPlainDateDaysInMonthGetter
            | Native::TemporalPlainDateDaysInYearGetter
            | Native::TemporalPlainDateMonthsInYearGetter
            | Native::TemporalPlainDateInLeapYearGetter
            | Native::TemporalPlainDateToString
            | Native::TemporalPlainDateToJSON
            | Native::TemporalPlainDateToLocaleString
            | Native::TemporalPlainDateToPlainDateTime
            | Native::TemporalPlainDateToPlainMonthDay
            | Native::TemporalPlainDateToPlainYearMonth
            | Native::TemporalPlainDateToZonedDateTime
            | Native::TemporalPlainDateEquals
            | Native::TemporalPlainDateWith
            | Native::TemporalPlainDateWithCalendar
            | Native::TemporalPlainDateValueOf
            | Native::TemporalPlainDateAdd
            | Native::TemporalPlainDateSubtract
            | Native::TemporalPlainDateUntil
            | Native::TemporalPlainDateSince
            | Native::TemporalPlainDateTime
            | Native::TemporalPlainDateTimeFrom
            | Native::TemporalPlainDateTimeCompare
            | Native::TemporalPlainDateTimeAdd
            | Native::TemporalPlainDateTimeSubtract
            | Native::TemporalPlainDateTimeRound
            | Native::TemporalPlainDateTimeUntil
            | Native::TemporalPlainDateTimeSince
            | Native::TemporalPlainDateTimeToString
            | Native::TemporalPlainDateTimeToJSON
            | Native::TemporalPlainDateTimeToPlainDate
            | Native::TemporalPlainDateTimeToPlainTime
            | Native::TemporalPlainDateTimeValueOf
            | Native::TemporalPlainDateTimeToZonedDateTime
            | Native::TemporalPlainDateTimeWith
            | Native::TemporalPlainDateTimeWithCalendar
            | Native::TemporalPlainDateTimeWithPlainTime
            | Native::TemporalPlainDateTimeCalendarIdGetter
            | Native::TemporalPlainDateTimeYearGetter
            | Native::TemporalPlainDateTimeMonthGetter
            | Native::TemporalPlainDateTimeMonthCodeGetter
            | Native::TemporalPlainDateTimeDayGetter
            | Native::TemporalPlainDateTimeEraGetter
            | Native::TemporalPlainDateTimeEraYearGetter
            | Native::TemporalPlainDateTimeDayOfWeekGetter
            | Native::TemporalPlainDateTimeDayOfYearGetter
            | Native::TemporalPlainDateTimeWeekOfYearGetter
            | Native::TemporalPlainDateTimeYearOfWeekGetter
            | Native::TemporalPlainDateTimeDaysInWeekGetter
            | Native::TemporalPlainDateTimeDaysInMonthGetter
            | Native::TemporalPlainDateTimeDaysInYearGetter
            | Native::TemporalPlainDateTimeMonthsInYearGetter
            | Native::TemporalPlainDateTimeInLeapYearGetter
            | Native::TemporalPlainDateTimeHourGetter
            | Native::TemporalPlainDateTimeMinuteGetter
            | Native::TemporalPlainDateTimeSecondGetter
            | Native::TemporalPlainDateTimeMillisecondGetter
            | Native::TemporalPlainDateTimeMicrosecondGetter
            | Native::TemporalPlainDateTimeNanosecondGetter
            | Native::TemporalPlainDateTimeEquals
            | Native::TemporalPlainTime
            | Native::TemporalPlainTimeFrom
            | Native::TemporalPlainTimeCompare
            | Native::TemporalPlainTimeAdd
            | Native::TemporalPlainTimeSubtract
            | Native::TemporalPlainTimeEquals
            | Native::TemporalPlainTimeHourGetter
            | Native::TemporalPlainTimeMinuteGetter
            | Native::TemporalPlainTimeSecondGetter
            | Native::TemporalPlainTimeMillisecondGetter
            | Native::TemporalPlainTimeMicrosecondGetter
            | Native::TemporalPlainTimeNanosecondGetter
            | Native::TemporalPlainTimeValueOf
            | Native::TemporalPlainTimeRound
            | Native::TemporalPlainTimeUntil
            | Native::TemporalPlainTimeSince
            | Native::TemporalPlainTimeToString
            | Native::TemporalPlainTimeToJSON
            | Native::TemporalPlainTimeWith
            | Native::TemporalPlainMonthDay
            | Native::TemporalPlainMonthDayFrom
            | Native::TemporalPlainMonthDayCompare
            | Native::TemporalPlainMonthDayCalendarIdGetter
            | Native::TemporalPlainMonthDayDayGetter
            | Native::TemporalPlainMonthDayMonthCodeGetter
            | Native::TemporalPlainMonthDayEquals
            | Native::TemporalPlainMonthDayToPlainDate
            | Native::TemporalPlainMonthDayWith
            | Native::TemporalPlainMonthDayValueOf
            | Native::TemporalPlainYearMonth
            | Native::TemporalPlainYearMonthFrom
            | Native::TemporalPlainYearMonthCompare
            | Native::TemporalPlainYearMonthCalendarIdGetter
            | Native::TemporalPlainYearMonthYearGetter
            | Native::TemporalPlainYearMonthMonthGetter
            | Native::TemporalPlainYearMonthMonthCodeGetter
            | Native::TemporalPlainYearMonthEraGetter
            | Native::TemporalPlainYearMonthEraYearGetter
            | Native::TemporalPlainYearMonthReferenceISODayGetter
            | Native::TemporalPlainYearMonthDaysInMonthGetter
            | Native::TemporalPlainYearMonthDaysInYearGetter
            | Native::TemporalPlainYearMonthMonthsInYearGetter
            | Native::TemporalPlainYearMonthInLeapYearGetter
            | Native::TemporalPlainYearMonthEquals
            | Native::TemporalPlainYearMonthAdd
            | Native::TemporalPlainYearMonthSubtract
            | Native::TemporalPlainYearMonthUntil
            | Native::TemporalPlainYearMonthSince
            | Native::TemporalPlainYearMonthWith
            | Native::TemporalPlainYearMonthValueOf
            | Native::TemporalPlainYearMonthToPlainDate
            | Native::TemporalZonedDateTime
            | Native::TemporalZonedDateTimeFrom
            | Native::TemporalZonedDateTimeCompare
            | Native::TemporalZonedDateTimeEquals
            | Native::TemporalZonedDateTimeWith
            | Native::TemporalZonedDateTimeWithTimeZone
            | Native::TemporalZonedDateTimeWithCalendar
            | Native::TemporalZonedDateTimeWithPlainTime
            | Native::TemporalZonedDateTimeToInstant
            | Native::TemporalZonedDateTimeToPlainDate
            | Native::TemporalZonedDateTimeToPlainDateTime
            | Native::TemporalZonedDateTimeToPlainTime
            | Native::TemporalZonedDateTimeValueOf
            | Native::TemporalZonedDateTimeAdd
            | Native::TemporalZonedDateTimeSubtract
            | Native::TemporalZonedDateTimeGetTimeZoneTransition
            | Native::TemporalZonedDateTimeStartOfDay
            | Native::TemporalZonedDateTimeRound
            | Native::TemporalZonedDateTimeUntil
            | Native::TemporalZonedDateTimeSince
            | Native::TemporalZonedDateTimeToString
            | Native::TemporalZonedDateTimeToJSON
            | Native::TemporalZonedDateTimeToLocaleString
            | Native::TemporalZonedDateTimeEpochNanosecondsGetter
            | Native::TemporalZonedDateTimeTimeZoneIdGetter
            | Native::TemporalZonedDateTimeCalendarIdGetter
            | Native::TemporalZonedDateTimeYearGetter
            | Native::TemporalZonedDateTimeMonthGetter
            | Native::TemporalZonedDateTimeMonthCodeGetter
            | Native::TemporalZonedDateTimeDayGetter
            | Native::TemporalZonedDateTimeHourGetter
            | Native::TemporalZonedDateTimeMinuteGetter
            | Native::TemporalZonedDateTimeSecondGetter
            | Native::TemporalZonedDateTimeMillisecondGetter
            | Native::TemporalZonedDateTimeMicrosecondGetter
            | Native::TemporalZonedDateTimeNanosecondGetter
            | Native::TemporalZonedDateTimeEraGetter
            | Native::TemporalZonedDateTimeEraYearGetter
            | Native::TemporalZonedDateTimeDayOfWeekGetter
            | Native::TemporalZonedDateTimeDayOfYearGetter
            | Native::TemporalZonedDateTimeWeekOfYearGetter
            | Native::TemporalZonedDateTimeYearOfWeekGetter
            | Native::TemporalZonedDateTimeDaysInWeekGetter
            | Native::TemporalZonedDateTimeDaysInMonthGetter
            | Native::TemporalZonedDateTimeDaysInYearGetter
            | Native::TemporalZonedDateTimeMonthsInYearGetter
            | Native::TemporalZonedDateTimeInLeapYearGetter
            | Native::TemporalZonedDateTimeEpochMillisecondsGetter
            | Native::TemporalZonedDateTimeHoursInDayGetter
            | Native::TemporalZonedDateTimeOffsetGetter
            | Native::TemporalZonedDateTimeOffsetNanosecondsGetter
            | Native::TemporalInstant
            | Native::TemporalInstantFrom
            | Native::TemporalInstantCompare
            | Native::TemporalInstantFromEpochMilliseconds
            | Native::TemporalInstantFromEpochNanoseconds
            | Native::TemporalInstantEpochNanosecondsGetter
            | Native::TemporalInstantEpochMillisecondsGetter
            | Native::TemporalInstantToString
            | Native::TemporalInstantToJSON
            | Native::TemporalInstantValueOf
            | Native::TemporalInstantEquals
            | Native::TemporalInstantAdd
            | Native::TemporalInstantSubtract
            | Native::TemporalInstantRound
            | Native::TemporalInstantSince
            | Native::TemporalInstantUntil
            | Native::TemporalInstantToZonedDateTimeISO
            | Native::TemporalNowInstant
            | Native::TemporalNowPlainDateISO
            | Native::TemporalNowPlainDateTimeISO
            | Native::TemporalNowPlainTimeISO
            | Native::TemporalNowTimeZoneId
            | Native::TemporalNowZonedDateTimeISO => super::layer_hooks::intl_temporal_call(self, p, native, this, args),
            Native::IsHTMLDDA => Ok(Value::NULL),
            Native::Proxy => Err(self.type_error(p, "Proxy must be called with new".into())),
            Native::DataView => Err(self.type_error(p, "DataView constructor requires new".into())),
            Native::AbstractModuleSourceToStringTag => {
                Ok(self.abstract_module_source_to_string_tag(this))
            }
            Native::WithEnter => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if value.is_null() || value.is_undefined() {
                    return Err(self.type_error(p, "cannot convert nullish value to object".into()));
                }
                let object = self.box_object(value)?;
                self.with_stack.push(object);
                self.enter_with_scope();
                Ok(Value::UNDEFINED)
            }
            Native::WithExit => {
                self.with_stack.pop();
                Ok(Value::UNDEFINED)
            }
            Native::Eval => self.eval_native(p, args),
            Native::EvalScript => self.eval_script_native(p, args),
            Native::CollectGarbage => {
                self.collect_now(p);
                Ok(Value::UNDEFINED)
            }
            Native::ProxyRevocable => self.proxy_revocable(p, args),
            Native::ArrayBuffer | Native::SharedArrayBuffer => {
                Err(self.type_error(p, "constructor requires new".into()))
            }
            #[cfg(feature = "wasm")]
            Native::WasmHost => self.call_wasm_host(args),
            #[cfg(not(feature = "wasm"))]
            Native::WasmHost => Err(self.type_error(p, "WebAssembly is unavailable in this build".into())),
            Native::HostFunction => self.call_host_function(this, args),
            Native::Print => self.console_write(p, args, CapabilityId::WriteLine),
            Native::PrintError => self.console_write(p, args, CapabilityId::WriteErr),
            Native::Date => self.date_call(),
            Native::IntlPluralRules => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlSegmenter => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlDisplayNames => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlDurationFormat => {
                Err(self.type_error(p, "constructor requires new".into()))
            }
            Native::IntlListFormat => Err(self.type_error(p, "constructor requires new".into())),
            Native::DateNow => Ok(self.date_now()),
            Native::DateGetTime
            | Native::DateValueOf
            | Native::DateGetTimezoneOffset
            | Native::DateGetFullYear
            | Native::DateGetMonth
            | Native::DateGetDate
            | Native::DateGetDay
            | Native::DateGetHours
            | Native::DateGetMinutes
            | Native::DateGetSeconds
            | Native::DateGetMilliseconds
            | Native::DateGetUTCFullYear
            | Native::DateGetUTCMonth
            | Native::DateGetUTCDate
            | Native::DateGetUTCDay
            | Native::DateGetUTCHours
            | Native::DateGetUTCMinutes
            | Native::DateGetUTCSeconds
            | Native::DateGetUTCMilliseconds
            | Native::DateGetYear
            | Native::DateSetTime
            | Native::DateSetFullYear
            | Native::DateSetMonth
            | Native::DateSetUTCMonth
            | Native::DateSetDate
            | Native::DateSetUTCDate
            | Native::DateSetUTCFullYear
            | Native::DateSetHours
            | Native::DateSetMinutes
            | Native::DateSetSeconds
            | Native::DateSetMilliseconds
            | Native::DateSetUTCHours
            | Native::DateSetUTCMinutes
            | Native::DateSetUTCSeconds
            | Native::DateSetUTCMilliseconds
            | Native::DateSetYear
            | Native::DateToString
            | Native::DateToDateString
            | Native::DateToTimeString
            | Native::DateToUTCString
            | Native::DateToLocaleString
            | Native::DateToLocaleDateString
            | Native::DateToLocaleTimeString
            | Native::DateToISOString
            | Native::DateToJSON
            | Native::DateToPrimitive
            | Native::DateToTemporalInstant => self.date_native(p, native, this, args),
            Native::DateParse | Native::DateUTC => self.date_static_native(p, native, args),
            Native::RegExpCompile => self.regexp_compile_native(p, this, args),
            Native::RegExpExec => self.regexp_builtin_exec(p, this, args),
            Native::RegExpTest => self.regexp_test(p, this, args),
            Native::RegExpEscape => self.regexp_escape_native(p, args),
            Native::RegExpSymbolMatch => self.regexp_symbol_match(p, this, args),
            Native::RegExpSymbolSearch => self.regexp_symbol_search(p, this, args),
            Native::RegExpSymbolReplace => self.regexp_symbol_replace(p, this, args),
            Native::RegExpSymbolMatchAll => self.regexp_symbol_match_all(p, this, args),
            Native::RegExpSymbolSplit => self.regexp_symbol_split(p, this, args),
            Native::RegExpToString => self.regexp_to_string_native(p, this),
            Native::RegExpSpecies => Ok(this),
            Native::RegExpLegacyGetter => self.regexp_legacy_getter_native(p, this),
            Native::RegExpLegacySetter => self.regexp_legacy_setter_native(p, this, args),
            Native::RegExpGlobal
            | Native::RegExpIgnoreCase
            | Native::RegExpMultiline
            | Native::RegExpDotAll
            | Native::RegExpUnicode
            | Native::RegExpUnicodeSets
            | Native::RegExpSticky
            | Native::RegExpHasIndices => self.regexp_flag_native(p, native, this),
            Native::RegExpSource => self.regexp_slot_native(p, native, this),
            Native::RegExpFlags => self.regexp_flags_native(p, this),
            Native::ObjectPrototypeHasOwnProperty | Native::ObjectPrototypePropertyIsEnumerable => {
                let key_value =
                    self.to_property_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let receiver = self.box_object_or_type_error(p, this)?;
                let found = match self.own_property_enumerable(p, receiver, key_value)? {
                    Some(enumerable) => {
                        native == Native::ObjectPrototypeHasOwnProperty || enumerable
                    }
                    None => false,
                };
                Ok(if found { Value::TRUE } else { Value::FALSE })
            }
            Native::ObjectPrototypeLookupGetter | Native::ObjectPrototypeLookupSetter => {
                self.require_object_coercible(p, this)?;
                let key =
                    self.to_property_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let mut object = self.box_object(this)?;
                loop {
                    let descriptor = self.object_get_own_property_descriptor(p, &[object, key])?;
                    if !descriptor.is_undefined() {
                        let field =
                            self.intern_atom(if native == Native::ObjectPrototypeLookupGetter {
                                "get"
                            } else {
                                "set"
                            });
                        return self.get_property(p, descriptor, field);
                    }
                    object = self.object_get_prototype_of(p, object)?;
                    if object.is_null() {
                        return Ok(Value::UNDEFINED);
                    }
                }
            }
            Native::ObjectPrototypeDefineGetter | Native::ObjectPrototypeDefineSetter => {
                self.object_prototype_define_accessor(p, native, this, args)
            }
            Native::ObjectPrototypeToString => self.object_prototype_to_string(p, this),
            Native::ObjectPrototypeProtoGetter => {
                self.require_object_coercible(p, this)?;
                let object = self.box_object(this)?;
                self.object_get_prototype_of(p, object)
            }
            Native::ObjectPrototypeProtoSetter => {
                self.require_object_coercible(p, this)?;
                let proto = args.first().copied().unwrap_or(Value::UNDEFINED);
                if self.is_object_like(this)
                    && (proto.is_null() || self.object_data(proto).is_some())
                {
                    self.object_set_prototype_of(p, this, proto)?;
                }
                Ok(Value::UNDEFINED)
            }
            Native::ObjectPrototypeToLocaleString => {
                self.object_prototype_to_locale_string(p, this)
            }
            Native::ObjectPrototypeValueOf => self.box_object(this).map_err(|_| {
                self.type_error(
                    p,
                    "Object.prototype.valueOf called on null or undefined".into(),
                )
            }),
            Native::ObjectPrototypeIsPrototypeOf => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                if self.object_data(target).is_none() {
                    return Ok(Value::FALSE);
                }
                let prototype = self.box_object(this)?;
                let mut current = target;
                let mut found = false;
                while self.is_object_like(current) {
                    current = self.object_get_prototype_of(p, current)?;
                    if current == prototype {
                        found = true;
                        break;
                    }
                    if current.is_null() {
                        break;
                    }
                }
                Ok(if found { Value::TRUE } else { Value::FALSE })
            }
            Native::ReflectGet
            | Native::ReflectHas
            | Native::ReflectApply
            | Native::ReflectGetOwnPropertyDescriptor
            | Native::ReflectDefineProperty
            | Native::ReflectDeleteProperty
            | Native::ReflectPreventExtensions
            | Native::ReflectIsExtensible
            | Native::ReflectSet
            | Native::SuperSet
            | Native::ObjectLiteralPrototype
            | Native::ObjectLiteralCreate
            | Native::ReflectOwnKeys
            | Native::ReflectGetPrototypeOf
            | Native::ReflectSetPrototypeOf
            | Native::ReflectConstruct => self.call_reflect_native(p, native, args),
            Native::JsonParse => self.json_parse(p, args),
            Native::JsonStringify => self.json_stringify(p, args),
            Native::JsonRawJson => self.json_raw_json(p, args),
            Native::JsonIsRawJson => self.json_is_raw_json(args),
            Native::GlobalIsNaN | Native::GlobalIsFinite => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let number = self.to_number(p, value)?;
                Ok(Value::boolean(super::number::global_number_predicate(native, number).unwrap_or_default()))
            }
            // Each argument converted in order, then the one numeric body (`number::math_numbers`).
            Native::MathLog | Native::MathPow => {
                let a = args.first().copied().unwrap_or(Value::UNDEFINED);
                let x = self.to_number(p, a)?;
                let y = if native == Native::MathPow {
                    self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?
                } else {
                    f64::NAN
                };
                let result = super::number::math_numbers(native, &[x, y]).or_else(|| super::number::math_unary(native, x));
                Ok(Value::number(result.unwrap_or(f64::NAN)))
            }
            Native::ArrayPush => self.array_push_native(p, this, args),
            Native::ArrayIsArray => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.is_array(p, value)? {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::ArrayPop => self.array_pop_native(p, this),
            Native::ArraySlice => self.array_slice_native(p, this, args),
            Native::ArrayIncludes => self.array_includes_native(p, this, args),
            Native::ArrayJoin => self.array_join_native(p, this, args),
            Native::ArrayConcat => self.array_concat_native(p, this, args),
            Native::ArrayFlat => self.array_flatten_native(p, native, this, args),
            Native::ArrayReverse => self.array_reverse_native(p, this),
            Native::ArrayShift => self.array_shift_native(p, this),
            Native::ArrayUnshift => self.array_unshift_native(p, this, args),
            Native::ArraySplice => self.array_splice_native(p, this, args),
            Native::ArrayFill => self.array_fill_native(p, this, args),
            Native::ArrayAt
            | Native::ArrayLastIndexOf
            | Native::ArrayIndexOf
            | Native::ArrayCopyWithin
            | Native::ArrayWith
            | Native::ArrayForEach
            | Native::ArrayMap
            | Native::ArrayFilter
            | Native::ArraySome
            | Native::ArrayEvery
            | Native::ArrayFind
            | Native::ArrayFindIndex
            | Native::ArrayFindLast
            | Native::ArrayFindLastIndex
            | Native::ArrayGroup
            | Native::ArrayGroupToMap
            | Native::ArrayFlatMap
            | Native::ArrayReduce
            | Native::ArrayReduceRight => self.array_indexed_native(p, native, this, args),
            Native::ArrayToReversed
            | Native::ArrayToSpliced
            | Native::ArraySort
            | Native::ArrayToSorted
            | Native::ArrayToString
            | Native::ArrayToLocaleString
            | Native::ArraySpecies => self.array_modern_native(p, native, this, args),
            Native::ArrayKeys | Native::ArrayValues | Native::ArrayEntries => {
                self.array_iterator_native(p, native, this)
            }
            Native::StringValues => self.string_iterator_native(p, this),
            Native::ArrayBufferSlice => self.array_buffer_slice_native(p, this, args, false),
            Native::SharedArrayBufferSlice => self.array_buffer_slice_native(p, this, args, true),
            Native::ArrayBufferTransfer => {
                self.array_buffer_transfer_native(p, this, args, true, false)
            }
            Native::ArrayBufferResize => self.array_buffer_resize_native(p, this, args),
            Native::ArrayBufferTransferToFixedLength => {
                self.array_buffer_transfer_native(p, this, args, false, false)
            }
            Native::ArrayBufferTransferToImmutable => {
                self.array_buffer_transfer_native(p, this, args, false, true)
            }
            Native::ArrayBufferSliceToImmutable => {
                self.array_buffer_slice_immutable_native(p, this, args)
            }
            Native::ArrayBufferByteLengthGetter
            | Native::ArrayBufferDetachedGetter
            | Native::ArrayBufferImmutableGetter
            | Native::ArrayBufferMaxByteLengthGetter
            | Native::ArrayBufferResizableGetter
            | Native::SharedArrayBufferByteLengthGetter
            | Native::SharedArrayBufferGrowableGetter
            | Native::SharedArrayBufferMaxByteLengthGetter => {
                self.array_buffer_getter_native(p, native, this)
            }
            Native::DetachArrayBuffer => self.detach_array_buffer_native(p, args),
            Native::SharedArrayBufferGrow => self.shared_array_buffer_grow_native(p, this, args),
            Native::ArrayFrom
            | Native::ArrayFromAsync
            | Native::ArrayOf
            | Native::TypedArrayFrom
            | Native::TypedArrayOf => self.array_modern_native(p, native, this, args),
            Native::TypedArrayToStringTag => Ok(self.typed_array_to_string_tag_native(this)),
            Native::TypedArrayBufferGetter
            | Native::TypedArrayByteLengthGetter
            | Native::TypedArrayByteOffsetGetter
            | Native::TypedArrayLengthGetter => self.typed_array_getter_native(p, native, this),
            Native::FunctionPrototype => Ok(Value::UNDEFINED),
            Native::FunctionPrototypeHasInstance => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.ordinary_has_instance(p, this, value)? {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::FunctionCall => {
                let receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                self.call_value(p, this, receiver, args.get(1..).unwrap_or_default())
            }
            Native::FunctionApply => {
                let receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                let argument_array = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let arguments = self.call_argument_list(p, argument_array, true)?;
                self.call_value(p, this, receiver, &arguments)
            }
            Native::FunctionBind => self.bind_function(p, this, args),
            Native::FunctionBoundCall => self.call_bound_function(p, args),
            Native::FunctionToString => {
                if !self.is_function(this) {
                    return Err(self.type_error(
                        p,
                        "Function.prototype.toString called on incompatible receiver".into(),
                    ));
                }
                if let Some(source) = self.function_source_value(this) {
                    return Ok(source);
                }
                Ok(self
                    .heap
                    .alloc_string("function () { [native code] }".into()))
            }
            Native::Number
            | Native::NumberIsNaN
            | Native::NumberIsFinite
            | Native::NumberIsInteger
            | Native::NumberIsSafeInteger => self.call_number_native(p, native, args),
            Native::NumberFixed => self.number_format(p, this, args, native),
            Native::NumberPrecision => self.number_format(p, this, args, native),
            Native::String => {
                let argument = self.string_constructor_argument(args);
                let text = if let Some(description) = self.heap.symbol_data(argument) {
                    JsString::from(format!("Symbol({})", description.as_deref().unwrap_or("")))
                } else {
                    self.coerce_js_string(p, argument)?
                };
                Ok(self.heap.alloc_string(text))
            }
            Native::Symbol => self.call_symbol_constructor(p, args),
            Native::SymbolToString
            | Native::SymbolToPrimitive
            | Native::SymbolValueOf
            | Native::SymbolDescriptionGetter => self.call_symbol_value_native(p, native, this),
            Native::SymbolFor | Native::SymbolKeyFor => self.call_symbol_native(p, native, args),
            Native::TemporalPlainMonthDayToString
            | Native::TemporalPlainMonthDayToJSON
            | Native::TemporalPlainMonthDayToLocaleString
            | Native::TemporalPlainYearMonthToString
            | Native::TemporalPlainYearMonthToJSON
            | Native::TemporalPlainYearMonthToLocaleString => {
                layer_hooks::intl_temporal_call(self, p, native, this, args)
            }
            Native::Object
            | Native::Array
            | Native::Map
            | Native::Set
            | Native::WeakMap
            | Native::WeakSet
            | Native::WeakRef
            | Native::DisposableStack
            | Native::AsyncDisposableStack => self.construct_native(p, native, args),
            Native::RegExp => self.construct_regexp_native(p, args, None),
            Native::ThrowTypeError => {
                Err(self.type_error(p, "restricted arguments property".into()))
            }
            Native::FinalizationRegistry => {
                Err(self.type_error(p, "Constructor FinalizationRegistry requires 'new'".into()))
            }
            Native::FunctionCaller => {
                if self.function_caller_is_restricted(this) {
                    Err(self.type_error(p, "restricted function caller access".into()))
                } else {
                    self.function_caller_value(this)
                }
            }
            Native::CallSiteConstructor => self.call_site_constructor(p),
            Native::ErrorToString => self.error_to_string(p, this),
            Native::ErrorCaptureStackTrace => self.error_capture_stack_trace(p, args),
            Native::ErrorIsError => Ok(
                if self.error_is_error(args.first().copied().unwrap_or(Value::UNDEFINED)) {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            Native::ErrorStackGetter => self.error_stack_getter(p, this),
            Native::ErrorStackSetter => self.error_stack_setter(p, this, args),
            Native::CallSiteGetFileName
            | Native::CallSiteGetThis
            | Native::CallSiteGetFunction
            | Native::CallSiteGetFunctionName
            | Native::CallSiteGetLineNumber
            | Native::CallSiteGetColumnNumber
            | Native::CallSiteGetTypeName
            | Native::CallSiteGetMethodName
            | Native::CallSiteIsEval
            | Native::CallSiteGetEvalOrigin
            | Native::CallSiteIsConstructor
            | Native::CallSiteIsNative
            | Native::CallSiteToString => self.call_site_native(p, native, this),
            Native::ArrayBufferSpecies => Ok(this),
            Native::NumberExponential => self.number_exponential(p, this, args),
            native if native.is_error_constructor() => {
                let callee = self
                    .active_native_callable()
                    .unwrap_or_else(|| self.native_value(native));
                self.construct_value_with_new_target(p, callee, callee, args)
            }
            // Last among the guarded arms, so the named natives above dispatch by one jump.
            native if native.is_host_control_native() => self.call_host(p, native, args),
            _ => self.call_primitive_native(p, native, this, args),
        }
    }
    /// An arithmetic, bitwise or shift operator applied to two numbers: the integer fast path
    /// when both are integers, otherwise the IEEE 754 result. Any other operand pair, and
    /// every other operator, is left to `binary`. The four arithmetic operators match the
    /// shared arithmetic stage; the rest go through the numeric operator table.
    /// The operands are classified only for an operator that takes Numbers; a caller that has
    /// classified its operands already calls `number_binary_classified` instead.
    #[inline(always)]
    pub fn number_binary(&mut self, op: u32, left: Value, right: Value) -> Option<Value> {
        if NumberArithmetic::from_operator(op).is_none() && !is_numeric_binary_operator(op) {
            return None;
        }
        self.number_binary_classified(op, left, right, Value::numeric_pair(left, right))
    }

    /// `number_binary` of operands whose classification `pair` is `Value::numeric_pair(left,
    /// right)`, which the caller made once and reuses here rather than classifying them again.
    /// Its result is `number_binary`'s for the same operands.
    #[inline(always)]
    pub fn number_binary_classified(
        &mut self,
        op: u32,
        left: Value,
        right: Value,
        pair: NumericPair,
    ) -> Option<Value> {
        let Some(operation) = NumberArithmetic::from_operator(op) else {
            return self.tabled_number_binary(op, left, right, pair);
        };
        let (value, path) = number_arithmetic_values(operation, pair)?;
        self.record_binary_value_path(op, left, right, path);
        Some(value)
    }

    /// `number_binary_classified` for the operators past the four arithmetic ones.
    #[inline(always)]
    fn tabled_number_binary(&mut self, op: u32, left: Value, right: Value, pair: NumericPair) -> Option<Value> {
        if !is_numeric_binary_operator(op) {
            return None;
        }
        let operator = numeric_binary_operator(op)
            .expect("numeric operator range is represented in the operator table");
        if let NumericPair::Ints(a, b) = pair
            && let Some(value) = numeric_integer_result(operator, a, b)
        {
            self.record_binary_value_path(
                op,
                left,
                right,
                crate::profile::BinaryValuePath::IntegerFastPath,
            );
            return Some(value);
        }
        let (left_number, right_number) = pair.as_doubles()?;
        self.record_binary_value_path(
            op,
            left,
            right,
            crate::profile::BinaryValuePath::NumberFastPath,
        );
        Some(Value::number(numeric_number_result(
            operator,
            left_number,
            right_number,
        )))
    }

    #[inline]
    pub fn binary(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        if let Some(value) = self.number_binary(op, left, right) {
            return Ok(value);
        }
        if is_numeric_binary_operator(op) {
            let operator = numeric_binary_operator(op)
                .expect("numeric operator range is represented in the operator table");
            if operator == BinaryOperator::Addition {
                self.record_binary_value_path(
                    op,
                    left,
                    right,
                    crate::profile::BinaryValuePath::Fallback,
                );
                // ToPrimitive of a string is the string itself and concatenation never
                // collects, so string + string needs neither coercion nor extra roots.
                if self.heap.is_string(left)
                    && self.heap.is_string(right)
                {
                    return self.concatenate(p, left, right);
                }
                return self.with_coerced_operands(
                    p,
                    OperandCoercion::PrimitiveDefault,
                    left,
                    right,
                    |vm, left, right| {
                        if !vm.is_string(left)
                            && !vm.is_string(right)
                            && (matches!(vm.heap.get(left), Some(Cell::BigInt(_)))
                                || matches!(vm.heap.get(right), Some(Cell::BigInt(_))))
                        {
                            vm.binary_bigint(p, op, left, right)
                        } else {
                            vm.binary_slow(p, op, left, right)
                        }
                    },
                );
            }
            self.record_binary_value_path(
                op,
                left,
                right,
                crate::profile::BinaryValuePath::Fallback,
            );
            return self.with_coerced_operands(
                p,
                OperandCoercion::Numeric,
                left,
                right,
                |vm, left, right| {
                    if matches!(vm.heap.get(left), Some(Cell::BigInt(_)))
                        || matches!(vm.heap.get(right), Some(Cell::BigInt(_)))
                    {
                        vm.binary_bigint(p, op, left, right)
                    } else {
                        vm.binary_slow(p, op, left, right)
                    }
                },
            );
        }
        if let Some(operator) = RelationalOperator::from_immediate(op) {
            if let Some((left_number, right_number)) = Value::numeric_pair(left, right).as_doubles() {
                self.record_binary_value_path(
                    op,
                    left,
                    right,
                    crate::profile::BinaryValuePath::NumberFastPath,
                );
                return Ok(Self::integrity_bool(
                    operator.compare_numbers(left_number, right_number),
                ));
            }
            self.record_binary_value_path(
                op,
                left,
                right,
                crate::profile::BinaryValuePath::Fallback,
            );
            let result = self.compare_relational(p, operator, left, right)?;
            return Ok(Self::integrity_bool(result));
        }
        if op <= BinaryOperator::StrictInequality as u32 {
            let pair = Value::numeric_pair(left, right);
            if let NumericPair::Ints(a, b) = pair {
                self.record_binary_value_path(
                    op,
                    left,
                    right,
                    crate::profile::BinaryValuePath::IntegerFastPath,
                );
                return Ok(Self::integrity_bool(
                    if op == BinaryOperator::Equality as u32
                        || op == BinaryOperator::StrictEquality as u32
                    {
                        a == b
                    } else {
                        a != b
                    },
                ));
            }
            if let Some((a, b)) = pair.as_doubles() {
                self.record_binary_value_path(
                    op,
                    left,
                    right,
                    crate::profile::BinaryValuePath::NumberFastPath,
                );
                let equal = a == b;
                return Ok(Self::integrity_bool(
                    if op == BinaryOperator::Equality as u32
                        || op == BinaryOperator::StrictEquality as u32
                    {
                        equal
                    } else {
                        !equal
                    },
                ));
            }
        }
        self.record_binary_value_path(op, left, right, crate::profile::BinaryValuePath::Fallback);
        self.binary_slow(p, op, left, right)
    }

    pub(super) fn with_coerced_operands<R>(
        &mut self,
        p: &ResidualProgram,
        coercion: OperandCoercion,
        left: Value,
        right: Value,
        operation: impl FnOnce(&mut Self, Value, Value) -> Result<R, JsError>,
    ) -> Result<R, JsError> {
        self.with_call_roots(
            [left, right].into_iter().filter(|value| value.is_heap()),
            |vm| {
                let convert = |vm: &mut Self, value| match coercion {
                    OperandCoercion::PrimitiveDefault => vm.to_primitive(p, value, "default"),
                    OperandCoercion::PrimitiveNumber => vm.to_primitive(p, value, "number"),
                    OperandCoercion::Numeric => vm.to_numeric(p, value),
                };
                let left = convert(vm, left)?;
                vm.with_call_roots(
                    std::iter::once(left).filter(|value| value.is_heap()),
                    |vm| {
                        let right = convert(vm, right)?;
                        vm.with_call_roots(
                            std::iter::once(right).filter(|value| value.is_heap()),
                            |vm| operation(vm, left, right),
                        )
                    },
                )
            },
        )
    }

    fn binary_bigint(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        let Some(result) = self.bigint_operation(op, left, right) else {
            return Err(self.type_error(
                p,
                if self.is_bigint(left) && self.is_bigint(right) {
                    // `>>>` is the one numeric operator BigInt lacks.
                    "BigInts have no unsigned right shift, use >> instead"
                } else {
                    MIXED_BIGINT_TYPES
                }
                .into(),
            ));
        };
        match result {
            Ok(value) => Ok(self.heap.alloc_bigint(value)),
            Err(crate::bigint::Error::DivisionByZero) => {
                Err(self.range_error(p, "Division by zero".into()))
            }
            Err(crate::bigint::Error::NegativeExponent) => {
                Err(self.range_error(p, "Exponent must be positive".into()))
            }
            Err(crate::bigint::Error::TooLarge) => {
                Err(self.range_error(p, "Maximum BigInt size exceeded".into()))
            }
        }
    }

    fn is_bigint(&self, value: Value) -> bool {
        matches!(self.heap.get(value), Some(Cell::BigInt(_)))
    }

    /// BigInt operator `op` of two BigInt values: the result, or the error it raises. `None`
    /// when an operand is not a BigInt or the operator has no BigInt meaning. Inline, so the
    /// result is written where its caller allocates it.
    #[inline(always)]
    fn bigint_operation(
        &self,
        op: u32,
        left: Value,
        right: Value,
    ) -> Option<Result<crate::bigint::JsBigInt, crate::bigint::Error>> {
        const EXPONENTIAL: u32 = BinaryOperator::Exponential as u32;
        use crate::bigint::Bitwise;
        let (Some(Cell::BigInt(left)), Some(Cell::BigInt(right))) =
            (self.heap.get(left), self.heap.get(right))
        else {
            return None;
        };
        if let Some(operator) = numeric_binary_operator(op)
            && let Some(value) = crate::staged::bigint_small::small_operation(operator, left, right)
        {
            return Some(Ok(value));
        }
        Some(match op {
            ADDITION => Ok(left.add(right)),
            SUBTRACTION => Ok(left.sub(right)),
            MULTIPLICATION => left.mul(right),
            DIVISION => left.div_rem(right, true),
            REMAINDER => left.div_rem(right, false),
            EXPONENTIAL => left.pow(right),
            SHIFT_LEFT => left.shift(right, true),
            SHIFT_RIGHT => left.shift(right, false),
            BITWISE_OR => Ok(left.bitwise(right, Bitwise::Or)),
            BITWISE_XOR => Ok(left.bitwise(right, Bitwise::Xor)),
            BITWISE_AND => Ok(left.bitwise(right, Bitwise::And)),
            _ => return None,
        })
    }

    /// `binary_bigint` where it succeeds, for callers that leave failures to it: the result
    /// allocated, or `None`.
    pub fn bigint_binary_value(&mut self, op: u32, left: Value, right: Value) -> Option<Value> {
        let value = self.bigint_operation(op, left, right)?.ok()?;
        Some(self.heap.alloc_bigint(value))
    }

    /// `Date.now()`: the host clock's milliseconds, truncated. It runs no guest code, so the
    /// lane answers it in place (`steady_native_value`) as the native call does.
    pub fn date_now(&mut self) -> Value {
        Value::number(HostContext::new(&mut *self.host).invoke(CapabilityId::ClockMillis, None).trunc())
    }

    /// A relational operator (`<`, `<=`, `>`, `>=`) of two strings: IsLessThan orders them by
    /// code units, which runs no guest code. Identical Values name the same string, so they
    /// are equal without reading either; otherwise `order` compares them (in place when flat).
    pub fn string_relation(&self, op: u32, left: Value, right: Value) -> Option<bool> {
        let relation = NumberRelation::from_operator(op)?;
        let (left_string, right_string) = (self.heap.string(left)?, self.heap.string(right)?);
        if left == right {
            return relation.of_ordering(std::cmp::Ordering::Equal);
        }
        relation.of_ordering(left_string.order(right_string))
    }

    /// A relational operator (`<`, `<=`, `>`, `>=`) of two BigInt values.
    #[inline]
    pub fn bigint_relation(&self, op: u32, left: Value, right: Value) -> Option<bool> {
        // One cell read per operand: the pair's own digits order it (`JsBigInt::cmp`).
        let (Some(Cell::BigInt(left)), Some(Cell::BigInt(right))) = (self.heap.get(left), self.heap.get(right)) else { return None };
        NumberRelation::from_operator(op)?.of_ordering(left.cmp(right))
    }

    #[inline(never)]
    /// String concatenation for `+` once at least one operand is a string: the cached result
    /// when the dynamic-string cache knows the pair, otherwise both operands' string values
    /// joined.
    fn concatenate(
        &mut self,
        p: &ResidualProgram,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        if let Some(value) = self
            .memoized_concat(left, right)
            .or_else(|| self.intern_dynamic_concat(left, right))
            .or_else(|| self.short_ascii_concat(left, right))
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.string_concat(true);
            return Ok(value);
        }
        #[cfg(feature = "profile-aggregate")]
        self.profile.string_concat(false);
        // Not interned: hashing the result would read every unit of a growing `out += n`.
        let text = self.coerce_js_string(p, left)?.concat(&self.coerce_js_string(p, right)?);
        Ok(self.heap.alloc_string(text))
    }

    /// `+` of two operands that become strings without guest code, Numbers and strings,
    /// at least one a string: `concatenate`'s result. `None` for any other pair, or a result
    /// `concatenate` refuses, which leaves the operation to `binary_slow`.
    pub fn primitive_concatenation(
        &mut self,
        p: &ResidualProgram,
        left: Value,
        right: Value,
    ) -> Option<Value> {
        let (left_string, right_string) = (self.is_string(left), self.is_string(right));
        let converts = |string: bool, value: Value| string || value.as_number().is_some();
        if !(left_string || right_string) || !converts(left_string, left) || !converts(right_string, right) {
            return None;
        }
        self.concatenate(p, left, right).ok()
    }

    pub(super) fn binary_slow(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        if let Some(operator) = numeric_binary_operator(op)
            && let (Some(a), Some(b)) = (left.as_number(), right.as_number())
        {
            return Ok(Value::number(numeric_number_result(operator, a, b)));
        }
        if op == BinaryOperator::Addition as u32 && (self.is_string(left) || self.is_string(right))
        {
            return self.concatenate(p, left, right);
        }
        if let Some(operator) = RelationalOperator::from_immediate(op) {
            let result = self.compare_relational(p, operator, left, right)?;
            return Ok(Self::integrity_bool(result));
        }
        let answer = match op {
            op if op == BinaryOperator::Equality as u32 => self.equal(p, left, right)?,
            op if op == BinaryOperator::Inequality as u32 => !self.equal(p, left, right)?,
            op if op == BinaryOperator::StrictEquality as u32 => self.strict_equal(left, right),
            op if op == BinaryOperator::StrictInequality as u32 => !self.strict_equal(left, right),
            op if op == BinaryOperator::In as u32 => self.has_property(p, right, left)?,
            op if op == BinaryOperator::Instanceof as u32 => self.instanceof(p, left, right)?,
            _ => return Ok(Value::number(self.numeric(p, op, left, right)?)),
        };
        Ok(if answer { Value::TRUE } else { Value::FALSE })
    }
    #[inline(always)]
    pub fn binary_truthy(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<bool, JsError> {
        if let Some(result) = self.number_comparison_truthy(op, left, right) {
            return Ok(result);
        }
        let value = self.binary(p, op, left, right)?;
        Ok(self.truthy(value))
    }

    /// `binary_truthy` for a comparison of two numbers, which runs no guest code; `None` for
    /// any other operator or operand. The result is `number_relation`'s, profiled.
    #[inline(always)]
    pub(super) fn number_comparison_truthy(
        &mut self,
        op: u32,
        left: Value,
        right: Value,
    ) -> Option<bool> {
        let (holds, path) = number_relation(op, left, right)?;
        self.record_binary_value_path(op, left, right, path);
        Some(holds)
    }

    /// BigInt::add or BigInt::subtract of one to a BigInt `input`, which runs no guest code
    /// and allocates the result; `None` for any other value.
    pub(super) fn bigint_increment(&mut self, input: Value, decrement: bool) -> Option<Value> {
        let Some(Cell::BigInt(integer)) = self.heap.get(input) else {
            return None;
        };
        let one = crate::bigint::JsBigInt::from(1_u8);
        let next = if decrement { integer.sub(&one) } else { integer.add(&one) };
        Some(self.heap.alloc_bigint(next))
    }

    /// Shared numeric update after ToNumeric, used by bytecode and native hosts.
    pub(super) fn numeric_update(
        &mut self,
        p: &ResidualProgram,
        input: Value,
        decrement: bool,
    ) -> Result<Value, JsError> {
        let delta = if decrement { -1.0 } else { 1.0 };
        if let Some(next) = self.bigint_increment(input, decrement) {
            Ok(next)
        } else if let Some(integer) = input.as_int() {
            let next = if decrement {
                integer.checked_sub(1)
            } else {
                integer.checked_add(1)
            };
            Ok(next
                .map(Value::integer)
                .unwrap_or_else(|| Value::number(f64::from(integer) + delta)))
        } else {
            Ok(Value::number(self.to_number(p, input)? + delta))
        }
    }

    /// `identity_equality` decided from the operand words alone: identical non-Number
    /// words, or a record object against another record object or a nullish value. A record
    /// object is never `document.all` (`is_html_dda`), so loose and strict equality agree.
    /// `None` leaves the operands to `identity_equality`.
    #[inline(always)]
    pub fn word_identity_equality(op: u32, left: Value, right: Value) -> Option<bool> {
        let (_, negated) = equality_operator(op)?;
        let record = |value: Value| {
            value.direct_object_index().is_some() || value.direct_array_index().is_some()
        };
        let record_or_nullish =
            |value: Value| record(value) || value.is_null() || value.is_undefined();
        if left == right {
            return left.as_number().is_none().then_some(!negated);
        }
        ((record(left) && record_or_nullish(right)) || (record(right) && record_or_nullish(left)))
            .then_some(negated)
    }

    /// `==`, `!=`, `===` or `!==` when no conversion can decide it: a Number is strictly
    /// equal only to a Number; equal bits are the same value; distinct strict operands are
    /// equal only as strings or BigInts of equal contents, as are loose operands of one of
    /// those types; a loose null or undefined equals only null, undefined and [[IsHTMLDDA]]
    /// objects; two distinct objects are loosely unequal. `None` leaves two Numbers, which
    /// Number comparison decides, and every comparison that may coerce to the full algorithm.
    #[inline(always)]
    pub fn identity_equality(&self, op: u32, left: Value, right: Value) -> Option<bool> {
        let (strict, negated) = equality_operator(op)?;
        match (left.as_number().is_some(), right.as_number().is_some()) {
            // Number comparison decides two Numbers (NaN, signed zeros).
            (true, true) => return None,
            // A Number is strictly equal only to a Number; loose equality converts the other
            // operand, which can run guest code.
            (true, false) | (false, true) => return strict.then_some(negated),
            (false, false) => {}
        }
        let equal = if left == right {
            true
        } else if strict {
            // Distinct words are equal only as strings or BigInts of equal contents.
            self.strict_equal(left, right)
        } else {
            let nullish = |value: Value| value.is_null() || value.is_undefined();
            match (nullish(left), nullish(right)) {
                (true, true) => true,
                (true, false) => self.is_html_dda(right),
                (false, true) => self.is_html_dda(left),
                (false, false) => match (self.heap.get(left), self.heap.get(right)) {
                    // Operands of one type compare as `===` does.
                    _ if let (Some(l), Some(r)) = (self.heap.string(left), self.heap.string(right)) => l == r,
                    (Some(Cell::BigInt(left)), Some(Cell::BigInt(right))) => left == right,
                    _ if self.heap.is_symbol(left) && self.heap.is_symbol(right) => false,
                    // Two distinct Symbols, as a kind tag (`x.kind == Kind.A`) compares.
                    _ if self.is_object_like(left) && self.is_object_like(right) => false,
                    _ => return None,
                },
            }
        };
        Some(equal != negated)
    }

    pub(super) fn numeric(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<f64, JsError> {
        let a = self.to_number(p, left)?;
        let b = self.to_number(p, right)?;
        let operator = numeric_binary_operator(op)
            .ok_or_else(|| JsError(format!("unsupported binary operator {op}").into()))?;
        Ok(numeric_number_result(operator, a, b))
    }

    pub(super) fn call_argument_list(
        &mut self,
        p: &ResidualProgram,
        list: Value,
        allow_nullish: bool,
    ) -> Result<Vec<Value>, JsError> {
        if allow_nullish && (list.is_null() || list.is_undefined()) {
            return Ok(Vec::new());
        }
        self.create_list_from_array_like(p, list, ArrayLikeElementKind::Any)
    }

    pub(super) fn create_list_from_array_like(
        &mut self,
        p: &ResidualProgram,
        list: Value,
        element_kind: ArrayLikeElementKind,
    ) -> Result<Vec<Value>, JsError> {
        if !self.is_object_like(list) {
            return Err(self.type_error(p, "array-like list must be an object".into()));
        }
        // A hole-free array or arguments object answers every read below from its own data,
        // which no guest code observes.
        if let Some(values) = self.dense_apply_arguments(list)
            && (matches!(element_kind, ArrayLikeElementKind::Any)
                || values.iter().all(|value| {
                    self.heap.is_string(*value) || self.heap.is_symbol(*value)
                }))
        {
            return Ok(values);
        }
        let object = self.heap.root(list);
        let mut elements = Vec::new();
        let outcome = (|| {
            let length_atom = self.length_atom;
            let length_value =
                self.get_property(p, self.heap.root_value(object).unwrap(), length_atom)?;
            let length_value = self.heap.root(length_value);
            let length_number = self.to_number(p, self.heap.root_value(length_value).unwrap());
            self.heap.release_root(length_value);
            let length_number = length_number?;
            let length = crate::units::to_length(length_number);
            elements
                .try_reserve(length)
                .map_err(|_| self.type_error(p, "array-like list is too large".into()))?;
            for index in 0..length {
                let atom = self.index_key_atom(index);
                let element = self.get_property(p, self.heap.root_value(object).unwrap(), atom)?;
                if matches!(element_kind, ArrayLikeElementKind::PropertyKey)
                    && !self.heap.is_string(element)
                    && !self.heap.is_symbol(element)
                {
                    return Err(self
                        .type_error(p, "array-like list contains an invalid property key".into()));
                }
                elements.push(self.heap.root(element));
            }
            Ok(elements
                .iter()
                .map(|root| self.heap.root_value(*root).unwrap())
                .collect())
        })();
        for root in elements {
            self.heap.release_root(root);
        }
        self.heap.release_root(object);
        outcome
    }
}

/// `Number ** Number` (`Math.pow`), the value `staged::math::power` computes.
pub fn exponentiate(base: f64, exponent: f64) -> f64 {
    crate::staged::math::power(base, exponent)
}

/// The entry that runs a native's call (`Vm::call_native_dispatch`), one per family of
/// natives that share an implementation; `Table` for the natives no family claims.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(super) enum NativeFamily {
    /// Not derived yet (`Vm::native_families`).
    Unresolved,
    WasmExport,
    Generator,
    ArrayCallback,
    StringHtml,
    Finalization,
    Disposal,
    Collection,
    WasmJs,
    ShadowRealm,
    ShadowRealmCalled,
    WeakCalled,
    TypedArray,
    TypedArrayCalled,
    Atomics,
    Test262Agent,
    BigInt,
    Promise,
    /// The reaction natives of async-from-sync iteration and async generators' `return`.
    AsyncReaction,
    ObjectStatic,
    LazyDataProperty,
    ProxyRevoke,
    DataView,
    Table,
}

impl Vm {
/// `native`'s family, by the families' own membership tests in their precedence order: the
/// string, RegExp and Math methods belong to none of the families after them.
fn native_family(native: Native) -> NativeFamily {
    use NativeFamily as F;
    match native {
        Native::WasmExportedFunction => return F::WasmExport,
        Native::GeneratorNext | Native::GeneratorReturn | Native::GeneratorThrow => return F::Generator,
        _ => {}
    }
    if native.is_string_regexp_or_math() {
        return F::Table;
    }
    if native.is_array_callback() {
        return F::ArrayCallback;
    }
    if super::string::is_string_html_method(native) {
        return F::StringHtml;
    }
    if Self::is_finalization_native(native) {
        return F::Finalization;
    }
    if Self::is_disposal_native(native) {
        return F::Disposal;
    }
    if Self::is_collection_native(native) {
        return F::Collection;
    }
    #[cfg(feature = "wasm")]
    if super::wasm_js::is_wasm_js_native(native) {
        return F::WasmJs;
    }
    match native {
        Native::ShadowRealmEvaluate
        | Native::ShadowRealmImportValue
        | Native::ShadowRealmImportValueFulfilled
        | Native::ShadowRealmWrappedFunction => return F::ShadowRealm,
        Native::ShadowRealm => return F::ShadowRealmCalled,
        Native::WeakMap | Native::WeakSet | Native::WeakRef => return F::WeakCalled,
        _ => {}
    }
    if native.is_typed_array_method() || native.is_typed_array_iterator() {
        return F::TypedArray;
    }
    if native.is_typed_array_constructor() {
        return F::TypedArrayCalled;
    }
    if native.is_atomics_native() {
        return F::Atomics;
    }
    match native {
        Native::Test262Agent => return F::Test262Agent,
        _ if native.is_bigint_native() => return F::BigInt,
        _ if native.is_promise_native() => return F::Promise,
        Native::AsyncFromSyncValue
        | Native::AsyncFromSyncValueRejected
        | Native::AsyncGeneratorReturnFulfilled
        | Native::AsyncGeneratorReturnRejected
        | Native::AsyncGeneratorDelegateReturnStart => return F::AsyncReaction,
        _ if native.is_object_static() => return F::ObjectStatic,
        Native::LazyDataProperty => return F::LazyDataProperty,
        Native::ProxyRevoke => return F::ProxyRevoke,
        _ if native.is_data_view_native() => return F::DataView,
        _ => {}
    }
    F::Table
}
}

impl Vm {
    /// Write `console`'s arguments to the host's stream, separated by spaces: Node's console
    /// prints all of them rather than only the first.
    fn console_write(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        capability: crate::host::CapabilityId,
    ) -> Result<Value, JsError> {
        let mut text = String::new();
        for (index, argument) in args.iter().enumerate() {
            if index != 0 {
                text.push(' ');
            }
            let value = self.to_string(p, *argument)?;
            text.push_str(&value);
        }
        HostContext::new(&mut *self.host).invoke(capability, Some(&text));
        Ok(Value::UNDEFINED)
    }
}
