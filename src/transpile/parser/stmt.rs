//! Statement conversion - consolidated implementations
//!
//! These functions are consolidated into this single file
//! to avoid module declaration issues.

use crate::transpile::hir::{self, Expr, Stmt, VariableKind, ForInit, SwitchCase};
use super::expr::{convert_expr, convert_binding_pattern};
use oxc_ast::ast::*;

/// Convert statement to HIR
pub fn convert_statement(s: &Statement) -> Option<hir::ModuleItem> {
    match s {
        Statement::ImportDeclaration(i) => Some(hir::ModuleItem::Import(import_to_hir(i))),
        Statement::ExportNamedDeclaration(e) => convert_export_named(e),
        Statement::ExportDefaultDeclaration(e) => {
            let expr = export_default_kind_to_expr(&e.declaration)?;
            Some(hir::ModuleItem::Stmt(hir::Stmt::Return { arg: Some(expr) }))
        }
        Statement::ExportAllDeclaration(_) => None,
        Statement::TSExportAssignment(_) => None,
        Statement::TSNamespaceExportDeclaration(_) => None,
        Statement::FunctionDeclaration(f) => {
            let decl = func_to_decl(f)?;
            Some(hir::ModuleItem::Decl(hir::Decl::Function(decl)))
        }
        Statement::ClassDeclaration(c) => {
            let decl = class_to_hir(c)?;
            Some(hir::ModuleItem::Decl(decl))
        }
        _ => Some(hir::ModuleItem::Stmt(stmt_to_hir_stmt(s).ok()?)),
    }
}

fn export_default_kind_to_expr(kind: &ExportDefaultDeclarationKind) -> Option<Expr> {
    match kind {
        ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
            let decl = func_to_decl(f)?;
            Some(Expr::Function(decl))
        }
        ExportDefaultDeclarationKind::ClassDeclaration(_) => {
            None
        }
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => None,
        _ => {
            let expr = kind.as_expression()?;
            convert_expr(expr).ok()
        }
    }
}

fn convert_export_named(e: &ExportNamedDeclaration) -> Option<hir::ModuleItem> {
    if let Some(source) = &e.source {
        let names: Vec<String> = e.specifiers.iter().map(|s| {
            match &s.local {
                ModuleExportName::IdentifierName(i) => i.name.to_string(),
                ModuleExportName::IdentifierReference(i) => i.name.to_string(),
                ModuleExportName::StringLiteral(s) => s.value.to_string(),
            }
        }).collect();
        Some(hir::ModuleItem::Export(hir::Export::ReExport {
            source: source.value.to_string(),
            names,
        }))
    } else {
        let specifiers: Vec<hir::Export> = e.specifiers.iter().map(|s| {
            let local = match &s.local {
                ModuleExportName::IdentifierName(i) => i.name.to_string(),
                ModuleExportName::IdentifierReference(i) => i.name.to_string(),
                ModuleExportName::StringLiteral(s) => s.value.to_string(),
            };
            let exported = match &s.exported {
                ModuleExportName::IdentifierName(i) => i.name.to_string(),
                ModuleExportName::IdentifierReference(i) => i.name.to_string(),
                ModuleExportName::StringLiteral(s) => s.value.to_string(),
            };
            if local == exported {
                hir::Export::Named { name: local }
            } else {
                hir::Export::NamedRenamed { local, exported }
            }
        }).collect();
        Some(hir::ModuleItem::Stmt(hir::Stmt::ExportNamed { specifiers }))
    }
}

