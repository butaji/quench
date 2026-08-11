//! Minimum-viable reduction of ES module declarations.
//!
//! The reducer keeps module declaration shape, but module graph resolution and
//! live bindings are deferred until a dedicated module runtime exists.

use std::collections::HashMap;

use oxc::ast::ast::{
    ExportDefaultDeclaration, ExportDefaultDeclarationKind, ExportNamedDeclaration,
    ImportDeclaration, ImportDeclarationSpecifier, Statement,
};

use crate::statements;
use crate::{facts::ProgramDb, ops::Op};

macro_rules! reduce_module_exported_decl {
    (function $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {{
        crate::reduce::reduce_function_declaration(
            $declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals,
        )
        .map(|_| None)
    }};
    (class $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {
        reduce_class(
            $declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals,
        )
    };
    (variable $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {{
        statements::reduce_variable(
            &$declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals,
        )
    }};
}

macro_rules! reduce_exported_default {
    (expression $expression:expr, $ops:expr, $facts:expr, $next_register:expr, $locals:expr) => {{
        if let Some(src) =
            crate::reduce::reduce_expression($expression, $ops, $facts, $next_register, $locals)
        {
            let _ = src;
        }
        Ok(None)
    }};
    (function $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {
        reduce_module_exported_decl!(
            function $declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals
        )
    };
    (class $declaration:expr, $ops:expr, $facts:expr, $next_register:expr, $next_slot:expr, $locals:expr) => {
        reduce_module_exported_decl!(
            class $declaration,
            $ops,
            $facts,
            $next_register,
            $next_slot,
            $locals
        )
    };
}

pub(super) fn reduce_module_declaration(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match statement {
        Statement::ImportDeclaration(import) => {
            reduce_import(import, ops, next_register, next_slot, locals)
        }
        Statement::ExportNamedDeclaration(export) => {
            reduce_named(export, ops, facts, next_register, next_slot, locals)
        }
        Statement::ExportDefaultDeclaration(export) => {
            reduce_default(export, ops, facts, next_register, next_slot, locals)
        }
        _ => Ok(None),
    }
}

fn reduce_import(
    import: &ImportDeclaration<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let mut declare = |name: &str| {
        let slot = *locals.entry(name.to_string()).or_insert_with(|| {
            let slot = *next_slot;
            *next_slot = next_slot.saturating_add(1);
            slot
        });
        let src = crate::reduce_support::emit_undefined(ops, next_register);
        ops.push(Op::StoreLocal { slot, src });
    };
    for specifier in import.specifiers.iter().flatten() {
        match specifier {
            ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                declare(default.local.name.as_str())
            }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns) => {
                declare(ns.local.name.as_str())
            }
            ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                declare(specifier.local.name.as_str())
            }
        }
    }
    Ok(None)
}

fn reduce_named(
    export: &ExportNamedDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    if let Some(declaration) = &export.declaration {
        return match declaration {
            oxc::ast::ast::Declaration::FunctionDeclaration(function) => {
                reduce_module_exported_decl!(
                    function function,
                    ops,
                    facts,
                    next_register,
                    next_slot,
                    locals
                )
            }
            oxc::ast::ast::Declaration::ClassDeclaration(class) => {
                reduce_module_exported_decl!(
                    class class,
                    ops,
                    facts,
                    next_register,
                    next_slot,
                    locals
                )
            }
            oxc::ast::ast::Declaration::VariableDeclaration(declaration) => {
                reduce_module_exported_decl!(
                    variable declaration,
                    ops,
                    facts,
                    next_register,
                    next_slot,
                    locals
                )
            }
            _ => Ok(None),
        };
    }
    Ok(None)
}

fn reduce_default(
    export: &ExportDefaultDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    if let Some(expression) = export.declaration.as_expression() {
        return reduce_exported_default!(
            expression expression,
            ops,
            facts,
            next_register,
            locals
        );
    }
    match &export.declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
            reduce_exported_default!(
                function function,
                ops,
                facts,
                next_register,
                next_slot,
                locals
            )
        }
        ExportDefaultDeclarationKind::ClassDeclaration(class) => {
            reduce_exported_default!(
                class class,
                ops,
                facts,
                next_register,
                next_slot,
                locals
            )
        }
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
