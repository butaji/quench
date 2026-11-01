use oxc_ast::ast::{Expression, UnaryExpression};
use oxc_syntax::operator::UnaryOperator;

use crate::bytecode::Constant;

pub(super) fn expression(value: &Expression<'_>) -> Option<Constant> {
    match value {
        Expression::NumericLiteral(value) => Some(Constant::Number(value.value)),
        Expression::StringLiteral(value) => Some(super::string::constant(value)),
        Expression::BigIntLiteral(value) => {
            Some(Constant::BigInt(crate::bigint::decimal_literal(&value.value)))
        }
        Expression::BooleanLiteral(value) => Some(Constant::Boolean(value.value)),
        Expression::NullLiteral(_) => Some(Constant::Null),
        Expression::ParenthesizedExpression(value) => expression(&value.expression),
        Expression::TSAsExpression(value) => expression(&value.expression),
        Expression::TSSatisfiesExpression(value) => expression(&value.expression),
        Expression::TSTypeAssertion(value) => expression(&value.expression),
        Expression::TSNonNullExpression(value) => expression(&value.expression),
        Expression::TSInstantiationExpression(value) => expression(&value.expression),
        Expression::UnaryExpression(value) => unary(value),
        _ => None,
    }
}

/// A unary operator applied to a static operand, where the result needs no run-time
/// coercion: `void` of any literal (which minifiers write for `undefined`), `!` of a
/// literal whose ToBoolean is fixed (`!0` and `!1` for the booleans), and `-`/`+` of a
/// number.
pub(super) fn unary(value: &UnaryExpression<'_>) -> Option<Constant> {
    let operand = expression(&value.argument)?;
    let folded = match (value.operator, operand) {
        (UnaryOperator::Void, _) => Constant::Undefined,
        (UnaryOperator::LogicalNot, operand) => match to_boolean(&operand) {
            Some(truthy) => Constant::Boolean(!truthy),
            None => return None,
        },
        (UnaryOperator::UnaryNegation, Constant::Number(value)) => Constant::Number(-value),
        (UnaryOperator::UnaryPlus, Constant::Number(value)) => Constant::Number(value),
        _ => return None,
    };
    Some(folded)
}

/// ToBoolean of a static constant; BigInt literals keep their run-time conversion.
fn to_boolean(value: &Constant) -> Option<bool> {
    match value {
        Constant::Number(value) => Some(*value != 0.0 && !value.is_nan()),
        Constant::String(value) => Some(!value.is_empty()),
        Constant::StringUnits(value) => Some(!value.is_empty()),
        Constant::Boolean(value) => Some(*value),
        Constant::Null | Constant::Undefined => Some(false),
        _ => None,
    }
}

/// Whether a test is a literal whose ToBoolean is true, so a loop need not evaluate it.
pub(super) fn always_truthy(value: &Expression<'_>) -> bool {
    expression(value)
        .and_then(|value| to_boolean(&value))
        .unwrap_or(false)
}
