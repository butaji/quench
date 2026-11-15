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
fn unsigned_shift_result_range_uses_effective_ecmascript_shift_count() {
    for (right, fits) in [(0.0, false), (-0.0, false), (1.0, true), (17.9, true),
        (31.0, true), (32.0, false), (33.0, true), (-1.0, true), (-32.0, false),
        (4_294_967_297.0, true), (f64::NAN, false), (f64::INFINITY, false),
        (f64::NEG_INFINITY, false)] {
        let operation = NumberBitwise::ShiftRightUnsigned;
        assert_eq!(operation.result_is_int32(Some(right)), fits, "{right}");
        let result = super::number_bitwise_f64_result(operation, -1, crate::value::number_to_u32(right) as i32);
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
        assert_eq!(number_is_truthy(value), expected);
    }
}
