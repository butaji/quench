use std::collections::HashMap;

use oxc::ast::ast::ThrowStatement;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce_throw(
    statement: &ThrowStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let Some(src) =
        crate::reduce::reduce_expression(&statement.argument, ops, facts, next_register, locals)
    else {
        return Err(vec!["Unsupported throw expression".to_string()]);
    };
    ops.push(Op::Throw { src });
    Ok(None)
}

pub(crate) fn reduce_try(
    statement: &oxc::ast::ast::TryStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let body = crate::reduce::reduce_statements_with_locals(
        &statement.block.body,
        facts,
        locals.clone(),
        0,
    )?;
    let handler = statement
        .handler
        .as_ref()
        .map(|handler| {
            crate::reduce::reduce_statements_with_locals(
                &handler.body.body,
                facts,
                locals.clone(),
                0,
            )
        })
        .transpose()?;
    let finalizer = statement
        .finalizer
        .as_ref()
        .map(|finalizer| {
            crate::reduce::reduce_statements_with_locals(&finalizer.body, facts, locals.clone(), 0)
        })
        .transpose()?;
    ops.push(Op::Try {
        body,
        handler,
        finalizer,
    });
    Ok(())
}

pub(crate) fn reduce_try_statement(
    statement: &oxc::ast::ast::TryStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_try(statement, ops, facts, locals).map(|_| None)
}
