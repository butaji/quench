//! Statement lowering module - convert OXC statements to runtime AST statements

mod declarations;
mod destructuring;
mod exports;

pub use declarations::*;
pub use destructuring::*;
pub use exports::*;

// Re-export for use by other modules
use crate::ast::{Expression, PropertyKey, Statement};
use crate::lower::control_flow::{
    lower_do_while_stmt, lower_for_in_stmt, lower_for_of_stmt, lower_for_stmt, lower_if_stmt,
    lower_switch, lower_try_stmt, lower_while_stmt,
};
use crate::lower::expr::lower_expr;
use crate::lower::helpers::LowerError;
use oxc::ast::ast;

/// Lower an OXC Program to our runtime Program
pub fn lower_program(program: &ast::Program) -> Result<crate::ast::Program, LowerError> {
    if program.source_type.is_module() {
        lower_module(program)
    } else {
        lower_script(program)
    }
}

/// Lower an OXC Module to our runtime Program
pub fn lower_module(module: &ast::Program) -> Result<crate::ast::Program, LowerError> {
    let mut statements: Vec<Statement> = Vec::new();
    let mut export_stmts: Vec<Statement> = Vec::new();

    // OXC stores directive strings (e.g. "use strict") in program.directives.
    // Prepend them as expression statements so the interpreter can detect them.
    for directive in &module.directives {
        let expr = Expression::String(directive.expression.value.to_string());
        statements.push(Statement::Expression(Box::new(expr)));
    }

    for stmt in &module.body {
        match lower_stmt(stmt) {
            Some(Statement::Export(stmt)) => export_stmts.push(*stmt),
            Some(stmt) => statements.push(stmt),
            None => {}
        }
    }

    // If we have export statements, add them at the end
    statements.extend(export_stmts);

    Ok(crate::ast::Program::Script(statements))
}

/// Lower an OXC Script to our runtime Program
pub fn lower_script(script: &ast::Program) -> Result<crate::ast::Program, LowerError> {
    let mut statements: Vec<Statement> = Vec::new();

    // OXC stores directive strings (e.g. "use strict") in program.directives,
    // not in program.body. Prepend them as expression statements so the
    // interpreter's check_use_strict_directive can find them.
    for directive in &script.directives {
        let expr = Expression::String(directive.expression.value.to_string());
        statements.push(Statement::Expression(Box::new(expr)));
    }

    for stmt in &script.body {
        if let Some(s) = lower_stmt_checked(stmt)? {
            statements.push(s);
        }
    }
    Ok(crate::ast::Program::Script(statements))
}

/// Lower a statement, propagating an error for truly unsupported statements
fn lower_stmt_checked(stmt: &ast::Statement) -> Result<Option<Statement>, LowerError> {
    match stmt {
        ast::Statement::WithStatement(_) => {
            Err(LowerError::new("`with` statements are not supported"))
        }
        _ => Ok(lower_stmt(stmt)),
    }
}

/// Lower an OXC Statement to our Statement
#[allow(unreachable_patterns, clippy::complexity)]
pub fn lower_stmt(stmt: &ast::Statement) -> Option<Statement> {
    match stmt {
        ast::Statement::EmptyStatement(_) => Some(Statement::Empty),
        ast::Statement::BlockStatement(block) => {
            let stmts: Vec<Statement> = block.body.iter().filter_map(lower_stmt).collect();
            Some(Statement::Block(stmts))
        }
        ast::Statement::BreakStatement(_) => Some(Statement::Break(None)),
        ast::Statement::ContinueStatement(_) => Some(Statement::Continue(None)),
        ast::Statement::DebuggerStatement(_) => Some(Statement::Empty),
        ast::Statement::WithStatement(w) => {
            let object = lower_expr(&w.object).ok()?;
            let body = Box::new(lower_stmt(&w.body)?);
            Some(Statement::With {
                object: Box::new(object),
                body,
            })
        }
        ast::Statement::VariableDeclaration(var_decl) => lower_var_decl(var_decl),
        ast::Statement::FunctionDeclaration(func_decl) => lower_fn_decl(func_decl),
        ast::Statement::ClassDeclaration(class_decl) => lower_class_decl(class_decl),
        ast::Statement::ReturnStatement(ret) => {
            let expr = ret.argument.as_ref().and_then(|e| lower_expr(e).ok());
            Some(Statement::Return(expr.map(Box::new)))
        }
        ast::Statement::LabeledStatement(labeled) => lower_stmt(&labeled.body),
        ast::Statement::IfStatement(if_stmt) => lower_if_stmt(if_stmt),
        ast::Statement::SwitchStatement(switch) => lower_switch(switch),
        ast::Statement::ThrowStatement(throw) => {
            let expr = lower_expr(&throw.argument).ok()?;
            Some(Statement::Throw(Box::new(expr)))
        }
        ast::Statement::TryStatement(try_stmt) => lower_try_stmt(try_stmt),
        ast::Statement::WhileStatement(while_stmt) => lower_while_stmt(while_stmt),
        ast::Statement::DoWhileStatement(do_while) => lower_do_while_stmt(do_while),
        ast::Statement::ForStatement(for_stmt) => lower_for_stmt(for_stmt),
        ast::Statement::ForInStatement(for_in_stmt) => lower_for_in_stmt(for_in_stmt),
        ast::Statement::ForOfStatement(for_of_stmt) => lower_for_of_stmt(for_of_stmt),
        ast::Statement::ExpressionStatement(expr_stmt) => {
            let expr = lower_expr(&expr_stmt.expression).ok()?;
            Some(Statement::Expression(Box::new(expr)))
        }
        ast::Statement::ImportDeclaration(import) => lower_import_decl(import),
        ast::Statement::ExportNamedDeclaration(export) => lower_export_named(export),
        ast::Statement::ExportDefaultDeclaration(export) => lower_export_default_decl(export),
        ast::Statement::ExportAllDeclaration(export) => lower_export_all_decl(export),
        _ => None,
    }
}

