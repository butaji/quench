//! Ershov's division at parse: an expression whose operands are all known before run time is
//! static, and its value is computed here, once, by the interpreter's own operator semantics
//! (`vm::operations::static_number_binary`). Every tier then reads the residual constant, so
//! the work is done at the earliest stage and no tier repeats it.

use oxc_ast::ast::{BinaryExpression, BinaryOperator, Expression, UnaryExpression};
use oxc_syntax::operator::UnaryOperator;

use crate::bytecode::Constant;

/// How deep a static operator tree is folded in one call. A long left-deep chain whose
/// leftmost operand is dynamic would otherwise be descended once per level as the compiler
/// walks it; past this depth the lower levels fold when the compiler reaches them.
const FOLD_DEPTH: u32 = 64;

pub(super) fn expression(value: &Expression<'_>) -> Option<Constant> {
    folded(value, FOLD_DEPTH)
}

fn folded(value: &Expression<'_>, depth: u32) -> Option<Constant> {
    match value {
        Expression::NumericLiteral(value) => Some(Constant::Number(value.value)),
        Expression::StringLiteral(value) => Some(super::string::constant(value)),
        Expression::BigIntLiteral(value) => {
            Some(Constant::BigInt(crate::bigint::decimal_literal(&value.value)))
        }
        Expression::BooleanLiteral(value) => Some(Constant::Boolean(value.value)),
        Expression::NullLiteral(_) => Some(Constant::Null),
        Expression::UnaryExpression(value) => unary_at(value, depth),
        Expression::BinaryExpression(value) => binary_at(value, depth.checked_sub(1)?),
        value => folded(transparent(value)?, depth),
    }
}

/// The operand of an expression that evaluates to its operand: parentheses and TypeScript's
/// type-only wrappers.
fn transparent<'a>(value: &'a Expression<'a>) -> Option<&'a Expression<'a>> {
    Some(match value {
        Expression::ParenthesizedExpression(value) => &value.expression,
        Expression::TSAsExpression(value) => &value.expression,
        Expression::TSSatisfiesExpression(value) => &value.expression,
        Expression::TSTypeAssertion(value) => &value.expression,
        Expression::TSNonNullExpression(value) => &value.expression,
        Expression::TSInstantiationExpression(value) => &value.expression,
        _ => return None,
    })
}

/// A unary operator applied to a static operand, where the result needs no run-time
/// coercion: `void` of any literal (which minifiers write for `undefined`), `!` of a
/// literal whose ToBoolean is fixed (`!0` and `!1` for the booleans), and `-`/`+` of a
/// number.
pub(super) fn unary(value: &UnaryExpression<'_>) -> Option<Constant> {
    unary_at(value, FOLD_DEPTH)
}

fn unary_at(value: &UnaryExpression<'_>, depth: u32) -> Option<Constant> {
    let operand = folded(&value.argument, depth)?;
    let folded = match (value.operator, operand) {
        (UnaryOperator::Void, _) => Constant::Undefined,
        (UnaryOperator::LogicalNot, operand) => Constant::Boolean(!to_boolean(&operand)?),
        (UnaryOperator::UnaryNegation, Constant::Number(value)) => Constant::Number(-value),
        (UnaryOperator::UnaryPlus, Constant::Number(value)) => Constant::Number(value),
        _ => return None,
    };
    Some(folded)
}

/// A binary operator applied to static operands whose result runs no coercion: any numeric
/// or comparison operator on two Numbers, and concatenation or equality of two strings.
pub(super) fn binary(value: &BinaryExpression<'_>) -> Option<Constant> {
    binary_at(value, FOLD_DEPTH)
}

fn binary_at(value: &BinaryExpression<'_>, depth: u32) -> Option<Constant> {
    // The right operand is usually a leaf: a dynamic one ends the fold before the left
    // subtree is walked.
    let right = folded(&value.right, depth)?;
    let left = folded(&value.left, depth)?;
    match (left, right) {
        (Constant::Number(left), Constant::Number(right)) => {
            crate::vm::operations::static_number_binary(value.operator as u32, left, right)
        }
        (Constant::String(left), Constant::String(right)) => strings(value.operator, &left, &right),
        _ => None,
    }
}

fn strings(operator: BinaryOperator, left: &str, right: &str) -> Option<Constant> {
    Some(match operator {
        BinaryOperator::Addition => Constant::String([left, right].concat().into()),
        BinaryOperator::Equality | BinaryOperator::StrictEquality => Constant::Boolean(left == right),
        BinaryOperator::Inequality | BinaryOperator::StrictInequality => Constant::Boolean(left != right),
        _ => return None,
    })
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