/// Convert statement to HIR
pub fn stmt_to_hir_stmt(s: &Statement) -> Result<Stmt, ()> {
    match s {
        Statement::ExpressionStatement(e) => Ok(hir::Stmt::Expr {
            expr: convert_expr(&e.expression).map_err(|_| ())?,
        }),
        Statement::IfStatement(s) => Ok(hir::Stmt::If {
            test: convert_expr(&s.test).map_err(|_| ())?,
            consequent: Box::new(stmt_to_hir_stmt(&s.consequent)?),
            alternate: match &s.alternate {
                Some(a) => Some(Box::new(stmt_to_hir_stmt(a)?)),
                None => None,
            },
        }),
        Statement::BlockStatement(b) => Ok(hir::Stmt::Block {
            stmts: b.body.iter().filter_map(|s| stmt_to_hir_stmt(s).ok()).collect(),
        }),
        Statement::ReturnStatement(r) => Ok(hir::Stmt::Return {
            arg: r.argument.as_ref().and_then(|a| convert_expr(a).ok()),
        }),
        Statement::SwitchStatement(s) => {
            let discriminant = convert_expr(&s.discriminant).map_err(|_| ())?;
            let cases: Vec<SwitchCase> = s.cases.iter().map(|c| {
                let test = c.test.as_ref().and_then(|t| convert_expr(t).ok());
                let consequent: Vec<Stmt> = c.consequent.iter().filter_map(|s| stmt_to_hir_stmt(s).ok()).collect();
                SwitchCase { test, consequent }
            }).collect();
            Ok(hir::Stmt::Switch { discriminant, cases })
        }
        Statement::TryStatement(t) => {
            let block = hir::Block(t.block.body.iter().filter_map(|s| stmt_to_hir_stmt(s).ok()).collect());
            let handler = t.handler.as_ref().map(|h| {
                hir::CatchClause {
                    param: match &h.param {
                        Some(p) => match &p.pattern {
                            BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                            _ => String::new(),
                        },
                        None => String::new(),
                    },
                    body: Box::new(hir::Block(h.body.body.iter().filter_map(|s| stmt_to_hir_stmt(s).ok()).collect())),
                }
            });
            Ok(hir::Stmt::Try { block, handler, finalizer: None })
        }
        Statement::ThrowStatement(t) => Ok(hir::Stmt::Throw {
            arg: convert_expr(&t.argument).map_err(|_| ())?,
        }),
        Statement::BreakStatement(_) => Ok(hir::Stmt::Break { label: None }),
        Statement::ContinueStatement(_) => Ok(hir::Stmt::Continue { label: None }),
        Statement::LabeledStatement(l) => Ok(hir::Stmt::Labeled {
            label: l.label.name.to_string(),
            body: Box::new(stmt_to_hir_stmt(&l.body)?),
        }),
        Statement::WhileStatement(w) => Ok(hir::Stmt::While {
            test: convert_expr(&w.test).map_err(|_| ())?,
            body: Box::new(stmt_to_hir_stmt(&w.body)?),
        }),
        Statement::DoWhileStatement(d) => Ok(hir::Stmt::DoWhile {
            body: Box::new(stmt_to_hir_stmt(&d.body)?),
            test: convert_expr(&d.test).map_err(|_| ())?,
        }),
        Statement::ForStatement(f) => {
            let init = f.init.as_ref().map(|i| match i {
                ForStatementInit::VariableDeclaration(v) => {
                    let kind = match v.kind {
                        VariableDeclarationKind::Const => VariableKind::Const,
                        VariableDeclarationKind::Let => VariableKind::Let,
                        VariableDeclarationKind::Var => VariableKind::Var,
                        _ => VariableKind::Let,
                    };
                    let vars: Vec<(String, Option<Expr>)> = v.declarations.iter().filter_map(|d| {
                        let name = match &d.id {
                            BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                            _ => String::new(),
                        };
                        let init = d.init.as_ref().and_then(|e| convert_expr(e).ok());
                        Some((name, init))
                    }).collect();
                    ForInit::Variable(kind, vars)
                }
                _ => {
                    match i.as_expression() {
                        Some(expr) => match convert_expr(expr) {
                            Ok(e) => ForInit::Expr(Box::new(e)),
                            Err(_) => ForInit::Variable(VariableKind::Let, vec![]),
                        },
                        None => ForInit::Variable(VariableKind::Let, vec![]),
                    }
                }
            });
            let test = f.test.as_ref().and_then(|t| convert_expr(t).ok());
            let update = f.update.as_ref().and_then(|u| convert_expr(u).ok());
            let body = Box::new(stmt_to_hir_stmt(&f.body)?);
            Ok(hir::Stmt::For { init, test, update, body })
        }
        Statement::ForInStatement(f) => {
            let left = match &f.left {
                ForStatementLeft::VariableDeclaration(v) => {
                    let kind = match v.kind {
                        VariableDeclarationKind::Const => VariableKind::Const,
                        VariableDeclarationKind::Let => VariableKind::Let,
                        VariableDeclarationKind::Var => VariableKind::Var,
                        _ => VariableKind::Let,
                    };
                    let vars: Vec<(String, Option<Expr>)> = v.declarations.iter().filter_map(|d| {
                        let name = match &d.id {
                            BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                            _ => String::new(),
                        };
                        Some((name, None))
                    }).collect();
                    ForInit::Variable(kind, vars)
                }
                ForStatementLeft::AssignmentTargetIdentifier(id) => {
                    ForInit::Expr(Box::new(hir::Expr::Ident { name: id.name.to_string() }))
                }
                _ => ForInit::Variable(VariableKind::Let, vec![]),
            };
            let right = convert_expr(&f.right).map_err(|_| ())?;
            let body = Box::new(stmt_to_hir_stmt(&f.body)?);
            Ok(hir::Stmt::ForIn { left, right, body })
        }
        Statement::ForOfStatement(f) => {
            let left = match &f.left {
                ForStatementLeft::VariableDeclaration(v) => {
                    let kind = match v.kind {
                        VariableDeclarationKind::Const => VariableKind::Const,
                        VariableDeclarationKind::Let => VariableKind::Let,
                        VariableDeclarationKind::Var => VariableKind::Var,
                        _ => VariableKind::Let,
                    };
                    let vars: Vec<(String, Option<Expr>)> = v.declarations.iter().filter_map(|d| {
                        let name = match &d.id {
                            BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                            _ => String::new(),
                        };
                        Some((name, None))
                    }).collect();
                    ForInit::Variable(kind, vars)
                }
                ForStatementLeft::AssignmentTargetIdentifier(id) => {
                    ForInit::Expr(Box::new(hir::Expr::Ident { name: id.name.to_string() }))
                }
                _ => ForInit::Variable(VariableKind::Let, vec![]),
            };
            let right = convert_expr(&f.right).map_err(|_| ())?;
            let body = Box::new(stmt_to_hir_stmt(&f.body)?);
            Ok(hir::Stmt::ForOf { left, right, body, is_await: f.r#await })
        }
        Statement::VariableDeclaration(v) => {
            let kind = match v.kind {
                VariableDeclarationKind::Const => VariableKind::Const,
                VariableDeclarationKind::Let => VariableKind::Let,
                VariableDeclarationKind::Var => VariableKind::Var,
                _ => VariableKind::Let,
            };
            if let Some(decl) = v.declarations.first() {
                let name = match &decl.id {
                    BindingPattern::BindingIdentifier(i) => i.name.to_string(),
                    _ => String::new(),
                };
                let init = decl.init.as_ref().and_then(|e| convert_expr(e).ok());
                let pattern = convert_binding_pattern(&decl.id);
                Ok(hir::Stmt::Variable(hir::VariableDecl {
                    name,
                    kind,
                    type_: None,
                    init,
                    pattern,
                }))
            } else {
                Ok(hir::Stmt::Empty)
            }
        }
        Statement::FunctionDeclaration(f) => {
            let decl = func_to_decl(f).ok_or(())?;
            Ok(hir::Stmt::FunctionDecl(decl))
        }
        Statement::ClassDeclaration(c) => {
            let decl = match class_to_hir(c) {
                Some(hir::Decl::Class(cd)) => cd,
                _ => return Ok(hir::Stmt::Empty),
            };
            Ok(hir::Stmt::Class(decl))
        }
        _ => Ok(hir::Stmt::Empty),
    }
}

fn func_to_decl(f: &Function) -> Option<hir::FunctionDecl> {
    let params: Vec<hir::Param> = f.params.items.iter().filter_map(|p| {
        let name = match &p.pattern {
            BindingPattern::BindingIdentifier(i) => i.name.to_string(),
            _ => String::new(),
        };
        let pat = convert_binding_pattern(&p.pattern);
        Some(hir::Param {
            name,
            type_: None,
            default: None,
            optional: false,
            pattern: pat,
            ownership: hir::Ownership::Owned,
        })
    }).collect();
    let body = f.body.as_ref().map(|b| {
        hir::Block(b.statements.iter().filter_map(|s| stmt_to_hir_stmt(s).ok()).collect())
    });
    Some(hir::FunctionDecl {
        name: f.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(),
        generics: vec![],
        params,
        return_type: None,
        body,
        is_async: f.r#async,
        is_generator: f.generator,
        decorators: vec![],
        throws: false,
        error_type: None,
    })
}

fn class_to_hir(c: &Class) -> Option<hir::Decl> {
    Some(hir::Decl::Class(hir::ClassDecl {
        name: c.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(),
        extends: None,
        members: vec![],
        generics: vec![],
        methods: vec![],
    }))
}

fn import_to_hir(i: &ImportDeclaration) -> hir::Import {
    let specs = i.specifiers.as_ref().map_or(vec![], |v| {
        v.iter()
            .map(|s| match s {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    let imported_name = match &s.imported {
                        ModuleExportName::IdentifierName(i) => i.name.to_string(),
                        ModuleExportName::IdentifierReference(i) => i.name.to_string(),
                        ModuleExportName::StringLiteral(s) => s.value.to_string(),
                    };
                    let local_name = s.local.name.to_string();
                    let alias = if imported_name == local_name { None } else { Some(local_name) };
                    hir::ImportSpecifier::Named { name: imported_name, alias }
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    hir::ImportSpecifier::Default { name: s.local.name.to_string() }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    hir::ImportSpecifier::Namespace { name: s.local.name.to_string() }
                }
            })
            .collect()
    });
    hir::Import {
        source: i.source.value.to_string(),
        specifiers: specs,
        type_only: false,
    }
}
