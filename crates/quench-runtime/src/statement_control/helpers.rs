use std::collections::HashMap;

use oxc::ast::ast::{BreakStatement, ContinueStatement, LabeledStatement, Statement};

use crate::{facts::ProgramDb, ops::Op, statement_control::ReduceResult};

type Locals = HashMap<String, u16>;

pub(super) fn reduce_labeled_or_conditional(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut Locals,
) -> Option<ReduceResult> {
    match statement {
        Statement::LabeledStatement(statement) => Some(reduce_labeled(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )),
        Statement::IfStatement(statement) => Some(crate::reduce::reduce_if_statement(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )),
        _ => None,
    }
}

pub(super) fn reduce_loop_statement(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut Locals,
) -> Option<ReduceResult> {
    if let Some(result) =
        reduce_counted_loop(statement, ops, facts, next_register, next_slot, locals)
    {
        return Some(result);
    }
    reduce_enumeration_loop(statement, ops, facts, next_register, next_slot, locals)
}

fn reduce_counted_loop(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut Locals,
) -> Option<ReduceResult> {
    match statement {
        Statement::ForStatement(statement) => Some(loop_result(crate::loops::reduce_for(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ))),
        Statement::WhileStatement(statement) => Some(loop_result(crate::loops::reduce_while(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ))),
        Statement::DoWhileStatement(statement) => Some(loop_result(crate::loops::reduce_do_while(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ))),
        _ => None,
    }
}

fn reduce_enumeration_loop(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut Locals,
) -> Option<ReduceResult> {
    match statement {
        Statement::ForInStatement(statement) => Some(loop_result(crate::loops::reduce_for_in(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ))),
        Statement::ForOfStatement(statement) => Some(loop_result(crate::loops::reduce_for_of(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ))),
        _ => None,
    }
}

pub(super) fn reduce_control_statement(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &mut Locals,
) -> Option<ReduceResult> {
    match statement {
        Statement::TryStatement(statement) => Some(crate::control_flow::reduce_try_statement(
            statement, ops, facts, locals,
        )),
        Statement::SwitchStatement(statement) => {
            Some(crate::switch::reduce(statement, ops, facts, next_register, locals).map(|_| None))
        }
        Statement::BreakStatement(statement) => Some(reduce_break(statement, ops)),
        Statement::ContinueStatement(statement) => Some(reduce_continue(statement, ops)),
        _ => None,
    }
}

pub(super) fn unsupported_statement(statement: &Statement<'_>) -> ReduceResult {
    Err(vec![format!(
        "Unsupported executable statement: {statement:?}"
    )])
}

fn reduce_labeled(
    statement: &LabeledStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut Locals,
) -> ReduceResult {
    let start = ops.len();
    let wraps_loop = wraps_loop(&statement.body);
    let result = crate::reduce::reduce_statement(
        &statement.body,
        ops,
        facts,
        next_register,
        next_slot,
        locals,
    )?;
    if wraps_loop {
        set_loop_label(&mut ops[start..], statement.label.name.to_string());
    }
    Ok(result)
}

fn wraps_loop(statement: &Statement<'_>) -> bool {
    matches!(
        statement,
        Statement::ForStatement(_)
            | Statement::WhileStatement(_)
            | Statement::DoWhileStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
    )
}

fn set_loop_label(ops: &mut [Op], label: String) {
    if let Some(
        Op::Loop { label: slot, .. }
        | Op::ForIn { label: slot, .. }
        | Op::ForOf { label: slot, .. },
    ) = ops.last_mut()
    {
        *slot = Some(label);
    }
}

fn loop_result(result: Result<(), Vec<String>>) -> ReduceResult {
    result.map(|_| None)
}

fn reduce_break(statement: &BreakStatement<'_>, ops: &mut Vec<Op>) -> ReduceResult {
    let label = statement.label.as_ref().map(|label| label.name.to_string());
    ops.push(Op::Break { label });
    Ok(None)
}

fn reduce_continue(statement: &ContinueStatement<'_>, ops: &mut Vec<Op>) -> ReduceResult {
    let label = statement.label.as_ref().map(|label| label.name.to_string());
    ops.push(Op::Continue { label });
    Ok(None)
}
