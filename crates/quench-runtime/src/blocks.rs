use std::collections::HashMap;

use oxc::ast::ast::BlockStatement;

use crate::{facts::ProgramDb, ops::Op, reduce::reduce_statement};

pub(crate) fn reduce(
    block: &BlockStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let barrier_len = facts.eval_var_barrier.len();
    facts
        .eval_var_barrier
        .extend(crate::semantic_early::lexically_declared_names_in(
            &block.body,
        ));
    let result = reduce_body(block, ops, facts, next_register, next_slot, locals);
    facts.eval_var_barrier.truncate(barrier_len);
    result
}

fn reduce_body(
    block: &BlockStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    hoist_var_names(block, facts.strict, next_slot, locals);
    let mut block_locals = locals.clone();
    crate::reduce_support::predeclare_lexicals(&block.body, &mut block_locals, next_slot);
    let stack = crate::using_scope::reserve(&block.body, &mut block_locals, next_slot);
    crate::using_scope::emit_tdz(&block.body, ops, &block_locals);
    block_locals.retain(|name, _| !name.starts_with("\0lexical-predeclared:"));
    let mut body = Vec::new();
    let last = reduce_block_statements(block, &mut body, facts, next_register, next_slot, &mut block_locals)?;
    emit_wrapped(ops, body, stack, crate::using_scope::has_await_using(&block.body), next_register)?;
    Ok(last)
}

fn reduce_block_statements(
    block: &BlockStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    block_locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let mut last = None;
    for statement in &block.body {
        if skip_blocked_function(statement, facts) {
            continue;
        }
        if let Some(value) = reduce_statement(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            block_locals,
        )? {
            last = Some(value);
        }
    }
    Ok(last)
}

fn skip_blocked_function(statement: &oxc::ast::ast::Statement<'_>, facts: &ProgramDb) -> bool {
    let oxc::ast::ast::Statement::FunctionDeclaration(function) = statement else {
        return false;
    };
    function.id.as_ref().is_some_and(|identifier| {
        facts
            .eval_var_barrier
            .contains(&identifier.name.to_string())
    })
}

fn emit_wrapped(
    ops: &mut Vec<Op>,
    body: Vec<Op>,
    stack: Option<u16>,
    await_using: bool,
    next_register: &mut u16,
) -> Result<(), Vec<String>> {
    let Some(stack) = stack else {
        ops.extend(body);
        return Ok(());
    };
    crate::using_scope::emit_create(ops, stack, await_using, next_register);
    ops.extend(crate::using_scope::wrap(
        body,
        stack,
        await_using,
        next_register,
    )?);
    Ok(())
}

/// Reserve enclosing-scope slots for names that hoist out of the block. In
/// strict mode Annex B is suppressed, so block-level functions stay scoped.
fn hoist_var_names(
    block: &BlockStatement<'_>,
    strict: bool,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) {
    let hoisted = if strict {
        crate::semantic_early::strict_var_declared_names_in(&block.body)
    } else {
        crate::semantic_early::var_declared_names_in(&block.body)
    };
    for name in hoisted {
        locals.entry(name).or_insert_with(|| {
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            slot
        });
    }
}
