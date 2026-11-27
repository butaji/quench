//! Enum member values: TypeScript's constant evaluation, then one of two emits.
//! An enum whose members all fold becomes one object literal with fixed keys (a
//! stable shape, so member reads are field reads); any other enum keeps
//! TypeScript's incremental emit.

use oxc_ast::ast::*;
use rustc_hash::FxHashMap;

use crate::bytecode::Constant;

#[derive(Clone)]
pub(super) enum Value {
    Number(f64),
    String(String),
}

pub(super) enum Member {
    Constant(Value),
    /// A value only the run time knows.
    Computed(Computed),
}

pub(super) enum Computed {
    /// The enum's runtime initializer at this index of its initializer list.
    Initializer(usize),
    /// The previous member's value plus one, when that member's value is not constant.
    Successor(String),
}

pub(super) fn member_name(name: &TSEnumMemberName<'_>) -> Option<String> {
    match name {
        TSEnumMemberName::Identifier(identifier) => Some(identifier.name.to_string()),
        TSEnumMemberName::String(literal) | TSEnumMemberName::ComputedString(literal) => {
            Some(literal.value.to_string())
        }
        TSEnumMemberName::ComputedTemplateString(template) => {
            template.single_quasi().map(|quasi| quasi.to_string())
        }
    }
}

/// Fold `expression` against the members declared before it (`enum_name.X`,
/// `enum_name["X"]` and bare `X` all name a member).
pub(super) fn fold(expression: &Expression<'_>, enum_name: &str, known: &FxHashMap<String, Value>) -> Option<Value> {
    match expression.without_parentheses() {
        Expression::NumericLiteral(literal) => Some(Value::Number(literal.value)),
        Expression::StringLiteral(literal) => Some(Value::String(literal.value.to_string())),
        Expression::TemplateLiteral(template) => template.single_quasi().map(|quasi| Value::String(quasi.to_string())),
        Expression::Identifier(identifier) => match identifier.name.as_str() {
            "Infinity" => Some(Value::Number(f64::INFINITY)),
            "NaN" => Some(Value::Number(f64::NAN)),
            name => known.get(name).cloned(),
        },
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) if object.name == enum_name => known.get(member.property.name.as_str()).cloned(),
            _ => None,
        },
        Expression::ComputedMemberExpression(member) => match (&member.object, &member.expression) {
            (Expression::Identifier(object), Expression::StringLiteral(key)) if object.name == enum_name => {
                known.get(key.value.as_str()).cloned()
            }
            _ => None,
        },
        Expression::UnaryExpression(unary) => unary_fold(unary, enum_name, known),
        Expression::BinaryExpression(binary) => binary_fold(binary, enum_name, known),
        _ => None,
    }
}

fn number(expression: &Expression<'_>, enum_name: &str, known: &FxHashMap<String, Value>) -> Option<f64> {
    match fold(expression, enum_name, known)? {
        Value::Number(value) => Some(value),
        Value::String(_) => None,
    }
}

fn unary_fold(unary: &UnaryExpression<'_>, enum_name: &str, known: &FxHashMap<String, Value>) -> Option<Value> {
    let value = number(&unary.argument, enum_name, known)?;
    Some(Value::Number(match unary.operator {
        UnaryOperator::UnaryPlus => value,
        UnaryOperator::UnaryNegation => -value,
        UnaryOperator::BitwiseNot => f64::from(!crate::value::number_to_i32(value)),
        _ => return None,
    }))
}

fn binary_fold(binary: &BinaryExpression<'_>, enum_name: &str, known: &FxHashMap<String, Value>) -> Option<Value> {
    let left = fold(&binary.left, enum_name, known)?;
    let right = fold(&binary.right, enum_name, known)?;
    if binary.operator == BinaryOperator::Addition
        && let (Value::String(_), _) | (_, Value::String(_)) = (&left, &right)
    {
        return Some(Value::String(format!("{}{}", display(&left)?, display(&right)?)));
    }
    let (Value::Number(a), Value::Number(b)) = (left, right) else { return None };
    // The interpreter's own Number operators (`static_number_binary`): a member's value is the value the runtime computes.
    match crate::vm::operations::static_number_binary(binary.operator as u32, a, b)? {
        Constant::Number(value) => Some(Value::Number(value)),
        _ => None,
    }
}

/// The JavaScript text of a folded value, when it is exact without runtime help
/// (safe integers; other numbers keep TypeScript's runtime emit).
pub(super) fn display(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) if !(*number == 0.0 && number.is_sign_negative()) =>
            crate::vm::operations::safe_integer(*number).map(|integer| integer.to_string()),
        Value::Number(_) => None,
        Value::String(text) => Some(text.clone()),
    }
}

pub(super) fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            character if (character as u32) < 0x20 || matches!(character, '\u{2028}' | '\u{2029}') => {
                quoted.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}
