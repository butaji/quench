//! Minimum-viable reduction of ES module declarations.
//!
//! Module graph and binding resolution are out of scope; the reducer only
//! skips import declarations and forwards any wrapped function or class
//! declaration to the regular reducer so its body executes against the
//! host environment.

use std::collections::HashMap;

use oxc::ast::ast::{ExportDefaultDeclaration, ExportNamedDeclaration, Statement};

use crate::{facts::ProgramDb, ops::Op};

use super::reduce_statements::reduce_function_declaration;

pub(super) fn reduce_module_declaration(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match statement {
        Statement::ImportDeclaration(_) | Statement::ExportAllDeclaration(_) => Ok(None),
        Statement::ExportNamedDeclaration(export) => {
            reduce_named(export, ops, facts, next_register, next_slot, locals)
        }
        Statement::ExportDefaultDeclaration(export) => {
            reduce_default(export, ops, facts, next_register, next_slot, locals)
        }
        _ => Ok(None),
    }
}

fn reduce_named(
    export: &ExportNamedDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match export.declaration.as_ref() {
        Some(oxc::ast::ast::Declaration::FunctionDeclaration(function)) => {
            reduce_function_declaration(function, ops, facts, next_register, next_slot, locals)?;
            Ok(None)
        }
        Some(oxc::ast::ast::Declaration::ClassDeclaration(class)) => Ok(reduce_class(
            class,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )?),
        _ => Ok(None),
    }
}

fn reduce_default(
    export: &ExportDefaultDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match &export.declaration {
        oxc::ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
            reduce_function_declaration(function, ops, facts, next_register, next_slot, locals)?;
            Ok(None)
        }
        oxc::ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => Ok(reduce_class(
            class,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )?),
        _ => Ok(None),
    }
}

fn reduce_class(
    class: &oxc::ast::ast::Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
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
    let register = crate::classes::reduce_expression(class, ops, facts, next_register, locals)
        .ok_or_else(|| vec!["Unsupported class body".to_string()])?;
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    Ok(None)
}
