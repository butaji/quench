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
    let mut block_locals = locals.clone();
    crate::reduce_support::predeclare_lexicals(&block.body, &mut block_locals, next_slot);
    block_locals.retain(|name, _| !name.starts_with("\0lexical-predeclared:"));
    let mut last = None;
    for statement in &block.body {
        if let oxc::ast::ast::Statement::FunctionDeclaration(function) = statement {
            if function.id.as_ref().is_some_and(|identifier| {
                facts
                    .eval_var_barrier
                    .contains(&identifier.name.to_string())
            }) {
                continue;
            }
        }
        if let Some(value) = reduce_statement(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            &mut block_locals,
        )? {
            last = Some(value);
        }
    }
    Ok(last)
}
