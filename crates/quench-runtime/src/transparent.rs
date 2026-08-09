use std::collections::HashMap;

use oxc::ast::ast::ParenthesizedExpression;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce(
    expression: &ParenthesizedExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    crate::reduce::reduce_expression(&expression.expression, ops, facts, next_register, locals)
}
