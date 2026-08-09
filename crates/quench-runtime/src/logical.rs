use std::collections::HashMap;

use oxc::syntax::operator::LogicalOperator;

use crate::{facts::ProgramDb, literal::reduce_literal, ops::Constant, ops::Op};

pub(crate) fn reduce_expression(
    logical: &oxc::ast::ast::LogicalExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let literal = reduce_literal(&logical.left)?;
    let left = crate::reduce::reduce_expression(&logical.left, ops, facts, next_register, locals)?;
    let use_left = match logical.operator {
        LogicalOperator::Or => is_truthy(&literal.op),
        LogicalOperator::And => !is_truthy(&literal.op),
        LogicalOperator::Coalesce => !matches!(literal.op, Constant::Null),
    };
    if use_left {
        Some(left)
    } else {
        crate::reduce::reduce_expression(&logical.right, ops, facts, next_register, locals)
    }
}

fn is_truthy(value: &Constant) -> bool {
    match value {
        Constant::Boolean(value) => *value,
        Constant::Number(value) => *value != 0.0 && !value.is_nan(),
        Constant::String(value) => !value.is_empty(),
        Constant::Null | Constant::Undefined => false,
    }
}
