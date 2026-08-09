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
        oxc::ast::ast::Statement::ClassDeclaration(value) => {
            reduce_class(value, ops, next_register, next_slot, locals)
        }
        _ => Err(vec!["Invalid declaration statement".to_string()]),
    }
}

fn reduce_class(
    class: &oxc::ast::ast::Class<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let Some(identifier) = class.id.as_ref() else {
        return Err(vec!["Anonymous class declaration".to_string()]);
    };
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(identifier.name.to_string(), slot);
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeFunction {
        dst: register,
        body: vec![
            Op::Const {
                dst: 0,
                value: crate::ops::Constant::Undefined,
            },
            Op::Return { src: 0 },
        ],
        params: 0,
        captures: 0,
    });
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    Ok(None)
}
