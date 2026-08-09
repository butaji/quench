use crate::{execute::VmError, ops::Op, value::Value};
use std::collections::HashMap;

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<Value, VmError> {
    let Op::Branch {
        condition,
        then_ops,
        else_ops,
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let value = crate::execute::read_register(registers, *condition)?;
    let selected = if crate::execute::is_truthy(&value) {
        then_ops
    } else {
        else_ops
    };
    crate::execute::execute_in_place(selected, registers)
}

pub(crate) fn execute_or_continue(
    registers: &mut Vec<Value>,
    op: &Op,
) -> Result<Option<Value>, VmError> {
    match execute(registers, op) {
        Ok(value) => Ok(Some(value)),
        Err(VmError::MissingReturn) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn reduce(
    statement: &oxc::ast::ast::Statement<'_>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<Vec<Op>, Vec<String>> {
    match statement {
        oxc::ast::ast::Statement::BlockStatement(block) => {
            let next_slot = crate::reduce_support::register_base(locals);
            crate::reduce::reduce_statements_no_tail(&block.body, facts, locals.clone(), next_slot)
        }
        statement => {
            let mut ops = Vec::new();
            let mut next_register = crate::reduce_support::register_base(locals);
            let mut next_slot = crate::reduce_support::register_base(locals);
            let mut locals = locals.clone();
            crate::reduce::reduce_statement(
                statement,
                &mut ops,
                facts,
                &mut next_register,
                &mut next_slot,
                &mut locals,
            )?;
            Ok(ops)
        }
    }
}
