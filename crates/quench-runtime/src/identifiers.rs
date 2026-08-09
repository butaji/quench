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
    if let Some(register) =
        globals::reduce(identifier.name.as_str(), ops, facts, next_register, locals)
    {
        return Some(register);
    }
    let slot = *locals.get(identifier.name.as_str())?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::LoadLocal {
        dst: register,
        slot,
    });
    Some(register)
}
