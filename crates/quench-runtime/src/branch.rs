use crate::{execute::VmError, ops::Op, value::Value};
use std::collections::HashMap;

pub(crate) fn execute(registers: &[Value], op: &Op) -> Result<(), VmError> {
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
    crate::execute::execute_with_registers(selected, registers.to_vec()).map(|_| ())
}

pub(crate) fn execute_special(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    match op {
        Op::CallMethod { .. } => crate::methods::execute(registers, op),
        Op::Construct { .. } => crate::construct::execute(registers, op),
        Op::Branch { .. } => execute(registers, op),
        Op::Try { .. } => crate::exceptions::execute(registers, op),
        Op::Loop { .. } => crate::loops::execute(registers, op),
        Op::Switch { .. } => crate::switch::execute(registers, op),
        _ => Err(VmError::MissingReturn),
    }
}

pub(crate) fn reduce(
    statement: &oxc::ast::ast::Statement<'_>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<Vec<Op>, Vec<String>> {
    match statement {
        oxc::ast::ast::Statement::BlockStatement(block) => {
            crate::reduce::reduce_statements_with_locals(&block.body, facts, locals.clone(), 0)
        }
        statement => {
            let mut ops = Vec::new();
            let mut next_register = 0;
            let mut next_slot = 0;
            let mut locals = locals.clone();
            let last = crate::reduce::reduce_statement(
                statement,
                &mut ops,
                facts,
                &mut next_register,
                &mut next_slot,
                &mut locals,
            )?;
            crate::reduce::finish_program(ops, last)
        }
    }
}
