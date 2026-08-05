//! Export and import lowering functions
//!
//! Note: Most export/import lowering is now done directly in stmt/mod.rs.
//! This file is kept for backwards compatibility and any additional helpers.

use crate::ast::{Expression, PropertyKey, Statement};
use crate::lower::stmt::{lower_class_decl, lower_fn_decl, lower_var_decl};
use oxc::ast::ast;

use super::super::expr::lower_expr;
use super::declarations::lower_class;
use super::lower_stmt;

/// Lower an OXC ModuleDecl to a Statement
#[allow(clippy::complexity)]
pub fn lower_module_decl(decl: &ast::Statement) -> Option<Statement> {
    match decl {
        // export default function/class
        ast::Statement::ExportDefaultDeclaration(export) => lower_export_default_decl(export),
        // Named exports
        ast::Statement::ExportNamedDeclaration(export) => lower_export_named(export),
        // export * from 'module'
        ast::Statement::ExportAllDeclaration(export) => {
            let source = export.source.value.to_string();
            Some(lower_export_star_from(&source))
        }
        // import statements
        ast::Statement::ImportDeclaration(import) => lower_import(import),
        _ => None,
    }
}

/// Lower an import statement to our Import statement
#[allow(clippy::complexity)]
pub fn lower_import(import: &ast::ImportDeclaration) -> Option<Statement> {
    let source = import.source.value.to_string();
    let import_type = import.with_clause.as_ref().and_then(|clause| {
        clause.with_entries.iter().find_map(|entry| {
            let key = match &entry.key {
                ast::ImportAttributeKey::Identifier(key) => key.name.as_str(),
                ast::ImportAttributeKey::StringLiteral(key) => key.value.as_str(),
            };
            (key == "type").then(|| entry.value.value.to_string())
        })
    });
    let Some(specifiers) = import.specifiers.as_ref() else {
        return Some(Statement::Import {
            default: None,
            named: Vec::new(),
            namespace: None,
            source,
            deferred: matches!(import.phase, Some(ast::ImportPhase::Defer)),
            import_type,
        });
    };

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
        deferred: matches!(import.phase, Some(ast::ImportPhase::Defer)),
        import_type,
    })
}