fn lower_import_decl(import: &ast::ImportDeclaration) -> Option<Statement> {
    let source = import.source.value.to_string();
    let specifiers = import.specifiers.as_ref()?;

    let default = specifiers.iter().find_map(|spec| {
        if let ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(default_spec) = spec {
            Some(default_spec.local.name.as_str().to_string())
        } else {
            None
        }
    });
    let namespace = specifiers.iter().find_map(|spec| {
        if let ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns_spec) = spec {
            Some(ns_spec.local.name.as_str().to_string())
        } else {
            None
        }
    });
    let named: Vec<(String, String)> = specifiers
        .iter()
        .filter_map(|spec| {
            if let ast::ImportDeclarationSpecifier::ImportSpecifier(named_spec) = spec {
                let local = named_spec.local.name.as_str().to_string();
                let imported = module_export_name_to_string(&named_spec.imported);
                // Always include named imports (local may differ from imported in alias case)
                Some((local, imported))
            } else {
                None
            }
        })
        .collect();
    Some(Statement::Import {
        default,
        named,
        namespace,
        source,
    })
}

fn module_export_name_to_string(name: &ast::ModuleExportName) -> String {
    match name {
        ast::ModuleExportName::IdentifierReference(i) => i.name.as_str().to_string(),
        ast::ModuleExportName::IdentifierName(i) => i.name.as_str().to_string(),
        ast::ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

#[allow(dead_code)]
fn lower_export_named_local(export: &ast::ExportNamedDeclaration) -> Option<Statement> {
    // Handle export-from syntax: export { x } from 'module'
    if let Some(src) = &export.source {
        let source = src.value.to_string();
        let (imports, stmts) = collect_export_from_specs(&export.specifiers);
        let import_stmt = Statement::Import {
            default: None,
            named: imports,
            namespace: None,
            source,
        };
        let mut all_stmts = vec![import_stmt];
        all_stmts.extend(stmts);
        return Some(Statement::Block(all_stmts));
    }

    // Handle direct exports - ExportSpecifier is a struct with local and exported fields
    let mut stmts = Vec::new();
    for spec in &export.specifiers {
        let exported = module_export_name_to_string(&spec.exported);
        let local = module_export_name_to_string(&spec.local);
        stmts.push(make_export_assignment(&exported, &local));
    }

    if stmts.is_empty() {
        None
    } else if stmts.len() == 1 {
        Some(Statement::Export(Box::new(
            stmts.into_iter().next().unwrap(),
        )))
    } else {
        Some(Statement::Export(Box::new(Statement::Block(stmts))))
    }
}

#[allow(dead_code)]
fn lower_export_default_decl_local(export: &ast::ExportDefaultDeclaration) -> Option<Statement> {
    use crate::ast::Param;

    match &export.declaration {
        ast::ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
            let name = func
                .id
                .as_ref()
                .map(|i| i.name.as_str().to_string())
                .unwrap_or_else(|| "default".to_string());
            let params: Vec<Param> = func
                .params
                .items
                .iter()
                .map(|p| {
                    let (name, default) = match &p.pattern.kind {
                        ast::BindingPatternKind::BindingIdentifier(ident) => {
                            (ident.name.as_str().to_string(), None)
                        }
                        ast::BindingPatternKind::AssignmentPattern(ap) => {
                            let name = match &ap.left.kind {
                                ast::BindingPatternKind::BindingIdentifier(ident) => {
                                    ident.name.as_str().to_string()
                                }
                                _ => "arg".to_string(),
                            };
                            let default = lower_expr(&ap.right).ok().map(Box::new);
                            (name, default)
                        }
                        _ => ("arg".to_string(), None),
                    };
                    Param {
                        name,
                        default,
                        pattern: None,
                        rest: false,
                    }
                })
                .collect();
            let body = func
                .body
                .as_ref()
                .map(|b| b.statements.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            Some(Statement::FunctionDeclaration {
                name,
                params,
                body,
                is_async: func.r#async,
                is_generator: func.generator,
            })
        }
        ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
            let name = class
                .id
                .as_ref()
                .map(|i| i.name.as_str().to_string())
                .unwrap_or_else(|| "default".to_string());
            let class = lower_class(class)?;
            Some(Statement::ClassDeclaration { name, class })
        }
        // Expression variants: export default <expression>
        ast::ExportDefaultDeclarationKind::Identifier(id) => {
            Some(Statement::Expression(Box::new(Expression::Assignment {
                left: Box::new(Expression::Member {
                    object: Box::new(Expression::Identifier("exports".to_string())),
                    property: PropertyKey::Ident("default".to_string()),
                    computed: false,
                }),
                right: Box::new(Expression::Identifier(id.name.as_str().to_string())),
            })))
        }
        _ => {
            // Try to lower as expression
            if let Some(expr) = export.declaration.as_expression() {
                let lowered = lower_expr(expr).ok()?;
                Some(Statement::Expression(Box::new(Expression::Assignment {
                    left: Box::new(Expression::Member {
                        object: Box::new(Expression::Identifier("exports".to_string())),
                        property: PropertyKey::Ident("default".to_string()),
                        computed: false,
                    }),
                    right: Box::new(lowered),
                })))
            } else {
                None
            }
        }
    }
}

fn lower_export_all_decl(export: &ast::ExportAllDeclaration) -> Option<Statement> {
    let source = export.source.value.to_string();
    Some(lower_export_star_from_local(&source))
}

fn lower_export_star_from_local(source: &str) -> Statement {
    let unique_name = format!("_star_{}", source.replace(['/', '-', '.'], "_"));
    let import_stmt = Statement::Import {
        default: None,
        named: vec![],
        namespace: Some(unique_name.clone()),
        source: source.to_string(),
    };
    // Lower the for-in loop manually using statements
    // for (let k in ns) exports[k] = ns[k];
    let iterable = Expression::Identifier(unique_name.clone());
    let variable = Expression::Identifier("_k".to_string());
    let body = Statement::Expression(Box::new(Expression::Assignment {
        left: Box::new(Expression::Member {
            object: Box::new(Expression::Identifier("exports".to_string())),
            property: PropertyKey::Computed(Box::new(Expression::Identifier("_k".to_string()))),
            computed: true,
        }),
        right: Box::new(Expression::Member {
            object: Box::new(Expression::Identifier(unique_name)),
            property: PropertyKey::Computed(Box::new(Expression::Identifier("_k".to_string()))),
            computed: true,
        }),
    }));
    let for_in = Statement::ForIn {
        variable: Box::new(variable),
        object: Box::new(iterable),
        body: Box::new(body),
    };
    Statement::Block(vec![import_stmt, for_in])
}

#[allow(dead_code)]
fn collect_export_from_specs_mod(
    specs: &[ast::ExportSpecifier],
) -> (Vec<(String, String)>, Vec<Statement>) {
    let mut imports = Vec::new();
    let mut stmts = Vec::new();
    for spec in specs {
        let exported = module_export_name_to_string(&spec.exported);
        let local = module_export_name_to_string(&spec.local);
        let stmt = make_export_assignment(&exported, &local);
        imports.push((local.clone(), local));
        stmts.push(stmt);
    }
    (imports, stmts)
}

#[allow(dead_code)]
fn module_export_name_to_string_mod(name: &ast::ModuleExportName) -> String {
    match name {
        ast::ModuleExportName::IdentifierReference(i) => i.name.as_str().to_string(),
        ast::ModuleExportName::IdentifierName(i) => i.name.as_str().to_string(),
        ast::ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}
