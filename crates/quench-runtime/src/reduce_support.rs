//! Small reduction helpers shared across the reducer.

use std::collections::HashMap;

use crate::ops::{Constant, Op};

/// Highest allocated local slot plus one, used as the next register base.
pub(crate) fn register_base(locals: &HashMap<String, u16>) -> u16 {
    locals
        .values()
        .copied()
        .max()
        .map_or(0, |slot| slot.saturating_add(1))
}

/// Reserve a local slot for each top-level function declaration.
pub(crate) fn predeclare_functions(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        let oxc::ast::ast::Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        let Some(identifier) = function.id.as_ref() else {
            continue;
        };
        if locals.contains_key(identifier.name.as_str()) {
            continue;
        }
        locals.insert(identifier.name.to_string(), *next_slot);
        *next_slot = next_slot.saturating_add(1);
    }
}

/// Append a terminal `Return`, either the last expression value or `undefined`.
pub(crate) fn finish_program(
    mut ops: Vec<Op>,
    last_value: Option<u16>,
) -> Result<Vec<Op>, Vec<String>> {
    if let Some(register) = last_value {
        ops.push(Op::Return { src: register });
    } else {
        ops.push(Op::Const {
            dst: 0,
            value: Constant::Undefined,
        });
        ops.push(Op::Return { src: 0 });
    }
    Ok(ops)
}

/// Emit a constant `undefined` register and return it.
pub(crate) fn emit_undefined(ops: &mut Vec<Op>, next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: register,
        value: Constant::Undefined,
    });
    register
}
