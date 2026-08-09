//! OXC AST reduction for ES module declarations.
//!
//! Converts module declarations into residual module ops.

use crate::{
    facts::ProgramDb,
    ops::Op,
    reduce::{reduce_expression, reduce_statement},
};
use oxc::ast::ast::{ExportNamedDeclaration, ModuleDeclaration, Statement};
use std::collections::HashMap;

/// Check if a statement is a module declaration.
pub(crate) fn is_module_declaration(statement: &Statement<'_>) -> bool {
    statement.as_module_declaration().is_some()
}

/// Reduce a module declaration into module ops.
pub(crate) fn reduce_module_declaration(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let Some(declaration) = statement.as_module_declaration() else {
        return Ok(None);
    };
    match declaration {
        ModuleDeclaration::ImportDeclaration(import) => {
            reduce_import_declaration(import, ops, next_register, locals)
        }
        ModuleDeclaration::ExportNamedDeclaration(export) => {
            reduce_export_named(export, ops, facts, next_register, next_slot, locals)
        }
        ModuleDeclaration::ExportDefaultDeclaration(export) => {
            reduce_export_default(export, ops, facts, next_register, locals)
        }
        ModuleDeclaration::ExportAllDeclaration(export) => {
            reduce_export_all(export, ops, next_register)
        }
        // TypeScript module declarations - not supported yet
        _ => Ok(None),
    }
}

fn reduce_import_declaration(
    import: &oxc::ast::ast::ImportDeclaration<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let specifier = import.source.value.to_string();
    ops.push(Op::ImportModule { specifier });
    for item in &import.specifiers {
        reduce_import_specifier(item, &specifier, ops, next_register, locals);
    }
    Ok(None)
}

fn reduce_import_specifier(
    specifier: &oxc::ast::ast::ImportDeclarationSpecifier<'_>,
    module: &str,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) {
    let dst = { let r = *next_register; *next_register = next_register.saturating_add(1); r };
    match specifier {
        oxc::ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
            ops.push(Op::ImportNamed { dst, specifier: module.to_string(), name: "default".to_string() });
            store_if_local(&default.local.name.to_string(), dst, ops, locals);
        }
        oxc::ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns) => {
            ops.push(Op::ImportNamespace { dst, specifier: module.to_string() });
            store_if_local(&ns.local.name.to_string(), dst, ops, locals);
        }
        oxc::ast::ast::ImportDeclarationSpecifier::ImportSpecifier(spec) => {
            let name = spec.imported.as_identifier().map(|id| id.name.to_string())
                .or_else(|| spec.imported.as_string_literal().map(|s| s.value.to_string()))
                .unwrap_or_default();
            ops.push(Op::ImportNamed { dst, specifier: module.to_string(), name });
            store_if_local(&spec.local.name.to_string(), dst, ops, locals);
        }
    }
}

fn store_if_local(name: &str, src: u16, ops: &mut Vec<Op>, locals: &HashMap<String, u16>) {
    if let Some(slot) = locals.get(name) {
        ops.push(Op::StoreLocal { slot: *slot, src });
    }
}

fn reduce_export_named(
    export: &ExportNamedDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    if let Some(declaration) = &export.declaration {
        reduce_statement(&Statement::VariableDeclaration(declaration.clone()), ops, facts, next_register, next_slot, locals)?;
        return Ok(None);
    }
    for specifier in &export.specifiers {
        let name = specifier.local.as_identifier().map(|id| id.name.to_string()).unwrap_or_default();
        let src = locals.get(&name).copied().ok_or_else(|| vec![format!("Unknown export: {name}")])?;
        ops.push(Op::ExportValue { specifier: String::new(), name, src });
    }
    Ok(None)
}

fn reduce_export_default(
    export: &oxc::ast::ast::ExportDefaultDeclaration<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    if let Some(expr) = export.declaration.as_expression() {
        let src = reduce_expression(expr, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported export default expression".to_string()])?;
        ops.push(Op::ExportDefault { specifier: String::new(), src });
    }
    Ok(None)
}

fn reduce_export_all(
    export: &oxc::ast::ast::ExportAllDeclaration<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
) -> Result<Option<u16>, Vec<String>> {
    let specifier = export.source.value.to_string();
    let src = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::ExportAll { specifier, src });
    Ok(None)
}
