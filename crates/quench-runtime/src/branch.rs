use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};
use std::collections::HashMap;

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<Completion, VmError> {
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
    let Some(selected) = selected.ops() else {
        return Err(VmError::MissingReturn);
    };
    crate::execute::execute_completion_in_place(selected, registers)
}

pub(crate) fn reduce(
    statement: &oxc::ast::ast::Statement<'_>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
    next_slot: &mut u16,
) -> Result<(Vec<Op>, Option<u16>), Vec<String>> {
    match statement {
        oxc::ast::ast::Statement::BlockStatement(block) => {
            // Nested block bodies share the enclosing environment's slot
            // space, so their lexical slots must continue monotonically from
            // the caller's position rather than restarting at
            // register_base(locals). Restarting lets a nested binding collide
            // with a live enclosing binding (two names share one immutable
            // slot), which leaks const-immutability across loop iterations.
            let base = crate::reduce_support::register_base(locals);
            if *next_slot < base {
                *next_slot = base;
            }
            let (ops, last, final_slot) = crate::reduce::reduce_statements_no_tail_value(
                &block.body,
                facts,
                locals.clone(),
                *next_slot,
            )?;
            *next_slot = final_slot;
            Ok((ops, last))
        }
        statement => {
            let mut ops = Vec::new();
            let mut next_register = crate::reduce_support::register_base(locals);
            let mut body_slot = *next_slot;
            let mut locals = locals.clone();
            let last = crate::reduce::reduce_statement(
                statement,
                &mut ops,
                facts,
                &mut next_register,
                &mut body_slot,
                &mut locals,
            )?;
            *next_slot = body_slot;
            Ok((ops, last))
        }
    }
}
