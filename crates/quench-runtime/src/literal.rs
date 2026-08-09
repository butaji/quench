use oxc::{ast::ast::Expression, syntax::operator::BinaryOperator};

use crate::{facts::Constant as FactConstant, ops::Constant};

pub(crate) struct Literal {
    pub(crate) fact: FactConstant,
    pub(crate) op: Constant,
}

pub(crate) fn reduce_literal(expression: &Expression<'_>) -> Option<Literal> {
    match expression {
        Expression::NumericLiteral(number) => Some(Literal {
            fact: FactConstant::Number(number.value),
            op: Constant::Number(number.value),
        }),
        Expression::BooleanLiteral(boolean) => Some(Literal {
            fact: FactConstant::Boolean(boolean.value),
            op: Constant::Boolean(boolean.value),
        }),
        Expression::StringLiteral(string) => Some(Literal {
            fact: FactConstant::String(string.value.to_string()),
            op: Constant::String(string.value.to_string()),
        }),
        Expression::NullLiteral(_) => Some(Literal {
            fact: FactConstant::Null,
            op: Constant::Null,
        }),
        _ => None,
    }
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
        _ => return None,
    })
}
