use std::collections::HashMap;

use oxc::ast::ast::Statement;

use crate::{facts::ProgramDb, ops::Op};

mod helpers;

type ReduceResult = Result<Option<u16>, Vec<String>>;

pub(crate) fn reduce(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> ReduceResult {
    if let Statement::WithStatement(with_statement) = statement {
        return reduce_with(with_statement, ops, facts, next_register, next_slot, locals);
    }
    if let Some(result) = helpers::reduce_labeled_or_conditional(
        statement,
        ops,
        facts,
        next_register,
        next_slot,
        locals,
    ) {
        return result;
    }
    if let Some(result) =
        helpers::reduce_loop_statement(statement, ops, facts, next_register, next_slot, locals)
    {
        return result;
    }
    if let Some(result) =
        helpers::reduce_control_statement(statement, ops, facts, next_register, locals)
    {
        return result;
    }
    helpers::unsupported_statement(statement)
}

fn reduce_with(
    statement: &oxc::ast::ast::WithStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> ReduceResult {
    let object =
        crate::reduce::reduce_expression(&statement.object, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported with object".to_string()])?;
    let mut body = Vec::new();
    crate::reduce::reduce_statement(
        &statement.body,
        &mut body,
        facts,
        next_register,
        next_slot,
        locals,
    )?;
    ops.push(Op::With { object, body });
    Ok(None)
}

pub(crate) fn execute_label(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let Op::Label { name, body } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let completion = crate::execute::execute_completion_in_place(body, registers)?;
    if matches!(&completion, crate::completion::Completion::Break(Some(label)) if label == name) {
        Ok(crate::completion::Completion::Normal)
    } else {
        Ok(completion)
    }
}
