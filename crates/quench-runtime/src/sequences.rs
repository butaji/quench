use std::collections::HashMap;

use oxc::ast::ast::SequenceExpression;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce(
    sequence: &SequenceExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let mut result = None;
    for expression in &sequence.expressions {
        result = Some(crate::reduce::reduce_expression(
            expression,
            ops,
            facts,
            next_register,
            locals,
        )?);
    }
    result
}
