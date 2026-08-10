use std::collections::HashMap;

use oxc::ast::ast::{ReturnStatement, ThrowStatement};

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce_return(
    statement: &ReturnStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let register = statement
        .argument
        .as_ref()
        .and_then(|expression| {
            crate::reduce::reduce_expression(expression, ops, facts, next_register, locals)
        })
        .or_else(|| Some(crate::reduce_support::emit_undefined(ops, next_register)));
    if let Some(register) = register {
        ops.push(Op::Return { src: register });
    }
    Ok(None)
}

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
    let body =
        crate::reduce::reduce_statements_no_tail(&statement.block.body, facts, locals.clone(), 0)?;
    let handler = statement
        .handler
        .as_ref()
        .map(|handler| {
            let (handler_locals, catch_slot) = handler_locals(handler, locals);
            crate::reduce::reduce_statements_no_tail(&handler.body.body, facts, handler_locals, 0)
                .map(|ops| (ops, catch_slot))
        })
        .transpose()?;
    let (handler, catch_slot) = handler.map_or((None, None), |(ops, slot)| (Some(ops), slot));
    let finalizer = statement
        .finalizer
        .as_ref()
        .map(|finalizer| {
            crate::reduce::reduce_statements_no_tail(&finalizer.body, facts, locals.clone(), 0)
        })
        .transpose()?;
    ops.push(Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
    });
    Ok(())
}

fn handler_locals(
    handler: &oxc::ast::ast::CatchClause<'_>,
    locals: &HashMap<String, u16>,
) -> (HashMap<String, u16>, Option<u16>) {
    let mut result = locals.clone();
    let Some(parameter) = handler.param.as_ref() else {
        return (result, None);
    };
    let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) = &parameter.pattern.kind
    else {
        return (result, None);
    };
    let slot = result
        .values()
        .copied()
        .max()
        .map_or(0, |value| value.saturating_add(1));
    result.insert(identifier.name.to_string(), slot);
    (result, Some(slot))
}

pub(crate) fn reduce_try_statement(
    statement: &oxc::ast::ast::TryStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    reduce_try(statement, ops, facts, locals).map(|_| None)
}
