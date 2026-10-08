use oxc_ast::ast::Expression;

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
            BindingTime::Static(Constant::BigInt(value.value.to_string()))
        }
        Expression::BooleanLiteral(value) => BindingTime::Static(Constant::Boolean(value.value)),
        Expression::NullLiteral(_) => BindingTime::Static(Constant::Null),
        _ => BindingTime::Dynamic,
    }
}
