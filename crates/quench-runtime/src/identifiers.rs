use std::collections::HashMap;

use oxc::ast::ast::IdentifierReference;

use crate::{facts::ProgramDb, globals, ops::Op};

pub(crate) fn reduce(
    identifier: &IdentifierReference<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Some(register) = reduce_global(identifier, ops, facts, next_register, locals) {
        return Some(register);
    }
    if let Some(register) = reduce_local(identifier, ops, next_register, locals) {
        return Some(register);
    }
    Some(throw_reference_error(identifier, ops, next_register))
}

fn reduce_global(
    identifier: &IdentifierReference<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    globals::reduce(identifier.name.as_str(), ops, facts, next_register, locals)
}

fn reduce_local(
    identifier: &IdentifierReference<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let slot = *locals.get(identifier.name.as_str())?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::LoadLocal {
        dst: register,
        slot,
    });
    Some(register)
}

fn throw_reference_error(
    identifier: &IdentifierReference<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
) -> u16 {
    let constructor = allocate_register(next_register);
    ops.push(Op::MakeBuiltin {
        dst: constructor,
        builtin: crate::ops::Builtin::ReferenceError,
    });
    let message = allocate_register(next_register);
    ops.push(Op::Const {
        dst: message,
        value: crate::ops::Constant::String(format!("{} is not defined", identifier.name)),
    });
    let error = allocate_register(next_register);
    ops.push(Op::Construct {
        dst: error,
        callee: constructor,
        args: vec![message],
    });
    ops.push(Op::Throw { src: error });
    error
}

fn allocate_register(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}