fn module_export_name_to_string(name: &ast::ModuleExportName) -> String {
    match name {
        ast::ModuleExportName::IdentifierReference(i) => i.name.as_str().to_string(),
        ast::ModuleExportName::IdentifierName(i) => i.name.as_str().to_string(),
        ast::ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

/// Lower `export * from 'module'` to re-export all bindings
/// Creates: import * as ns from 'src'; for (let k in ns) exports[k] = ns[k];
pub fn lower_export_star_from(source: &str) -> Statement {
    let unique_name = format!("_star_{}", source.replace(['/', '-', '.'], "_"));
    let import_stmt = Statement::Import {
        default: None,
        named: vec![],
        namespace: Some(unique_name.clone()),
        source: source.to_string(),
        deferred: false,
        import_type: None,
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

pub fn lower_export_default_decl(export: &ast::ExportDefaultDeclaration) -> Option<Statement> {
    use crate::ast::Param;

    match &export.declaration {
        ast::ExportDefaultDeclarationKind::FunctionDeclaration(func_expr) => {
            let name = func_expr
                .id
                .as_ref()
                .map(|i| i.name.as_str().to_string())
                .unwrap_or_else(|| "default".to_string());
            let params: Vec<Param> = func_expr
                .params
                .items
                .iter()
                .map(|p| {
                    // OXC 0.142+ stores defaults in p.initializer
                    let mut param = crate::lower::stmt::lower_param_decl(p);
                    param.pattern = None;
                    param.rest = false;
                    param
                })
                .collect();
            let body = func_expr
                .body
                .as_ref()
                .map(|b| b.statements.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            let declaration = Statement::FunctionDeclaration {
                name,
                params,
                body,
                is_async: func_expr.r#async,
                is_generator: func_expr.generator,
            };
            Some(default_export_binding(
                declaration,
                func_expr.id.as_ref().map(|id| id.name.as_str()),
            ))
        }
        ast::ExportDefaultDeclarationKind::ClassDeclaration(class_expr) => {
            let original_name = class_expr.id.as_ref().map(|id| id.name.as_str());
            let name = class_expr
                .id
                .as_ref()
                .map(|i| i.name.as_str().to_string())
                .unwrap_or_else(|| "default".to_string());
            let class = lower_class(class_expr)?;
            Some(default_export_binding(
                Statement::ClassDeclaration { name, class },
                original_name,
            ))
        }
        // Expression export: export default <expression> -> exports.default = <expression>
        ast::ExportDefaultDeclarationKind::Identifier(id) => Some(default_export_value(
            Expression::Identifier(id.name.as_str().to_string()),
        )),
        _ => export
            .declaration
            .as_expression()
            .and_then(|expression| lower_expr(expression).ok())
            .map(default_export_value),
    }
}

fn default_export_binding(declaration: Statement, name: Option<&str>) -> Statement {
    let Some(name) = name else {
        return declaration;
    };
    Statement::Block(vec![
        declaration,
        default_export_value(Expression::Identifier(name.into())),
    ])
}

fn default_export_value(value: Expression) -> Statement {
    Statement::VarDeclaration {
        kind: crate::ast::VarKind::Var,
        name: "default".to_string(),
        init: Some(value),
    }
}

/// Lower export { foo, bar } to exports.foo = foo; exports.bar = bar;
/// Lower export { foo } from 'module' to imports.foo; exports.foo = foo;
pub fn lower_export_named(named: &ast::ExportNamedDeclaration) -> Option<Statement> {
    if let Some(declaration) = &named.declaration {
        let statement = match declaration {
            ast::Declaration::VariableDeclaration(declaration) => lower_var_decl(declaration),
            ast::Declaration::FunctionDeclaration(declaration) => lower_fn_decl(declaration),
            ast::Declaration::ClassDeclaration(declaration) => lower_class_decl(declaration),
            _ => None,
        }?;
        return Some(Statement::Export(Box::new(statement)));
    }
    // Handle export-from syntax: export { x } from 'module'
    if let Some(src) = &named.source {
        let source = src.value.to_string();
        let (imports, stmts) = collect_export_from_specs(&named.specifiers);
        let import_stmt = Statement::Import {
            default: None,
            named: imports,
            namespace: None,
            source,
            deferred: false,
            import_type: None,
        };
        let mut all_stmts = vec![import_stmt];
        all_stmts.extend(stmts);
        return Some(Statement::Block(all_stmts));
    }

    // In OXC, ExportSpecifier is a struct with local and exported fields
    let mut stmts = Vec::new();
    for spec in &named.specifiers {
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

/// Lower `export { x } from 'module'` to import and re-export
#[allow(dead_code)]
fn lower_export_from(
    named: &ast::ExportNamedDeclaration,
    src: &ast::StringLiteral,
) -> Option<Statement> {
    let source = src.value.to_string();
    let (imports, stmts) = collect_export_from_specs(&named.specifiers);
    let import_stmt = Statement::Import {
        default: None,
        named: imports,
        namespace: None,
        source,
        deferred: false,
        import_type: None,
    };
    let mut all_stmts = vec![import_stmt];
    all_stmts.extend(stmts);
    Some(Statement::Block(all_stmts))
}

pub(crate) fn collect_export_from_specs(
    specs: &[ast::ExportSpecifier],
) -> (Vec<(String, String)>, Vec<Statement>) {
    let mut imports = Vec::new();
    let mut stmts = Vec::new();
    for spec in specs {
        // In OXC, ExportSpecifier is a struct with local and exported fields
        let exported = module_export_name_to_string(&spec.exported);
        let local = module_export_name_to_string(&spec.local);
        let stmt = make_export_assignment(&exported, &local);
        imports.push((local.clone(), local));
        stmts.push(stmt);
    }
    (imports, stmts)
}

pub(crate) fn make_export_assignment(prop: &str, value: &str) -> Statement {
    Statement::Expression(Box::new(Expression::Assignment {
        left: Box::new(Expression::Member {
            object: Box::new(Expression::Identifier("exports".to_string())),
            property: PropertyKey::Ident(prop.to_string()),
            computed: false,
        }),
        right: Box::new(Expression::Identifier(value.to_string())),
    }))
}

// Re-export declaration lowering functions
