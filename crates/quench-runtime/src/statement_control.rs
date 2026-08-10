use std::collections::HashMap;

use oxc::ast::ast::Statement;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match statement {
        Statement::LabeledStatement(statement) => crate::reduce::reduce_statement(
            &statement.body,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ),
        Statement::IfStatement(statement) => crate::reduce::reduce_if_statement(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ),
        Statement::ForStatement(statement) => {
            crate::loops::reduce_for(statement, ops, facts, next_register, next_slot, locals)
                .map(|_| None)
        }
        Statement::WhileStatement(statement) => {
            crate::loops::reduce_while(statement, ops, facts, next_register, next_slot, locals)
                .map(|_| None)
        }
        Statement::DoWhileStatement(statement) => {
            crate::loops::reduce_do_while(statement, ops, facts, next_register, next_slot, locals)
                .map(|_| None)
        }
        Statement::ForInStatement(statement) => {
            crate::loops::reduce_for_in(statement, ops, facts, next_register, next_slot, locals)
                .map(|_| None)
        }
        Statement::TryStatement(statement) => {
            crate::control_flow::reduce_try_statement(statement, ops, facts, locals)
        }
        Statement::SwitchStatement(statement) => {
            crate::switch::reduce(statement, ops, facts, next_register, locals).map(|_| None)
        }
        Statement::BreakStatement(_) => {
            ops.push(Op::Break);
            Ok(None)
        }
        Statement::ContinueStatement(_) => {
            ops.push(Op::Continue);
            Ok(None)
        }
        _ => Err(vec![format!(
            "Unsupported executable statement: {statement:?}"
        )]),
    }
}
