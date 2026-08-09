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
    let mut last = None;
    for statement in &block.body {
        if let Some(value) =
            reduce_statement(statement, ops, facts, next_register, next_slot, locals)?
        {
            last = Some(value);
        }
    }
    Ok(last)
}
