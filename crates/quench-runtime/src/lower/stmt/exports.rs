//! Export lowering functions

use swc_ecma_ast as swc;
use crate::ast::{Expression, PropertyKey, Statement};
use crate::lower::expr::lower_expr;
use crate::lower::helpers::{atom_to_string, wtf8_atom_to_string};

use super::lower_stmt;

/// Lower a swc ModuleDecl to a Statement
pub fn lower_module_decl(decl: &swc::ModuleDecl) -> Option<Statement> {
    match decl {
        // export default function foo() { ... }
        swc::ModuleDecl::ExportDefaultDecl(export_decl) => {
            lower_export_default_decl(export_decl)
        }
        // export default expr
        swc::ModuleDecl::ExportDefaultExpr(expr) => {
            Some(Statement::Expression(Box::new(lower_expr(&expr.expr).ok()?)))
        }
        // export const/let/function/class declarations
        swc::ModuleDecl::ExportDecl(export_decl) => {
            lower_decl(&export_decl.decl)
        }
        // export { foo, bar }
        swc::ModuleDecl::ExportNamed(named) => {
            lower_export_named(named)
        }
        // export * from 'module' (not supported, skip)
        swc::ModuleDecl::ExportAll(_) => None,
        // import foo from 'module' (strip for CommonJS fallback)
        swc::ModuleDecl::Import(_) => None,
        // TypeScript export =
        swc::ModuleDecl::TsExportAssignment(_) => None,
        // TypeScript import =
        swc::ModuleDecl::TsImportEquals(_) => None,
        // TypeScript namespace export
        swc::ModuleDecl::TsNamespaceExport(_) => None,
    }
}

fn lower_export_default_decl(export_decl: &swc::ExportDefaultDecl) -> Option<Statement> {
    match &export_decl.decl {
        swc::DefaultDecl::Fn(func_expr) => {
            let name = func_expr.ident.as_ref()
                .map(|i| i.sym.to_string())
                .unwrap_or_else(|| "default".to_string());
            let params = func_expr.function.params.iter().map(|p| {
                match &p.pat {
                    swc::Pat::Ident(ident) => ident.id.sym.to_string(),
                    _ => "".to_string(),
                }
            }).collect();
            let body = func_expr.function.body.as_ref()
                .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
                .unwrap_or_default();
            Some(Statement::FunctionDeclaration { name, params, body })
        }
        swc::DefaultDecl::Class(class_expr) => {
            let name = class_expr.ident.as_ref()
                .map(|i| i.sym.to_string())
                .unwrap_or_else(|| "default".to_string());
            let class = lower_class(&class_expr.class)?;
            Some(Statement::ClassDeclaration { name, class })
        }
        swc::DefaultDecl::TsInterfaceDecl(_) => None,
    }
}

/// Lower export { foo, bar } to exports.foo = foo; exports.bar = bar;
pub fn lower_export_named(named: &swc::NamedExport) -> Option<Statement> {
    let mut stmts = Vec::new();
    for spec in &named.specifiers {
        match spec {
            swc::ExportSpecifier::Named(named_spec) => {
                stmts.push(lower_named_export_specifier(named_spec));
            }
            swc::ExportSpecifier::Default(_) => {
                stmts.push(lower_default_export_specifier());
            }
            swc::ExportSpecifier::Namespace(ns) => {
                stmts.push(lower_namespace_export_specifier(ns));
            }
        }
    }
    
    if stmts.is_empty() {
        None
    } else if stmts.len() == 1 {
        Some(Statement::Export(Box::new(stmts.into_iter().next().unwrap())))
    } else {
        Some(Statement::Export(Box::new(Statement::Block(stmts))))
    }
}

fn lower_named_export_specifier(named_spec: &swc::ExportNamedSpecifier) -> Statement {
    let exported = named_spec.exported.as_ref()
        .map(|e| match e {
            swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
            swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
        })
        .unwrap_or_else(|| {
            match &named_spec.orig {
                swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
                swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
            }
        });
    let local = match &named_spec.orig {
        swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
        swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
    };
    Statement::Expression(Box::new(Expression::Assignment {
        left: Box::new(Expression::Member {
            object: Box::new(Expression::Identifier("exports".to_string())),
            property: PropertyKey::Ident(exported),
            computed: false,
        }),
        right: Box::new(Expression::Identifier(local)),
    }))
}

fn lower_default_export_specifier() -> Statement {
    Statement::Expression(Box::new(Expression::Assignment {
        left: Box::new(Expression::Member {
            object: Box::new(Expression::Identifier("exports".to_string())),
            property: PropertyKey::Ident("default".to_string()),
            computed: false,
        }),
        right: Box::new(Expression::Identifier("default".to_string())),
    }))
}

fn lower_namespace_export_specifier(ns: &swc::ExportNamespaceSpecifier) -> Statement {
    let name = match &ns.name {
        swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
        swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
    };
    Statement::Expression(Box::new(Expression::Assignment {
        left: Box::new(Expression::Member {
            object: Box::new(Expression::Identifier("exports".to_string())),
            property: PropertyKey::Ident(name.clone()),
            computed: false,
        }),
        right: Box::new(Expression::Identifier(name)),
    }))
}

// Re-export declaration lowering functions
pub use crate::lower::stmt::declarations::lower_decl;
pub use crate::lower::stmt::declarations::lower_class;
