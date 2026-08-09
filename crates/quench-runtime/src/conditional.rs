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
    if let Expression::BooleanLiteral(test) = &conditional.test {
        let branch = if test.value {
            &conditional.consequent
        } else {
            &conditional.alternate
        };
        return crate::reduce::reduce_expression(branch, ops, facts, next_register, locals);
    }
    let condition =
        crate::reduce::reduce_expression(&conditional.test, ops, facts, next_register, locals)?;
    let consequent = reduce_branch(&conditional.consequent, facts, next_register, locals)?;
    let alternate = reduce_branch(&conditional.alternate, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(crate::ops::Op::Conditional {
        dst,
        condition,
        consequent,
        alternate,
    });
    Some(dst)
}

fn reduce_branch(
    expression: &Expression<'_>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<Vec<crate::ops::Op>> {
    let mut ops = Vec::new();
    let value =
        crate::reduce::reduce_expression(expression, &mut ops, facts, next_register, locals)?;
    ops.push(crate::ops::Op::Return { src: value });
    Some(ops)
}

pub(crate) fn execute(
    registers: &mut Vec<crate::value::Value>,
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    let crate::ops::Op::Conditional {
        dst,
        condition,
        consequent,
        alternate,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let test = crate::execute::read_register(registers, *condition)?;
    let branch = if crate::execute::is_truthy(&test) {
        consequent
    } else {
        alternate
    };
    let value = crate::execute::execute_in_place(branch, registers)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}
