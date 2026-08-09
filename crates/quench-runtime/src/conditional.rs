use std::collections::HashMap;

use oxc::ast::ast::Expression;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce_expression(
    conditional: &oxc::ast::ast::ConditionalExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let Expression::BooleanLiteral(test) = &conditional.test else {
        return None;
    };
    let branch = if test.value {
        &conditional.consequent
    } else {
        &conditional.alternate
    };
    crate::reduce::reduce_expression(branch, ops, facts, next_register, locals)
}
