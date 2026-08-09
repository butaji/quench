use std::collections::HashMap;

use oxc::ast::ast::ArrayExpression;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce(
    array: &ArrayExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let mut elements = Vec::new();
    for element in &array.elements {
        elements.push(crate::reduce::reduce_expression(
            element.as_expression()?,
            ops,
            facts,
            next_register,
            locals,
        )?);
    }
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeArray {
        dst: register,
        elements,
    });
    Some(register)
}
