use super::*;
use oxc_ast::ast::{BinaryOperator, UnaryOperator};

/// Shared Number/BigInt content mismatch diagnostic.
pub(super) const MIXED_BIGINT_TYPES: &str = "Cannot mix BigInt and other types, use explicit conversions";

pub(super) enum ArrayLikeElementKind {
    Any,
    PropertyKey,
}

const EXPONENTIATION_ZERO: f64 = 0.0;
const EXPONENTIATION_ONE: f64 = 1.0;
const EXPONENTIATION_TWO: f64 = 2.0;
const ODD_INTEGER_PARITY: f64 = 1.0;
/// The comparison operators occupy the binary operator discriminants up to this one.
pub(super) const LAST_COMPARISON_OPERATOR: u32 = BinaryOperator::GreaterEqualThan as u32;

/// A stage supplies the machine operation for each semantic Number relation.
/// The relation table below is shared by interpreter values and native emitters.
pub(super) trait NumberRelationStage<T> {
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
        pub(super) enum NumberRelation { $($relation),+ }

        impl NumberRelation {
            #[inline(always)]
            pub(super) fn from_operator(op: u32) -> Option<Self> {
                $(
                    if $(op == BinaryOperator::$operator as u32)||+ {
                        return Some(Self::$relation);
                    }
                )+
                None
            }

            #[inline(always)]
            pub(super) fn apply<T, S: NumberRelationStage<T>>(
                self,
                stage: &mut S,
                left: T,
                right: T,
            ) -> S::Output {
                match self {
                    $(Self::$relation => stage.$stage_method(left, right)),+
                }
            }
        }
    };
}

number_relations! {
    Equal => [Equality, StrictEquality], equal;
    NotEqual => [Inequality, StrictInequality], not_equal;
    LessThan => [LessThan], less_than;
    LessEqual => [LessEqualThan], less_equal;
    GreaterThan => [GreaterThan], greater_than;
    GreaterEqual => [GreaterEqualThan], greater_equal;
}

/// The four IEEE Number arithmetic operations have one operator-to-method
/// mapping. Interpreter values and native Cranelift values supply the stage.
pub(super) trait NumberArithmeticStage<T> {
    type Output;

    fn add(&mut self, left: T, right: T) -> Self::Output;
    fn subtract(&mut self, left: T, right: T) -> Self::Output;
    fn multiply(&mut self, left: T, right: T) -> Self::Output;
    fn divide(&mut self, left: T, right: T) -> Self::Output;
}

