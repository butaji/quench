use oxc_ast::ast::{Expression, UnaryExpression};
use oxc_syntax::operator::UnaryOperator;

use crate::bytecode::Constant;

#[derive(Clone, Copy)]
pub(super) enum BindingTime<T> {
    Static(T),
    Dynamic,
}

impl<T> BindingTime<T> {
    pub(super) fn static_value(self) -> Option<T> {
        match self {
            Self::Static(value) => Some(value),
            Self::Dynamic => None,
        }
    }
}

pub(super) fn expression(value: &Expression<'_>) -> BindingTime<Constant> {
    match value {
        Expression::NumericLiteral(value) => BindingTime::Static(Constant::Number(value.value)),
        Expression::StringLiteral(value) => BindingTime::Static(super::string::constant(value)),
        Expression::BigIntLiteral(value) => {
            BindingTime::Static(Constant::BigInt(crate::bigint::decimal_literal(&value.value)))
        }
        Expression::BooleanLiteral(value) => BindingTime::Static(Constant::Boolean(value.value)),
        Expression::NullLiteral(_) => BindingTime::Static(Constant::Null),
        Expression::ParenthesizedExpression(value) => expression(&value.expression),
        Expression::UnaryExpression(value) => unary(value),
        _ => BindingTime::Dynamic,
    }
}

/// A unary operator applied to a static operand, where the result needs no run-time
/// coercion: `void` of any literal (which minifiers write for `undefined`), `!` of a
/// literal whose ToBoolean is fixed (`!0` and `!1` for the booleans), and `-`/`+` of a
/// number.
pub(super) fn unary(value: &UnaryExpression<'_>) -> BindingTime<Constant> {
    let Some(operand) = expression(&value.argument).static_value() else {
        return BindingTime::Dynamic;
    };
    let folded = match (value.operator, operand) {
        (UnaryOperator::Void, _) => Constant::Undefined,
        (UnaryOperator::LogicalNot, operand) => match to_boolean(&operand) {
            Some(truthy) => Constant::Boolean(!truthy),
            None => return BindingTime::Dynamic,
        },
        (UnaryOperator::UnaryNegation, Constant::Number(value)) => Constant::Number(-value),
        (UnaryOperator::UnaryPlus, Constant::Number(value)) => Constant::Number(value),
        _ => return BindingTime::Dynamic,
    };
    BindingTime::Static(folded)
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
        .static_value()
        .and_then(|value| to_boolean(&value))
        .unwrap_or(false)
}
