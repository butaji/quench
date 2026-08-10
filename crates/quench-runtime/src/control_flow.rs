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
    let next_slot = crate::reduce_support::register_base(locals);
    let mut body_locals = locals.clone();
    let mut body_next_slot = next_slot;
    for statement in &statement.block.body {
        let names = crate::loops::body_var_names(statement);
        crate::loops::propagate_body_vars(&mut body_locals, &mut body_next_slot, &names);
    }
    let body = crate::reduce::reduce_statements_no_tail(
        &statement.block.body,
        facts,
        body_locals.clone(),
        body_next_slot,
    )?;
    let handler = statement
        .handler
        .as_ref()
        .map(|handler| {
            let (mut handler_locals, mut catch_slot) = handler_locals(handler, &body_locals);
            let mut handler_next_slot = crate::reduce_support::register_base(&handler_locals);
            for statement in &handler.body.body {
                let names = crate::loops::body_var_names(statement);
                for name in names {
                    handler_locals.insert(name, handler_next_slot);
                    handler_next_slot = handler_next_slot.saturating_add(1);
                }
            }
            for statement in &statement.block.body {
                let names = crate::loops::body_var_names(statement);
                for name in names {
                    handler_locals.insert(name, body_next_slot);
                    body_next_slot = body_next_slot.saturating_add(1);
                }
            }
            if let Some(parameter) = handler.param.as_ref() {
                if let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) =
                    &parameter.pattern.kind
                {
                    let slot = body_next_slot;
                    handler_locals.insert(identifier.name.to_string(), slot);
                    catch_slot = Some(slot);
                }
            }
            let ops = crate::reduce::reduce_statements_no_tail(
                &handler.body.body,
                facts,
                handler_locals,
                handler_next_slot,
            );
            ops.map(|ops| (ops, catch_slot))
        })
        .transpose()?;
    let (handler, catch_slot) = handler.map_or((None, None), |(ops, slot)| (Some(ops), slot));
    let finalizer = statement
        .finalizer
        .as_ref()
        .map(|finalizer| {
            crate::reduce::reduce_statements_no_tail(
                &finalizer.body,
                facts,
                locals.clone(),
                next_slot,
            )
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
