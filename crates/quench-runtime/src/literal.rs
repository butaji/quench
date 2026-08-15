use oxc::{ast::ast::Expression, span::Span, syntax::operator::BinaryOperator};

use crate::{facts::Constant as FactConstant, ops::Constant};

pub(crate) struct Literal {
    pub(crate) span: Span,
    pub(crate) fact: FactConstant,
    pub(crate) op: Constant,
}

pub(crate) fn reduce_literal(expression: &Expression<'_>) -> Option<Literal> {
    match expression {
        Expression::NumericLiteral(number) => Some(Literal {
            span: number.span,
            fact: FactConstant::Number(number.value),
            op: Constant::Number(number.value),
        }),
        Expression::BooleanLiteral(boolean) => Some(Literal {
            span: boolean.span,
            fact: FactConstant::Boolean(boolean.value),
            op: Constant::Boolean(boolean.value),
        }),
        Expression::StringLiteral(string) => string_literal(string.span, string.value.as_str()),
        Expression::NullLiteral(null) => Some(Literal {
            span: null.span,
            fact: FactConstant::Null,
            op: Constant::Null,
        }),
        Expression::BigIntLiteral(bigint) => {
            let value = bigint_value(bigint)?;
            Some(Literal {
                span: bigint.span,
                fact: FactConstant::BigInt(value.clone()),
                op: Constant::BigInt(value),
            })
        }
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => {
            reduce_template_literal(template)
        }
        _ => None,
    }
}

fn string_literal(span: Span, value: &str) -> Option<Literal> {
    let op = unicode_escape_constant(value).unwrap_or_else(|| Constant::String(value.to_string()));
    Some(Literal {
        span,
        fact: FactConstant::String(value.to_string()),
        op,
    })
}

fn unicode_escape_constant(value: &str) -> Option<Constant> {
    if !value.contains("\\u") {
        return None;
    }
    let mut units = Vec::new();
    let mut chars = value.chars().peekable();
    let mut changed = false;
    while let Some(character) = chars.next() {
        if character == '\\' && chars.peek() == Some(&'u') {
            chars.next();
            let digits: String = chars.by_ref().take(4).collect();
            if let Ok(code) = u16::from_str_radix(&digits, 16) {
                if (0xd800..=0xdfff).contains(&code) && units.last() == Some(&('\\' as u16)) {
                    units.pop();
                }
                units.push(code);
                changed = true;
                continue;
            }
            units.extend("\\u".encode_utf16());
            units.extend(digits.encode_utf16());
        } else {
            units.extend(character.to_string().encode_utf16());
        }
    }
    if !changed {
        return None;
    }
    Some(
        String::from_utf16(&units)
            .map(Constant::String)
            .unwrap_or(Constant::StringUnits(units)),
    )
}

fn reduce_template_literal(template: &oxc::ast::ast::TemplateLiteral<'_>) -> Option<Literal> {
    let value = template.quasis.first()?.value.cooked.as_ref()?.to_string();
    Some(Literal {
        span: template.span,
        fact: FactConstant::String(value.clone()),
        op: Constant::String(value),
    })
}

/// Decimal string of a BigInt literal, normalizing all supported radices.
pub(crate) fn bigint_value(bigint: &oxc::ast::ast::BigIntLiteral<'_>) -> Option<String> {
    let raw = bigint.raw.as_str().trim_end_matches('n').replace('_', "");
    let (radix, digits) = match raw.as_bytes() {
        [b'0', b'x' | b'X', digits @ ..] => (16, digits),
        [b'0', b'b' | b'B', digits @ ..] => (2, digits),
        [b'0', b'o' | b'O', digits @ ..] => (8, digits),
        _ => (10, raw.as_bytes()),
    };
    num_bigint::BigUint::parse_bytes(digits, radix).map(|value| value.to_str_radix(10))
}

pub(crate) fn reduce_operator(operator: BinaryOperator) -> Option<crate::ops::BinaryOp> {
    Some(match operator {
        BinaryOperator::Addition => crate::ops::BinaryOp::Add,
        BinaryOperator::Subtraction => crate::ops::BinaryOp::Subtract,
        BinaryOperator::Multiplication => crate::ops::BinaryOp::Multiply,
        BinaryOperator::Division => crate::ops::BinaryOp::Divide,
        BinaryOperator::Remainder => crate::ops::BinaryOp::Remainder,
        BinaryOperator::Exponential => crate::ops::BinaryOp::Exponentiate,
        BinaryOperator::Equality => crate::ops::BinaryOp::Equal,
        BinaryOperator::Inequality => crate::ops::BinaryOp::NotEqual,
        BinaryOperator::StrictEquality => crate::ops::BinaryOp::StrictEqual,
        BinaryOperator::StrictInequality => crate::ops::BinaryOp::StrictNotEqual,
        BinaryOperator::LessThan => crate::ops::BinaryOp::LessThan,
        BinaryOperator::LessEqualThan => crate::ops::BinaryOp::LessEqual,
        BinaryOperator::GreaterThan => crate::ops::BinaryOp::GreaterThan,
        BinaryOperator::GreaterEqualThan => crate::ops::BinaryOp::GreaterEqual,
        BinaryOperator::BitwiseOR => crate::ops::BinaryOp::BitwiseOr,
        BinaryOperator::BitwiseXOR => crate::ops::BinaryOp::BitwiseXor,
        BinaryOperator::BitwiseAnd => crate::ops::BinaryOp::BitwiseAnd,
        BinaryOperator::ShiftLeft => crate::ops::BinaryOp::ShiftLeft,
        BinaryOperator::ShiftRight => crate::ops::BinaryOp::ShiftRight,
        BinaryOperator::ShiftRightZeroFill => crate::ops::BinaryOp::ShiftRightZeroFill,
        BinaryOperator::Instanceof => crate::ops::BinaryOp::Instanceof,
        _ => return None,
    })
}
