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
    if !facts.has_dynamic_scope() {
        if let Some(register) = reduce_global(identifier, ops, facts, next_register, locals) {
            return Some(register);
        }
    }
    if let Some(register) = reduce_local(identifier, ops, facts, next_register, locals) {
        return Some(register);
    }
    resolve_name(identifier, ops, next_register)
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
    facts: &ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let slot = *locals.get(identifier.name.as_str())?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::LoadBinding {
        dst: register,
        slot,
        name: identifier.name.to_string(),
        dynamic: facts.in_function && slot < facts.eval_var_scope_start,
    });
    Some(register)
}

fn resolve_name(
    identifier: &IdentifierReference<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
) -> Option<u16> {
    let dst = allocate_register(next_register);
    ops.push(Op::ResolveName {
        dst,
        key: identifier.name.to_string(),
    });
    Some(dst)
}

fn allocate_register(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}
