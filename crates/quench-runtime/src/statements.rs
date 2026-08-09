use std::collections::HashMap;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn reduce_variable(
    declaration: &oxc::ast::ast::VariableDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    crate::reduce::reduce_declaration(declaration, ops, facts, next_register, next_slot, locals)
        .map(|_| None)
}

pub(crate) fn reduce_declaration(
    statement: &oxc::ast::ast::Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match statement {
        oxc::ast::ast::Statement::VariableDeclaration(value) => {
            reduce_variable(value, ops, facts, next_register, next_slot, locals)
        }
        oxc::ast::ast::Statement::FunctionDeclaration(value) => {
            crate::reduce::reduce_function_declaration(
                value,
                ops,
                facts,
                next_register,
                next_slot,
                locals,
            )
            .map(|_| None)
        }
        _ => Err(vec!["Invalid declaration statement".to_string()]),
    }
}
