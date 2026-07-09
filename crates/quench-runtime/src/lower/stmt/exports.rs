//! Export and import lowering functions

use swc_ecma_ast as swc;
use crate::ast::Param;
use crate::ast::{Expression, PropertyKey, Statement};
use crate::lower::expr::lower_expr;
use crate::lower::helpers::{atom_to_string, wtf8_atom_to_string};

use super::declarations::lower_param_decl;
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
        // export { foo, bar } or export { foo } from 'module'
        swc::ModuleDecl::ExportNamed(named) => {
            lower_export_named(named)
        }
        // export * from 'module' - re-export all
        swc::ModuleDecl::ExportAll(export_all) => {
            // Lower to: import * as _re_export from 'module'; Object.assign(exports, _re_export);
            let source = wtf8_atom_to_string(&export_all.src.value);
            Some(lower_export_star_from(&source))
        }
        // import statements
        swc::ModuleDecl::Import(import) => {
            lower_import(import)
        }
        // TypeScript export =
        swc::ModuleDecl::TsExportAssignment(_) => None,
        // TypeScript import =
        swc::ModuleDecl::TsImportEquals(_) => None,
        // TypeScript namespace export
        swc::ModuleDecl::TsNamespaceExport(_) => None,
    }
}

/// Lower an import statement to our Import statement
fn lower_import(import: &swc::ImportDecl) -> Option<Statement> {
    let source = wtf8_atom_to_string(&import.src.value);
    let default = import.specifiers.iter().find_map(|spec| {
        if let swc::ImportSpecifier::Default(default_spec) = spec {
            Some(atom_to_string(&default_spec.local.sym))
        } else {
            None
        }
    });
    let namespace = import.specifiers.iter().find_map(|spec| {
        if let swc::ImportSpecifier::Namespace(ns_spec) = spec {
            Some(atom_to_string(&ns_spec.local.sym))
        } else {
            None
        }
    });
    let named: Vec<(String, String)> = import.specifiers.iter().filter_map(|spec| {
        if let swc::ImportSpecifier::Named(named_spec) = spec {
            let local = atom_to_string(&named_spec.local.sym);
            let imported = named_spec.imported.as_ref()
                .map(|i| match i {
                    swc::ModuleExportName::Ident(ident) => atom_to_string(&ident.sym),
                    swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
                })
                .unwrap_or_else(|| local.clone());
            Some((local, imported))
        } else {
            None
        }
    }).collect();
    Some(Statement::Import { default, named, namespace, source })
}

/// Lower `export * from 'module'` to re-export all bindings
/// Creates: import * as ns from 'src'; for (let k in ns) exports[k] = ns[k];
fn lower_export_star_from(source: &str) -> Statement {
    let unique_name = format!("_star_{}", source.replace(['/', '-', '.'], "_"));
    let import_stmt = Statement::Import {
        default: None,
        named: vec![],
        namespace: Some(unique_name.clone()),
        source: source.to_string(),
    };
    // Lower the for-in loop manually using statements
    // for (let k in ns) exports[k] = ns[k];
    // This becomes a ForIn statement with a body that does the assignment
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

fn lower_export_default_decl(export_decl: &swc::ExportDefaultDecl) -> Option<Statement> {
    match &export_decl.decl {
        swc::DefaultDecl::Fn(func_expr) => {
            let name = func_expr.ident.as_ref()
                .map(|i| i.sym.to_string())
                .unwrap_or_else(|| "default".to_string());
            let params: Vec<Param> = func_expr.function.params.iter().map(|p| lower_param_decl(&p.pat)).collect();
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
/// Lower export { foo } from 'module' to imports.foo; exports.foo = foo;
pub fn lower_export_named(named: &swc::NamedExport) -> Option<Statement> {
    // Handle export-from syntax: export { x } from 'module'
    if let Some(src) = &named.src {
        return lower_export_from(named, src);
    }
    
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

/// Lower `export { x } from 'module'` to import and re-export
fn lower_export_from(named: &swc::NamedExport, src: &swc::Str) -> Option<Statement> {
    let source = wtf8_atom_to_string(&src.value);
    let unique_name = format!("_re_export_src_{}", source.replace(['/', '-', '.'], "_"));
    
    let mut stmts = Vec::new();
    let mut imports = Vec::new();
    
    for spec in &named.specifiers {
        match spec {
            swc::ExportSpecifier::Named(named_spec) => {
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
                imports.push((local.clone(), local.clone()));
                stmts.push(Statement::Expression(Box::new(Expression::Assignment {
                    left: Box::new(Expression::Member {
                        object: Box::new(Expression::Identifier("exports".to_string())),
                        property: PropertyKey::Ident(exported),
                        computed: false,
                    }),
                    right: Box::new(Expression::Identifier(local)),
                })));
            }
            swc::ExportSpecifier::Default(_) => {
                imports.push(("default".to_string(), "default".to_string()));
                stmts.push(Statement::Expression(Box::new(Expression::Assignment {
                    left: Box::new(Expression::Member {
                        object: Box::new(Expression::Identifier("exports".to_string())),
                        property: PropertyKey::Ident("default".to_string()),
                        computed: false,
                    }),
                    right: Box::new(Expression::Identifier("default".to_string())),
                })));
            }
            swc::ExportSpecifier::Namespace(ns) => {
                let name = match &ns.name {
                    swc::ModuleExportName::Ident(i) => atom_to_string(&i.sym),
                    swc::ModuleExportName::Str(s) => wtf8_atom_to_string(&s.value),
                };
                stmts.push(Statement::Expression(Box::new(Expression::Assignment {
                    left: Box::new(Expression::Member {
                        object: Box::new(Expression::Identifier("exports".to_string())),
                        property: PropertyKey::Ident(name.clone()),
                        computed: false,
                    }),
                    right: Box::new(Expression::Identifier(name)),
                })));
            }
        }
    }
    
    // Add import statement for the source module
    let import_stmt = Statement::Import {
        default: None,
        named: imports,
        namespace: None,
        source,
    };
    
    let mut all_stmts = vec![import_stmt];
    all_stmts.extend(stmts);
    
    Some(Statement::Block(all_stmts))
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
