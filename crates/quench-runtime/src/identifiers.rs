use oxc::ast::ast::IdentifierReference;
use std::collections::HashMap;

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
    resolve_name(identifier, ops, facts, next_register)
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
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let slot = *locals.get(identifier.name.as_str())?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    let name = interned_name(facts, identifier.name.as_str());
    ops.push(Op::LoadBinding {
        dst: register,
        slot,
        name,
        dynamic: facts.in_function && slot < facts.eval_var_scope_start,
    });
    Some(register)
}

fn resolve_name(
    identifier: &IdentifierReference<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
) -> Option<u16> {
    let dst = allocate_register(next_register);
    let key = interned_name(facts, identifier.name.as_str());
    ops.push(Op::ResolveName { dst, key });
    Some(dst)
}

fn interned_name(facts: &mut ProgramDb, name: &str) -> String {
    let id = facts.identifier_names.intern(name);
    facts
        .identifier_names
        .resolve(id)
        .expect("newly interned identifier")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interner_reuses_ids_and_preserves_content() {
        let mut table = crate::facts::IdentifierInterner::default();
        let first = table.intern("alpha");
        assert_eq!(table.intern("alpha"), first);
        let other = table.intern("beta");
        assert_ne!(first, other);
        assert_eq!(table.resolve(first), Some("alpha"));
        assert_eq!(table.resolve(other), Some("beta"));
        assert_eq!(table.resolve(u32::MAX), None);
    }

    #[test]
    fn separate_program_databases_do_not_share_identifier_ids() {
        let mut left = ProgramDb::default();
        let mut right = ProgramDb::default();
        assert_eq!(left.identifier_names.intern("same"), 0);
        assert_eq!(right.identifier_names.intern("same"), 0);
        assert_eq!(left.identifier_names.resolve(0), Some("same"));
        assert_eq!(right.identifier_names.resolve(0), Some("same"));
    }
}

fn allocate_register(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}
