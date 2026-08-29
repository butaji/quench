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
        .extend(crate::semantic::early::lexically_declared_names_in(
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
    let stack = crate::using::scope::reserve(&block.body, &mut block_locals, next_slot);
    crate::using::scope::emit_tdz(&block.body, ops, &block_locals);
    prepare_block_functions(&block.body, &mut block_locals, next_slot, ops);
    instantiate_block_functions(
        &block.body,
        ops,
        facts,
        next_register,
        next_slot,
        &mut block_locals,
    )?;
    let mut body = Vec::new();
    let last = reduce_block_statements(
        block,
        &mut body,
        facts,
        next_register,
        next_slot,
        &mut block_locals,
    )?;
    emit_wrapped(
        ops,
        body,
        stack,
        crate::using::scope::has_await_using(&block.body),
        next_register,
    )?;
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
    let own_functions = block_function_names(&block.body);
    let mut last = None;
    for statement in &block.body {
        if matches!(statement, oxc::ast::ast::Statement::FunctionDeclaration(_)) {
            continue;
        }
        let barrier = facts.eval_var_barrier.len();
        facts.eval_var_barrier.extend(own_functions.iter().cloned());
        // Track whether the upcoming statement may need to inherit the
        // previous V (used to carry the value through a `break` so that
        // the labelled statement spec can return it as the abrupt
        // completion's [[Value]]).
        let pending_abrupt = matches!(
            statement,
            oxc::ast::ast::Statement::BreakStatement(_)
                | oxc::ast::ast::Statement::ContinueStatement(_)
        );
        let start_ops = ops.len();
        let value = reduce_statement(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            block_locals,
        )?;
        facts.eval_var_barrier.truncate(barrier);
        if pending_abrupt {
            // Patch the just-emitted break/continue to carry `last`
            // as its V so the enclosing loop/label can surface it.
            patch_abrupt_value(ops, start_ops, last);
        }
        if let Some(value) = value {
            last = Some(value);
        }
    }
    Ok(last)
}

pub(crate) fn patch_abrupt_value(ops: &mut [Op], start: usize, value: Option<u16>) {
    for op in ops.iter_mut().skip(start) {
        match op {
            Op::Break { value: slot, .. } | Op::Continue { value: slot, .. } => {
                if slot.is_none() {
                    *slot = value;
                }
            }
            _ => {}
        }
    }
}

fn block_function_names(statements: &[oxc::ast::ast::Statement<'_>]) -> Vec<String> {
    statements
        .iter()
        .filter_map(|statement| {
            let oxc::ast::ast::Statement::FunctionDeclaration(function) = statement else {
                return None;
            };
            crate::reduce_support::annex_b_plain_function_name(function)
        })
        .collect()
}

pub(crate) fn prepare_block_functions(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
    ops: &mut Vec<Op>,
) {
    for name in block_function_names(statements) {
        crate::control_flow::preserve_annex_b_outer(locals, &name);
        if locals.contains_key(&format!("\0annex-b-lexical:{name}")) {
            continue;
        }
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert(name.clone(), slot);
        locals.insert(format!("\0annex-b-lexical:{name}"), slot);
        ops.push(Op::MarkUninitialized { slot, shared: true });
    }
}

fn instantiate_block_functions(
    statements: &[oxc::ast::ast::Statement<'_>],
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    for statement in statements {
        let oxc::ast::ast::Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        crate::reduce::reduce_function_declaration(
            function,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )?;
    }
    Ok(())
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
    crate::using::scope::emit_create(ops, stack, await_using, next_register);
    ops.extend(crate::using::scope::wrap(
        body,
        stack,
        await_using,
        next_register,
    )?);
    Ok(())
}

/// Reserve enclosing-scope slots for names that hoist out of the block. In
/// strict mode Annex B is suppressed, so block-level functions stay scoped.
pub(crate) fn hoist_var_names(
    block: &BlockStatement<'_>,
    strict: bool,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) {
    let hoisted = if strict {
        crate::semantic::early::strict_var_declared_names_in(&block.body)
    } else {
        crate::semantic::early::var_declared_names_in(&block.body)
    };
    let skip = crate::semantic::early::annex_b_lexical_collisions_in(&block.body);
    for name in hoisted {
        if skip.contains(&name) {
            continue;
        }
        locals.entry(name).or_insert_with(|| {
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            slot
        });
    }
}
