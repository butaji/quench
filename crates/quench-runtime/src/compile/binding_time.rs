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

/// The static value an identifier names, if any: a `const` binding the compiler folded.
pub(super) type Names<'n> = &'n mut dyn FnMut(&str) -> Option<Constant>;

pub(super) fn expression(value: &Expression<'_>) -> Option<Constant> {
    folded(value, FOLD_DEPTH, &mut |_| None)
}

/// `expression`, reading identifiers through `names`.
pub(super) fn expression_in(value: &Expression<'_>, names: Names<'_>) -> Option<Constant> {
    folded(value, FOLD_DEPTH, names)
}

fn folded(value: &Expression<'_>, depth: u32, names: Names<'_>) -> Option<Constant> {
    match value {
        Expression::Identifier(value) => names(value.name.as_str()),
        Expression::NumericLiteral(value) => Some(Constant::Number(value.value)),
        Expression::StringLiteral(value) => Some(super::string::constant(value)),
        Expression::BigIntLiteral(value) => {
            Some(Constant::BigInt(crate::bigint::decimal_literal(&value.value)))
        }
        Expression::BooleanLiteral(value) => Some(Constant::Boolean(value.value)),
        Expression::NullLiteral(_) => Some(Constant::Null),
        // A template without substitutions is its one cooked text, as a string literal is.
        Expression::TemplateLiteral(value) if value.expressions.is_empty() => {
            let [text] = value.quasis.as_slice() else { return None };
            text.value.cooked.as_ref()?;
            Some(super::string::template_constant(&text.value))
        }
        Expression::UnaryExpression(value) => unary_at(value, depth, names),
        Expression::BinaryExpression(value) => binary_at(value, depth.checked_sub(1)?, names),
        value => folded(transparent(value)?, depth, names),
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
    unary_at(value, FOLD_DEPTH, &mut |_| None)
}

fn unary_at(value: &UnaryExpression<'_>, depth: u32, names: Names<'_>) -> Option<Constant> {
    let operand = folded(&value.argument, depth, names)?;
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
pub(super) fn binary_in(value: &BinaryExpression<'_>, names: Names<'_>) -> Option<Constant> {
    binary_at(value, FOLD_DEPTH, names)
}

fn binary_at(value: &BinaryExpression<'_>, depth: u32, names: Names<'_>) -> Option<Constant> {
    // The right operand is usually a leaf: a dynamic one ends the fold before the left
    // subtree is walked.
    let right = folded(&value.right, depth, names)?;
    let left = folded(&value.left, depth, names)?;
    match (left, right) {
        (Constant::Number(left), Constant::Number(right)) => {
            crate::vm::operations::static_number_binary(value.operator as u32, left, right)
        }
        (Constant::String(left), Constant::String(right)) => strings(value.operator, &left, &right),
        (left, right) => immediates(value.operator, &left, &right),
    }
}

/// A binary operator on static primitives other than two Numbers or two strings, which run
/// no guest code: equality by the language's rules over the kinds it compares without
/// coercing a string, and every other operator but `in` and `instanceof` (which throw on a
/// primitive) as the Numbers its operands convert to (`immediate_number`).
fn immediates(operator: BinaryOperator, left: &Constant, right: &Constant) -> Option<Constant> {
    let equal = |equal: Option<bool>, negated: bool| Some(Constant::Boolean(equal? != negated));
    match operator {
        BinaryOperator::StrictEquality => equal(strict_equal(left, right), false),
        BinaryOperator::StrictInequality => equal(strict_equal(left, right), true),
        BinaryOperator::Equality => equal(loose_equal(left, right), false),
        BinaryOperator::Inequality => equal(loose_equal(left, right), true),
        BinaryOperator::In | BinaryOperator::Instanceof => None,
        _ => crate::vm::operations::static_number_binary(operator as u32, immediate_number(left)?, immediate_number(right)?),
    }
}

/// The Number a Number or an immediate primitive converts to (ToNumber); strings and
/// BigInts keep their run-time conversion.
fn immediate_number(value: &Constant) -> Option<f64> {
    match value {
        Constant::Number(value) => Some(*value),
        Constant::Boolean(value) => Some(f64::from(u8::from(*value))),
        Constant::Null => Some(0.0),
        Constant::Undefined => Some(f64::NAN),
        _ => None,
    }
}

/// Whether `value` is a kind whose equality needs no conversion table beyond `immediates`.
fn simple(value: &Constant) -> bool {
    matches!(value, Constant::Number(_) | Constant::String(_) | Constant::Boolean(_) | Constant::Null | Constant::Undefined)
}

/// IsStrictlyEqual of two simple constants: equal kinds compare their values, others differ.
fn strict_equal(left: &Constant, right: &Constant) -> Option<bool> {
    match (left, right) {
        (Constant::Number(left), Constant::Number(right)) => Some(left == right),
        (Constant::String(left), Constant::String(right)) => Some(left == right),
        (Constant::Boolean(left), Constant::Boolean(right)) => Some(left == right),
        (Constant::Null, Constant::Null) | (Constant::Undefined, Constant::Undefined) => Some(true),
        _ => (simple(left) && simple(right)).then_some(false),
    }
}

/// IsLooselyEqual where no string is coerced: `null` and `undefined` equal only each other,
/// and Numbers and Booleans compare as Numbers.
fn loose_equal(left: &Constant, right: &Constant) -> Option<bool> {
    let nullish = |value: &Constant| matches!(value, Constant::Null | Constant::Undefined);
    match (nullish(left), nullish(right)) {
        (true, true) => Some(true),
        (false, false) if !matches!(left, Constant::String(_)) && !matches!(right, Constant::String(_)) => {
            Some(immediate_number(left)? == immediate_number(right)?)
        }
        (false, false) => None,
        _ => (simple(left) && simple(right)).then_some(false),
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
pub(super) fn to_boolean(value: &Constant) -> Option<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(source: &str) -> Option<Constant> {
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::script()).parse();
        let oxc_ast::ast::Statement::ExpressionStatement(statement) = &parsed.program.body[0] else { panic!("{source}") };
        expression(&statement.expression)
    }

    #[test]
    fn binary_operators_fold_over_static_primitives() {
        assert!(matches!(fold("1 + 2"), Some(Constant::Number(value)) if value == 3.0));
        assert!(matches!(fold("true + 1"), Some(Constant::Number(value)) if value == 2.0));
        assert!(matches!(fold("undefined == null"), None), "`undefined` is a name, not a literal");
        assert!(matches!(fold("null == void 0"), Some(Constant::Boolean(true))));
        assert!(matches!(fold("null === void 0"), Some(Constant::Boolean(false))));
        assert!(matches!(fold("1 === 'a'"), Some(Constant::Boolean(false))));
        assert!(matches!(fold("true == 1"), Some(Constant::Boolean(true))));
        assert!(matches!(fold("null == 0"), Some(Constant::Boolean(false))));
        assert!(matches!(fold("null < 1"), Some(Constant::Boolean(true))));
        assert!(matches!(fold("void 0 < 1"), Some(Constant::Boolean(false))), "NaN compares false");
    }

    #[test]
    fn coercing_operators_are_left_to_run_time() {
        assert!(fold("1 == '1'").is_none(), "a string operand converts at run time");
        assert!(fold("1 in 2").is_none());
        assert!(fold("1 instanceof 2").is_none());
        assert!(fold("'a' - 1").is_none());
        assert!(fold("1n + 1").is_none());
    }

    #[test]
    fn truthiness_of_static_tests_is_decided_at_compile_time() {
        let truthy = |source| fold(source).and_then(|value| to_boolean(&value));
        assert_eq!(truthy("1 < 2"), Some(true));
        assert_eq!(truthy("1 === 2"), Some(false));
        assert_eq!(truthy("null == void 0"), Some(true));
        assert_eq!(truthy("'' + ''"), Some(false));
        assert_eq!(truthy("x < 2"), None);
    }
}