macro_rules! number_arithmetic {
    ($($operation:ident => $operator:ident, $stage_method:ident, $specialized_instruction:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub(super) enum NumberArithmetic { $($operation),+ }

        impl NumberArithmetic {
            #[inline(always)]
            pub(super) fn from_operator(operator: u32) -> Option<Self> {
                $(
                    if operator == BinaryOperator::$operator as u32 {
                        return Some(Self::$operation);
                    }
                )+
                None
            }

            #[inline(always)]
            pub(super) fn apply<T, S: NumberArithmeticStage<T>>(
                self,
                stage: &mut S,
                left: T,
                right: T,
            ) -> S::Output {
                match self {
                    $(Self::$operation => stage.$stage_method(left, right)),+
                }
            }

            /// The Number arithmetic operation denoted by a JavaScript bytecode
            /// and its operator. General `Binary` accepts every arithmetic row;
            /// specialized bytecodes are accepted only for their corresponding
            /// operator, as recorded in this table.
            pub(super) fn from_js_instruction(instruction: Op, operator: u32) -> Option<Self> {
                if instruction == Op::Binary {
                    return Self::from_operator(operator);
                }
                $(number_arithmetic!(
                    @specialized_instruction
                    instruction,
                    operator,
                    Self::$operation,
                    $operator,
                    $specialized_instruction
                );)+
                None
            }
        }
    };
    (
        @specialized_instruction
        $instruction:ident,
        $operator:ident,
        $operation:expr,
        $operator_name:ident,
        None
    ) => {};
    (@specialized_instruction $instruction:ident, $operator:ident, $operation:expr, $operator_name:ident, $specialized_instruction:ident) => {
        if $instruction == Op::$specialized_instruction
            && $operator == BinaryOperator::$operator_name as u32
        {
            return Some($operation);
        }
    };
}

number_arithmetic! {
    Add => Addition, add, NumericAdd;
    Subtract => Subtraction, subtract, None;
    Multiply => Multiplication, multiply, NumericMultiply;
    Divide => Division, divide, None;
}

pub(super) trait NumberUnaryStage<T> {
    type Output;

    fn plus(&mut self, value: T) -> Self::Output;
    fn negate(&mut self, value: T) -> Self::Output;
}

macro_rules! number_unary {
    ($($operation:ident => $operator:ident, $stage_method:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum NumberUnary { $($operation),+ }

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
            pub(super) fn from_js_instruction(instruction: Op, operator: u32) -> Option<Self> {
                (instruction == Op::Unary)
                    .then(|| Self::from_operator(operator))
                    .flatten()
            }

            #[inline(always)]
            pub(super) fn apply<T, S: NumberUnaryStage<T>>(
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

number_unary! {
    Plus => UnaryPlus, plus;
    Negate => UnaryNegation, negate;
}

/// A stage supplies predicate operations for the shared JavaScript Number ToBoolean
/// denotation. Interpreter and codegen stages use the same nonzero-and-not-NaN formula.
pub(super) trait NumberTruthinessStage<T> {
    type Predicate;

    fn zero(&mut self) -> T;
    fn equal(&mut self, left: T, right: T) -> Self::Predicate;
    fn not_equal(&mut self, left: T, right: T) -> Self::Predicate;
    fn and(&mut self, left: Self::Predicate, right: Self::Predicate) -> Self::Predicate;
}

#[inline(always)]
pub(super) fn number_truthiness<T: Copy, S: NumberTruthinessStage<T>>(
    stage: &mut S,
    value: T,
) -> S::Predicate {
    let zero = stage.zero();
    let nonzero = stage.not_equal(value, zero);
    let not_nan = stage.equal(value, value);
    stage.and(nonzero, not_nan)
}

/// The int32 operations used by JavaScript bitwise expressions. Keeping their
/// operator mapping and operation selection in one stage lets the interpreter
/// and native emitters share the same signed/unsigned shift decisions.
pub(super) trait NumberBitwiseStage<T> {
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
        pub(super) enum NumberBitwise { $($operation),+ }

        impl NumberBitwise {
            /// `from_operator` of a decoded operator, as one match.
            #[inline(always)]
            pub(super) fn from_binary_operator(operator: BinaryOperator) -> Option<Self> {
                match operator {
                    $(BinaryOperator::$operator => Some(Self::$operation),)+
                    _ => None,
                }
            }

            pub(super) fn from_operator(operator: u32) -> Option<Self> {
                $(
                    if operator == BinaryOperator::$operator as u32 {
                        return Some(Self::$operation);
                    }
                )+
                None
            }

            /// The signed int32 operation denoted by an eligible JavaScript
            /// bytecode and operator. Bitwise operands may be narrowed only
            /// for the general `Binary` instruction; entry guards are checked
            /// separately by the caller.
            pub(super) fn from_js_instruction(instruction: Op, operator: u32) -> Option<Self> {
                (instruction == Op::Binary)
                    .then(|| Self::from_operator(operator))
                    .flatten()
            }

            /// Whether this operation produces a signed Int32 from signed Int32
            /// inputs, which is the current entry guard's proof domain.
            pub(super) const fn in_signed_int32_domain(self) -> bool {
                matches!(self, Self::And | Self::Or | Self::Xor)
            }

            #[inline(always)]
            pub(super) fn apply<T, S: NumberBitwiseStage<T>>(
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

number_bitwise! {
    And => BitwiseAnd, and;
    Or => BitwiseOR, or;
    Xor => BitwiseXOR, xor;
    ShiftLeft => ShiftLeft, shift_left;
    ShiftRight => ShiftRight, shift_right;
    ShiftRightUnsigned => ShiftRightZeroFill, shift_right_unsigned;
}

struct I32BitwiseStage;

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
        ((left as u32).wrapping_shl((right as u32) & 31)) as i32
    }
    #[inline(always)]
    fn shift_right(&mut self, left: i32, right: i32) -> Self::Output {
        left >> ((right as u32) & 31)
    }
    #[inline(always)]
    fn shift_right_unsigned(&mut self, left: i32, right: i32) -> Self::Output {
        ((left as u32) >> ((right as u32) & 31)) as i32
    }
}

#[inline(always)]
fn number_bitwise_f64_result(operation: NumberBitwise, left: i32, right: i32) -> f64 {
    let bits = operation.apply(&mut I32BitwiseStage, left, right);
    if operation == NumberBitwise::ShiftRightUnsigned {
        f64::from(bits as u32)
    } else {
        f64::from(bits)
    }
}

/// `operation` of int32 operands as a value: the int32 result as it is, but for a zero-fill
/// shift above `i32::MAX`, which is the Number of its unsigned bits.
#[inline(always)]
fn number_bitwise_result(operation: NumberBitwise, left: i32, right: i32) -> Value {
    let bits = operation.apply(&mut I32BitwiseStage, left, right);
    if operation == NumberBitwise::ShiftRightUnsigned && bits < 0 {
        Value::number(f64::from(bits as u32))
    } else {
        Value::integer(bits)
    }
}

struct NumberF64Stage;

impl NumberArithmeticStage<f64> for NumberF64Stage {
    type Output = f64;

    #[inline(always)]
    fn add(&mut self, left: f64, right: f64) -> Self::Output {
        left + right
    }
    #[inline(always)]
    fn subtract(&mut self, left: f64, right: f64) -> Self::Output {
        left - right
    }
    #[inline(always)]
    fn multiply(&mut self, left: f64, right: f64) -> Self::Output {
        left * right
    }
    #[inline(always)]
    fn divide(&mut self, left: f64, right: f64) -> Self::Output {
        left / right
    }
}

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

#[inline(always)]
pub(super) fn number_unary_result(operator: u32, value: f64) -> Option<f64> {
    Some(NumberUnary::from_operator(operator)?.apply(&mut NumberF64Stage, value))
}

#[inline(always)]
fn number_arithmetic_result(operation: NumberArithmetic, left: f64, right: f64) -> f64 {
    operation.apply(&mut NumberF64Stage, left, right)
}

struct PartialOrderStage;

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

/// A comparison operator applied to two numbers, or to two integers, which compare as the
/// numbers they denote.
#[inline(always)]
fn number_comparison<T: PartialOrd>(op: u32, a: T, b: T) -> bool {
    NumberRelation::from_operator(op)
        .expect("comparison operators end at GreaterEqualThan")
        .apply(&mut PartialOrderStage, a, b)
}

const FIRST_NUMERIC_BINARY_OPERATOR: u32 = BinaryOperator::Addition as u32;
const LAST_NUMERIC_BINARY_OPERATOR: u32 = BinaryOperator::BitwiseAnd as u32;
const NUMERIC_BINARY_OPERATOR_COUNT: usize =
    (LAST_NUMERIC_BINARY_OPERATOR - FIRST_NUMERIC_BINARY_OPERATOR + 1) as usize;
const NUMERIC_BINARY_OPERATORS: [BinaryOperator; NUMERIC_BINARY_OPERATOR_COUNT] = [
    BinaryOperator::Addition,
    BinaryOperator::Subtraction,
    BinaryOperator::Multiplication,
    BinaryOperator::Division,
    BinaryOperator::Remainder,
    BinaryOperator::Exponential,
    BinaryOperator::ShiftLeft,
    BinaryOperator::ShiftRight,
    BinaryOperator::ShiftRightZeroFill,
    BinaryOperator::BitwiseOR,
    BinaryOperator::BitwiseXOR,
    BinaryOperator::BitwiseAnd,
];

/// Binary operator `op` of two integers where the integers alone decide its result: a
/// comparison's boolean through the shared relation definition, or the integer operators'
/// result (`numeric_integer_result`); `None` for any other operator and for a result the
/// integer case leaves to Numbers.
#[inline(always)]
pub(super) fn integer_binary(op: u32, left: i32, right: i32) -> Option<Value> {
    if let Some(relation) = NumberRelation::from_operator(op) {
        return Some(if relation.apply(&mut PartialOrderStage, left, right) {
            Value::TRUE
        } else {
            Value::FALSE
        });
    }
    numeric_integer_result(numeric_binary_operator(op)?, left, right)
}

/// `number_relation` of two integers alone: `None` for any other pair. Few values stay live,
/// so a lane handler inlining it saves no registers.
#[inline(always)]
pub(super) fn integer_relation(op: u32, left: Value, right: Value) -> Option<bool> {
    if op > LAST_COMPARISON_OPERATOR {
        return None;
    }
    let (a, b) = Value::int_pair(left, right)?;
    Some(number_comparison(op, a, b))
}

/// A comparison operator on two Numbers, comparing integers as integers; `None` for any
/// other operator or value.
#[inline(always)]
pub(super) fn number_relation(op: u32, left: Value, right: Value) -> Option<bool> {
    if op > LAST_COMPARISON_OPERATOR {
        return None;
    }
    if let Some((a, b)) = Value::int_pair(left, right) {
        return Some(number_comparison(op, a, b));
    }
    // Equality, strict equality and the relational operators agree with IEEE 754
    // comparison on numbers: NaN is unordered and unequal, and -0 equals +0.
    Some(number_comparison(op, left.as_number()?, right.as_number()?))
}

#[inline(always)]
fn numeric_binary_operator(immediate: u32) -> Option<BinaryOperator> {
    let index = immediate.checked_sub(FIRST_NUMERIC_BINARY_OPERATOR)? as usize;
    let operator = *NUMERIC_BINARY_OPERATORS.get(index)?;
    (operator as u32 == immediate).then_some(operator)
}

#[inline(always)]
fn is_numeric_binary_operator(immediate: u32) -> bool {
    (FIRST_NUMERIC_BINARY_OPERATOR..=LAST_NUMERIC_BINARY_OPERATOR).contains(&immediate)
}

#[inline(always)]
fn numeric_number_result(operator: BinaryOperator, left: f64, right: f64) -> f64 {
    if let Some(operation) = NumberArithmetic::from_operator(operator as u32) {
        return number_arithmetic_result(operation, left, right);
    }
    if let Some(operation) = NumberBitwise::from_operator(operator as u32) {
        let left = number_to_u32(left) as i32;
        let right = number_to_u32(right) as i32;
        return number_bitwise_f64_result(operation, left, right);
    }
    match operator {
        BinaryOperator::Remainder => left % right,
        BinaryOperator::Exponential => exponentiate(left, right),
        _ => unreachable!("numeric operator table only contains numeric operators"),
    }
}

#[cfg(test)]
mod numeric_instruction_tests {
    use super::{NumberArithmetic, NumberBitwise, NumberUnary, Op, number_is_truthy};
    use oxc_ast::ast::{BinaryOperator, UnaryOperator};

    #[test]
    fn arithmetic_bytecode_denotations_come_from_the_operation_table() {
        assert_eq!(
            NumberArithmetic::from_js_instruction(Op::Binary, BinaryOperator::Addition as u32),
            Some(NumberArithmetic::Add)
        );
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
            assert_eq!(number_is_truthy(value), expected);
        }
    }
}

#[inline(always)]
pub(super) fn numeric_integer_add_result(left: i32, right: i32) -> Value {
    match left.checked_add(right) {
        Some(sum) => Value::integer(sum),
        None => Value::number(f64::from(left) + f64::from(right)),
    }
}

#[inline(always)]
pub(super) fn numeric_integer_multiply_result(left: i32, right: i32) -> Value {
    match left.checked_mul(right) {
        Some(0) if (left < 0) != (right < 0) => Value::number(-0.0),
        Some(product) => Value::integer(product),
        None => Value::number(f64::from(left) * f64::from(right)),
    }
}

#[inline(always)]
fn numeric_integer_subtract_result(left: i32, right: i32) -> Value {
    left.checked_sub(right)
        .map(Value::integer)
        .unwrap_or_else(|| Value::number(f64::from(left) - f64::from(right)))
}

/// An arithmetic operator on two Numbers: `integer` when both are integers, else `number` on
/// their IEEE 754 values.
pub(super) const ADDITION: u32 = BinaryOperator::Addition as u32;
pub(super) const SUBTRACTION: u32 = BinaryOperator::Subtraction as u32;
pub(super) const MULTIPLICATION: u32 = BinaryOperator::Multiplication as u32;
pub(super) const DIVISION: u32 = BinaryOperator::Division as u32;

pub(super) const REMAINDER: u32 = BinaryOperator::Remainder as u32;
pub(super) const SHIFT_LEFT: u32 = BinaryOperator::ShiftLeft as u32;
pub(super) const SHIFT_RIGHT: u32 = BinaryOperator::ShiftRight as u32;
pub(super) const SHIFT_RIGHT_ZERO_FILL: u32 = BinaryOperator::ShiftRightZeroFill as u32;
pub(super) const BITWISE_OR: u32 = BinaryOperator::BitwiseOR as u32;
pub(super) const BITWISE_XOR: u32 = BinaryOperator::BitwiseXOR as u32;
pub(super) const BITWISE_AND: u32 = BinaryOperator::BitwiseAnd as u32;

/// Operator `OP` on two Numbers through their double values, which is how ECMA-262 defines
/// every numeric operator's result (`Value::number` stores the integral ones as integers);
/// `None` for any other operand.
#[inline(always)]
pub(super) fn number_value_operator<const OP: u32>(left: Value, right: Value) -> Option<Value> {
    let (left, right) = Value::number_pair(left, right)?;
    Some(Value::number(numeric_number_result(
        numeric_binary_operator(OP)?,
        left,
        right,
    )))
}

/// Integer operator `OP` (`numeric_integer_result`) when both operands are integers;
/// `None` otherwise, leaving other Numbers to `number_binary`.
#[inline(always)]
pub(super) fn integer_operator<const OP: u32>(left: Value, right: Value) -> Option<Value> {
    // One operand at a time: each tag test needs a single scratch register, which keeps the
    // lane's integer handlers free of saved registers.
    let (left, right) = (left.as_int()?, right.as_int()?);
    numeric_integer_result(numeric_binary_operator(OP)?, left, right)
}

#[inline(always)]
fn number_arithmetic_values(
    operation: NumberArithmetic,
    left: Value,
    right: Value,
) -> Option<(Value, crate::profile::BinaryValuePath)> {
    if operation != NumberArithmetic::Divide
        && let Some((left, right)) = Value::int_pair(left, right)
    {
        let value = match operation {
            NumberArithmetic::Add => numeric_integer_add_result(left, right),
            NumberArithmetic::Subtract => numeric_integer_subtract_result(left, right),
            NumberArithmetic::Multiply => numeric_integer_multiply_result(left, right),
            NumberArithmetic::Divide => unreachable!("division uses its Number fast path"),
        };
        return Some((value, crate::profile::BinaryValuePath::IntegerFastPath));
    }
    let (left, right) = Value::number_pair(left, right)?;
    Some((
        Value::number(number_arithmetic_result(operation, left, right)),
        crate::profile::BinaryValuePath::NumberFastPath,
    ))
}

#[inline(always)]
fn numeric_integer_result(operator: BinaryOperator, left: i32, right: i32) -> Option<Value> {
    Some(match operator {
        BinaryOperator::Addition => numeric_integer_add_result(left, right),
        BinaryOperator::Subtraction => numeric_integer_subtract_result(left, right),
        BinaryOperator::Multiplication => numeric_integer_multiply_result(left, right),
        BinaryOperator::Division => {
            if right == 0 || (left == 0 && right < 0) {
                return None;
            }
            let remainder = left.checked_rem(right)?;
            if remainder != 0 {
                return None;
            }
            Value::integer(left.checked_div(right)?)
        }
        BinaryOperator::Remainder => {
            if right == 0 {
                return None;
            }
            let remainder = left.checked_rem(right)?;
            if remainder == 0 && left < 0 {
                Value::number(-0.0)
            } else {
                Value::integer(remainder)
            }
        }
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
        match (self, ordering) {
            (Self::LessThan, Less) | (Self::LessEqual, Less | Equal) => true,
            (Self::GreaterThan, Greater) | (Self::GreaterEqual, Greater | Equal) => true,
            _ => false,
        }
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

impl<H: Host> Vm<H> {
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

    #[inline(always)]
    pub(super) fn numeric_integer_binary(
        &self,
        operator: u32,
        left: Value,
        right: Value,
    ) -> Option<Value> {
        const ADDITION: u32 = BinaryOperator::Addition as u32;
        const SUBTRACTION: u32 = BinaryOperator::Subtraction as u32;
        const MULTIPLICATION: u32 = BinaryOperator::Multiplication as u32;
        let (left, right) = Value::int_pair(left, right)?;
        // The common operators match the immediate directly; the rest go through the table.
        match operator {
            ADDITION => Some(numeric_integer_add_result(left, right)),
            SUBTRACTION => Some(numeric_integer_subtract_result(left, right)),
            MULTIPLICATION => Some(numeric_integer_multiply_result(left, right)),
            _ => numeric_integer_result(numeric_binary_operator(operator)?, left, right),
        }
    }

    pub(super) fn call_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.check_execution_interrupt()?;
        let result = self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            let receiver = vm.string_method_receiver(p, native, this)?;
            if receiver == this {
                return vm.call_native_dispatch(p, native, receiver, args);
            }
            vm.with_call_roots([receiver], |vm| {
                vm.call_native_dispatch(p, native, receiver, args)
            })
        });
        // String natives make text on demand; charge it before the caller's next safepoint.
        self.heap.charge_materialized_text();
        result
    }

    fn call_native_dispatch(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        // The string and RegExp methods belong to none of the families below.
        if native.is_string_regexp_or_math() {
            return self.call_native_table(p, native, this, args);
        }
        if let Some(result) = self.string_html_method(p, native, this, args) {
            return result;
        }
        if Self::is_finalization_native(native) {
            return self.call_finalization_registry_native(p, native, this, args);
        }
        if Self::is_disposal_native(native) {
            return self.call_disposal_native(p, native, this, args);
        }
        if Self::is_collection_native(native) {
            return self.call_collection_native(p, native, this, args);
        }
        if super::wasm_js::is_wasm_js_native(native) {
            return self.call_wasm_js_native(p, native, this, args);
        }
        if matches!(
            native,
            Native::ShadowRealmEvaluate
                | Native::ShadowRealmImportValue
                | Native::ShadowRealmImportValueFulfilled
                | Native::ShadowRealmWrappedFunction
        ) {
            return self.shadow_realm_native(p, native, this, args);
        }
        if native == Native::ShadowRealm {
            return Err(self.type_error(p, "ShadowRealm constructor requires 'new'".into()));
        }
        if matches!(native, Native::WeakMap | Native::WeakSet | Native::WeakRef) {
            let constructor = match native {
                Native::WeakMap => "WeakMap",
                Native::WeakSet => "WeakSet",
                Native::WeakRef => "WeakRef",
                _ => unreachable!(),
            };
            return Err(self.type_error(
                p,
                format!("Constructor {constructor} requires 'new'").into(),
            ));
        }
        if let Some(result) = self.maybe_call_typed_array_native(p, native, this, args) {
            return result;
        }
        if native.is_typed_array_constructor() {
            return Err(self.type_error(p, "typed array constructor requires new".into()));
        }
        if native.is_atomics_native() {
            return self.atomics_native(p, native, args);
        }
        if native == Native::Test262Agent {
            return self.call_test262_agent(p, args);
        }
        if native.is_bigint_native() {
            return self.bigint_native(p, native, this, args);
        }
        if native.is_promise_native() {
            return self.call_promise_native(p, native, this, args);
        }
        if native == Native::AsyncFromSyncValue {
            return self.async_from_sync_value(args);
        }
        if native == Native::AsyncFromSyncValueRejected {
            return self.async_from_sync_value_rejected(p, args);
        }
        if native == Native::AsyncGeneratorReturnFulfilled {
            return self.async_generator_return_fulfilled(p, args);
        }
        if native == Native::AsyncGeneratorReturnRejected {
            return self.async_generator_return_rejected(p, args);
        }
        if native == Native::AsyncGeneratorDelegateReturnStart {
            return self.async_generator_delegate_return_start(p, args);
        }
        if native.is_object_static() {
            return self.call_object_native(p, native, args);
        }
        if native == Native::LazyDataProperty {
            return self.lazy_data_property_access(p, this, args);
        }
        if native == Native::TemporalInitialize {
            self.install_temporal(p)?;
            return Ok(Value::UNDEFINED);
        }
        if native == Native::ProxyRevoke {
            let callee = self
                .active_native_callable()
                .ok_or_else(|| JsError("proxy revoke requires an active callee".into()))?;
            return self.proxy_revoke(callee);
        }
        if native.is_data_view_native() {
            return self.data_view_native(p, native, this, args);
        }
        self.call_native_table(p, native, this, args)
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
                self.with_scope_entered = true;
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
            native if native.is_host_control_native() => self.call_host(p, native, args),
            Native::WasmHost => self.call_wasm_host(args),
            Native::HostFunction => self.call_host_function(this, args),
            Native::Print => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, v)?;
                HostContext::new(&mut self.host).invoke(CapabilityId::WriteLine, Some(&text));
                Ok(Value::UNDEFINED)
            }
            Native::Date => self.date_call(),
            Native::IntlNumberFormat => self.intl_number_format_call(p, this, args),
            Native::IntlNumberFormatSupportedLocalesOf
            | Native::IntlDisplayNamesSupportedLocalesOf => {
                self.intl_supported_locales_of(p, args, super::intl_number::is_supported_locale)
            }
            Native::IntlNumberFormatFormatGetter => self.intl_number_format_format_getter(p, this),
            Native::IntlNumberFormatFormat => self.intl_number_format_format(p, this, args),
            Native::IntlNumberFormatFormatToParts => {
                self.intl_number_format_format_to_parts(p, this, args)
            }
            Native::IntlNumberFormatFormatRange => {
                self.intl_number_format_format_range(p, this, args)
            }
            Native::IntlNumberFormatFormatRangeToParts => {
                self.intl_number_format_format_range_to_parts(p, this, args)
            }
            Native::IntlNumberFormatResolvedOptions => {
                self.intl_number_format_resolved_options(p, this)
            }
            Native::IntlPluralRules => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlPluralRulesSupportedLocalesOf
            | Native::IntlPluralRulesSelect
            | Native::IntlPluralRulesSelectRange
            | Native::IntlPluralRulesResolvedOptions => {
                self.intl_plural_rules_native(p, native, this, args)
            }
            Native::IntlGetCanonicalLocales
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
            | Native::IntlLocaleNumericGetter => self.intl_namespace_native(p, native, this, args),
            Native::IntlRelativeTimeFormat => {
                self.intl_relative_time_format_native(p, native, this, args)
            }
            Native::IntlRelativeTimeFormatResolvedOptions => {
                self.intl_relative_time_format_resolved_options(p, this)
            }
            Native::IntlRelativeTimeFormatSupportedLocalesOf => {
                self.intl_supported_locales_of(p, args, super::intl_number::is_supported_locale)
            }
            Native::IntlRelativeTimeFormatFormat | Native::IntlRelativeTimeFormatFormatToParts => {
                self.intl_relative_time_format_native(p, native, this, args)
            }
            Native::IntlSegmenter => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlSegmenterSupportedLocalesOf
            | Native::IntlSegmenterSegment
            | Native::IntlSegmenterResolvedOptions
            | Native::IntlSegmenterSegmentsIterator
            | Native::IntlSegmenterSegmentsContaining
            | Native::IntlSegmenterIteratorNext => {
                self.intl_segmenter_native(p, native, this, args)
            }
            Native::IntlCollator
            | Native::IntlCollatorSupportedLocalesOf
            | Native::IntlCollatorCompareGetter
            | Native::IntlCollatorCompare
            | Native::IntlCollatorResolvedOptions => {
                self.intl_collator_native(p, native, this, args)
            }
            Native::IntlDateTimeFormat => self.intl_date_time_format_call(p, this, args),
            Native::IntlDisplayNames => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlDisplayNamesOf | Native::IntlDisplayNamesResolvedOptions => {
                self.intl_display_names_native(p, native, this, args)
            }
            Native::IntlDurationFormat => {
                Err(self.type_error(p, "constructor requires new".into()))
            }
            Native::IntlDurationFormatSupportedLocalesOf => {
                self.intl_supported_locales_of(p, args, super::intl_number::is_supported_locale)
            }
            Native::IntlDurationFormatFormatGetter
            | Native::IntlDurationFormatFormat
            | Native::IntlDurationFormatFormatToParts
            | Native::IntlDurationFormatResolvedOptions => {
                self.intl_duration_format_native(p, native, this, args)
            }
            Native::IntlListFormat => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlListFormatFormatGetter
            | Native::IntlListFormatFormat
            | Native::IntlListFormatFormatToParts
            | Native::IntlListFormatResolvedOptions
            | Native::IntlListFormatSupportedLocalesOf => {
                self.intl_list_format_native(p, native, this, args)
            }
            Native::IntlDateTimeFormatFormatGetter
            | Native::IntlDateTimeFormatFormat
            | Native::IntlDateTimeFormatFormatToParts
            | Native::IntlDateTimeFormatFormatRange
            | Native::IntlDateTimeFormatFormatRangeToParts
            | Native::IntlDateTimeFormatSupportedLocalesOf
            | Native::IntlDateTimeFormatResolvedOptions => {
                self.intl_date_time_format_native(p, native, this, args)
            }
            Native::DateNow => Ok(Value::number(
                HostContext::new(&mut self.host)
                    .invoke(CapabilityId::ClockMillis, None)
                    .trunc(),
            )),
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
            Native::MathLog => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(self.to_number(p, v)?.ln()))
            }
            Native::MathPow => {
                let a = args.first().copied().unwrap_or(Value::UNDEFINED);
                let b = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(exponentiate(
                    self.to_number(p, a)?,
                    self.to_number(p, b)?,
                )))
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
                let text = if let Some(Cell::Symbol(description)) = self.heap.get(argument) {
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
            Native::TemporalToLocaleString => match self.heap.get(this) {
                Some(Cell::TemporalDuration { .. }) => {
                    self.temporal_duration_to_locale_string(p, this, args)
                }
                _ => self.temporal_to_locale_string(p, this, args),
            },
            Native::TemporalDuration
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
            | Native::TemporalDurationBlankGetter => {
                self.temporal_duration_native(p, native, this, args)
            }
            Native::TemporalPlainDate
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
            | Native::TemporalPlainDateValueOf => {
                self.temporal_plain_date_native(p, native, this, args)
            }
            Native::TemporalPlainDateAdd | Native::TemporalPlainDateSubtract => {
                self.temporal_plain_date_arithmetic(p, native, this, args)
            }
            Native::TemporalPlainDateUntil | Native::TemporalPlainDateSince => {
                self.temporal_plain_date_difference(p, native, this, args)
            }
            Native::TemporalPlainDateTime
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
            | Native::TemporalPlainDateTimeEquals => {
                self.temporal_plain_date_time_native(p, native, this, args)
            }
            Native::TemporalPlainTime
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
            | Native::TemporalPlainTimeNanosecondGetter => {
                self.temporal_plain_time_native(p, native, this, args)
            }
            Native::TemporalPlainTimeValueOf => {
                self.temporal_plain_time_native(p, native, this, args)
            }
            Native::TemporalPlainTimeRound
            | Native::TemporalPlainTimeUntil
            | Native::TemporalPlainTimeSince
            | Native::TemporalPlainTimeToString
            | Native::TemporalPlainTimeToJSON
            | Native::TemporalPlainTimeWith => {
                self.temporal_plain_time_native(p, native, this, args)
            }
            Native::TemporalPlainMonthDay
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
            | Native::TemporalPlainYearMonthToPlainDate => {
                self.temporal_calendar_projection_native(p, native, this, args)
            }
            Native::TemporalPlainMonthDayToString
            | Native::TemporalPlainMonthDayToJSON
            | Native::TemporalPlainMonthDayToLocaleString
            | Native::TemporalPlainYearMonthToString
            | Native::TemporalPlainYearMonthToJSON
            | Native::TemporalPlainYearMonthToLocaleString => {
                super::temporal_date_projection::native(self, p, native, this, args)
            }
            Native::TemporalZonedDateTime
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
            | Native::TemporalZonedDateTimeOffsetNanosecondsGetter => {
                self.temporal_zoned_date_time_native(p, native, this, args)
            }
            Native::TemporalInstant
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
            | Native::TemporalInstantToZonedDateTimeISO => {
                self.temporal_instant_native(p, native, this, args)
            }
            Native::TemporalNowInstant
            | Native::TemporalNowPlainDateISO
            | Native::TemporalNowPlainDateTimeISO
            | Native::TemporalNowPlainTimeISO
            | Native::TemporalNowTimeZoneId
            | Native::TemporalNowZonedDateTimeISO => self.temporal_now_native(p, native, args),
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
            _ => self.call_primitive_native(p, native, this, args),
        }
    }
    /// An arithmetic, bitwise or shift operator applied to two numbers: the integer fast path
    /// when both are integers, otherwise the IEEE 754 result. Any other operand pair, and
    /// every other operator, is left to `binary`. The four arithmetic operators match the
    /// shared arithmetic stage; the rest go through the numeric operator table.
    #[inline(always)]
    pub(super) fn number_binary(&mut self, op: u32, left: Value, right: Value) -> Option<Value> {
        let Some(operation) = NumberArithmetic::from_operator(op) else {
            return self.tabled_number_binary(op, left, right);
        };
        let (value, path) = number_arithmetic_values(operation, left, right)?;
        self.record_binary_value_path(op, left, right, path);
        Some(value)
    }

    /// `number_binary` for the operators past the four arithmetic ones.
    #[inline(always)]
    fn tabled_number_binary(&mut self, op: u32, left: Value, right: Value) -> Option<Value> {
        if !is_numeric_binary_operator(op) {
            return None;
        }
        if let Some(value) = self.numeric_integer_binary(op, left, right) {
            self.record_binary_value_path(
                op,
                left,
                right,
                crate::profile::BinaryValuePath::IntegerFastPath,
            );
            return Some(value);
        }
        let (left_number, right_number) = Value::number_pair(left, right)?;
        let operator = numeric_binary_operator(op)
            .expect("numeric operator range is represented in the operator table");
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
    pub(super) fn binary(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        // The concatenation cache holds only string pairs, so a heap left operand probes it
        // before the Number paths it could never take.
        if op == BinaryOperator::Addition as u32
            && left.is_heap()
            && let Some(value) = self.cached_concat(left, right)
        {
            #[cfg(feature = "profile-aggregate")]
            self.profile.concat_cache(true);
            return Ok(value);
        }
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
            if let (Some(left_number), Some(right_number)) = (left.as_number(), right.as_number()) {
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
        if op <= BinaryOperator::StrictInequality as u32
            && let Some((a, b)) = Value::int_pair(left, right)
        {
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
        if op <= BinaryOperator::StrictInequality as u32
            && let (Some(a), Some(b)) = (left.as_number(), right.as_number())
        {
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
                    "BigInt operation is not supported"
                } else {
                    MIXED_BIGINT_TYPES
                }
                .into(),
            ));
        };
        match result {
            Ok(value) => Ok(self.heap.alloc(Cell::bigint(value))),
            Err(crate::bigint::Error::DivisionByZero) => {
                Err(self.range_error(p, "Division by zero".into()))
            }
            Err(crate::bigint::Error::NegativeExponent) => {
                Err(self.range_error(p, "Exponent must be positive".into()))
            }
            Err(crate::bigint::Error::ExponentTooLarge) => {
                Err(self.range_error(p, "Maximum BigInt size exceeded".into()))
            }
        }
    }

    fn is_bigint(&self, value: Value) -> bool {
        matches!(self.heap.get(value), Some(Cell::BigInt(_)))
    }

    /// BigInt operator `op` of two BigInt values: the result, or the error it raises. `None`
    /// when an operand is not a BigInt or the operator has no BigInt meaning.
    fn bigint_operation(
        &self,
        op: u32,
        left: Value,
        right: Value,
    ) -> Option<Result<num_bigint::BigInt, crate::bigint::Error>> {
        const EXPONENTIAL: u32 = BinaryOperator::Exponential as u32;
        let (Some(Cell::BigInt(left)), Some(Cell::BigInt(right))) =
            (self.heap.get(left), self.heap.get(right))
        else {
            return None;
        };
        let (left, right): (&num_bigint::BigInt, &num_bigint::BigInt) = (left, right);
        let nonzero_divisor = || {
            if right.sign() == num_bigint::Sign::NoSign {
                Err(crate::bigint::Error::DivisionByZero)
            } else {
                Ok(right)
            }
        };
        Some(match op {
            ADDITION => Ok(left + right),
            SUBTRACTION => Ok(left - right),
            MULTIPLICATION => Ok(left * right),
            DIVISION => nonzero_divisor().map(|right| left / right),
            REMAINDER => nonzero_divisor().map(|right| left % right),
            EXPONENTIAL => {
                if right.sign() == num_bigint::Sign::Minus {
                    Err(crate::bigint::Error::NegativeExponent)
                } else {
                    u32::try_from(right.magnitude())
                        .map(|exponent| left.pow(exponent))
                        .map_err(|_| crate::bigint::Error::ExponentTooLarge)
                }
            }
            SHIFT_LEFT => crate::bigint::shift(left, right, true),
            SHIFT_RIGHT => crate::bigint::shift(left, right, false),
            BITWISE_OR => Ok(left | right),
            BITWISE_XOR => Ok(left ^ right),
            BITWISE_AND => Ok(left & right),
            _ => return None,
        })
    }

    /// `binary_bigint` where it succeeds, for callers that leave failures to it: the result
    /// allocated, or `None`.
    pub(super) fn bigint_binary_value(&mut self, op: u32, left: Value, right: Value) -> Option<Value> {
        let value = self.bigint_operation(op, left, right)?.ok()?;
        Some(self.heap.alloc(Cell::bigint(value)))
    }

    /// A relational operator (`<`, `<=`, `>`, `>=`) of two strings: IsLessThan orders them by
    /// code units, which runs no guest code.
    pub(super) fn string_relation(&self, op: u32, left: Value, right: Value) -> Option<bool> {
        let (left, right) = (self.heap.string(left)?, self.heap.string(right)?);
        let ordering = left.units().cmp(&right.units());
        Some(match NumberRelation::from_operator(op)? {
            NumberRelation::LessThan => ordering.is_lt(),
            NumberRelation::LessEqual => ordering.is_le(),
            NumberRelation::GreaterThan => ordering.is_gt(),
            NumberRelation::GreaterEqual => ordering.is_ge(),
            NumberRelation::Equal | NumberRelation::NotEqual => return None,
        })
    }

    /// A relational operator (`<`, `<=`, `>`, `>=`) of two BigInt values.
    pub(super) fn bigint_relation(&self, op: u32, left: Value, right: Value) -> Option<bool> {
        let (Some(Cell::BigInt(left)), Some(Cell::BigInt(right))) =
            (self.heap.get(left), self.heap.get(right))
        else {
            return None;
        };
        let ordering = left.cmp(right);
        Some(match NumberRelation::from_operator(op)? {
            NumberRelation::LessThan => ordering.is_lt(),
            NumberRelation::LessEqual => ordering.is_le(),
            NumberRelation::GreaterThan => ordering.is_gt(),
            NumberRelation::GreaterEqual => ordering.is_ge(),
            NumberRelation::Equal | NumberRelation::NotEqual => return None,
        })
    }

    #[cold]
    #[inline(never)]
    /// String concatenation for `+` once at least one operand is a string: an interned
    /// result when the dynamic-string cache knows the pair, otherwise both operands' string
    /// values joined and interned.
    fn concatenate(
        &mut self,
        p: &ResidualProgram,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        if let Some(value) = self.intern_dynamic_concat(left, right) {
            #[cfg(feature = "profile-aggregate")]
            self.profile.string_concat(true);
            return Ok(value);
        }
        #[cfg(feature = "profile-aggregate")]
        self.profile.string_concat(false);
        let mut text = self.coerce_js_string(p, left)?;
        text.push_js_string(&self.coerce_js_string(p, right)?);
        Ok(self.intern_dynamic_value(text))
    }

    /// `+` of two operands that become strings without guest code, Numbers and strings,
    /// at least one a string: `concatenate`'s result. `None` for any other pair, or a result
    /// `concatenate` refuses, which leaves the operation to `binary_slow`.
    pub(super) fn primitive_concatenation(
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
            0 => self.equal(p, left, right)?,
            1 => !self.equal(p, left, right)?,
            2 => self.strict_equal(left, right),
            3 => !self.strict_equal(left, right),
            20 => self.has_property(p, right, left)?,
            21 => self.instanceof(p, left, right)?,
            _ => return Ok(Value::number(self.numeric(p, op, left, right)?)),
        };
        Ok(if answer { Value::TRUE } else { Value::FALSE })
    }
    #[inline(always)]
    pub(super) fn binary_truthy(
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
    /// any other operator or operand.
    #[inline(always)]
    pub(super) fn number_comparison_truthy(
        &mut self,
        op: u32,
        left: Value,
        right: Value,
    ) -> Option<bool> {
        if op > LAST_COMPARISON_OPERATOR {
            return None;
        }
        if let Some((a, b)) = Value::int_pair(left, right) {
            self.record_binary_value_path(
                op,
                left,
                right,
                crate::profile::BinaryValuePath::IntegerFastPath,
            );
            return Some(number_comparison(op, a, b));
        }
        // Equality, strict equality and the relational operators agree with IEEE 754
        // comparison on numbers: NaN is unordered and unequal, and -0 equals +0.
        let (a, b) = (left.as_number()?, right.as_number()?);
        self.record_binary_value_path(
            op,
            left,
            right,
            crate::profile::BinaryValuePath::NumberFastPath,
        );
        Some(number_comparison(op, a, b))
    }

    /// BigInt::add or BigInt::subtract of one to a BigInt `input`, which runs no guest code
    /// and allocates the result; `None` for any other value.
    pub(super) fn bigint_increment(&mut self, input: Value, decrement: bool) -> Option<Value> {
        let Some(Cell::BigInt(integer)) = self.heap.get(input) else {
            return None;
        };
        let integer: &num_bigint::BigInt = integer;
        let next = if decrement { integer - 1_u8 } else { integer + 1_u8 };
        Some(self.heap.alloc(Cell::bigint(next)))
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
    pub(super) fn word_identity_equality(op: u32, left: Value, right: Value) -> Option<bool> {
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
    pub(super) fn identity_equality(&self, op: u32, left: Value, right: Value) -> Option<bool> {
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
                    self.heap.is_string(*value) || matches!(self.heap.get(*value), Some(Cell::Symbol(_)))
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
            let length = if length_number.is_nan() || length_number <= 0.0 {
                0
            } else {
                length_number.floor().min(MAX_SAFE_INTEGER) as usize
            };
            elements
                .try_reserve(length)
                .map_err(|_| self.type_error(p, "array-like list is too large".into()))?;
            for index in 0..length {
                let atom = self.intern_atom(&index.to_string());
                let element = self.get_property(p, self.heap.root_value(object).unwrap(), atom)?;
                if matches!(element_kind, ArrayLikeElementKind::PropertyKey)
                    && !self.heap.is_string(element)
                    && !matches!(self.heap.get(element), Some(Cell::Symbol(_)))
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

pub(super) fn exponentiate(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() {
        return f64::NAN;
    }
    if exponent == EXPONENTIATION_ZERO {
        return EXPONENTIATION_ONE;
    }
    if base.is_nan() || base.abs() == EXPONENTIATION_ONE && exponent.is_infinite() {
        return f64::NAN;
    }
    if base.is_infinite() {
        return infinite_power(base, exponent);
    }
    if base == EXPONENTIATION_ZERO {
        return zero_power(base, exponent);
    }
    base.powf(exponent)
}

fn infinite_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        f64::INFINITY
    } else {
        EXPONENTIATION_ZERO
    };
    signed_power_magnitude(base, exponent, magnitude)
}

fn zero_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        EXPONENTIATION_ZERO
    } else {
        f64::INFINITY
    };
    signed_power_magnitude(base, exponent, magnitude)
}

fn signed_power_magnitude(base: f64, exponent: f64, magnitude: f64) -> f64 {
    if base.is_sign_negative() && is_odd_integer(exponent) {
        -magnitude
    } else {
        magnitude
    }
}

fn is_odd_integer(value: f64) -> bool {
    value.is_finite()
        && value.fract() == EXPONENTIATION_ZERO
        && value.abs() % EXPONENTIATION_TWO == ODD_INTEGER_PARITY
}

/// Whether an equality operator is strict and whether it is negated; `None` for any other.
#[inline(always)]
pub(super) fn equality_operator(op: u32) -> Option<(bool, bool)> {
    const EQUALITY: u32 = BinaryOperator::Equality as u32;
    const INEQUALITY: u32 = BinaryOperator::Inequality as u32;
    const STRICT_EQUALITY: u32 = BinaryOperator::StrictEquality as u32;
    const STRICT_INEQUALITY: u32 = BinaryOperator::StrictInequality as u32;
    // The four operators are consecutive from an aligned first, each negation right after
    // its test and the strict pair after the loose one, so two bits of the offset decide
    // them, with no table of branches.
    const NEGATED: u32 = INEQUALITY - EQUALITY;
    const STRICT: u32 = STRICT_EQUALITY - EQUALITY;
    const _: () = assert!(
        NEGATED == 1
            && STRICT == 2
            && STRICT_INEQUALITY - EQUALITY == STRICT | NEGATED
            && EQUALITY % (STRICT << 1) == 0
    );
    let offset = op.wrapping_sub(EQUALITY);
    (offset <= STRICT | NEGATED).then_some((offset & STRICT != 0, offset & NEGATED != 0))
}
