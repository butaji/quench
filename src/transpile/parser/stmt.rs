//! Statement conversion

use crate::transpile::hir;
use oxc_ast::ast::*;

fn func_to_hir(func: &Function) -> hir::ModuleItem {
    hir::ModuleItem::Decl(hir::Decl::Function(hir::FunctionDecl {
        name: func
            .id
            .as_ref()
            .map(|i| i.name.to_string())
            .unwrap_or_default(),
        generics: vec![],
        params: vec![],
        return_type: None,
        body: None,
        is_async: func.r#async,
        is_generator: false,
        decorators: vec![],
    }))
}

fn import_to_hir(i: &ImportDeclaration) -> hir::ModuleItem {
    let specs: Vec<hir::ImportSpecifier> = i.specifiers.as_ref().map_or(vec![], |specs| {
        specs
            .iter()
            .map(|spec| match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => hir::ImportSpecifier::Named {
                    name: s.local.name.to_string(),
                    alias: None,
                },
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    hir::ImportSpecifier::Default {
                        name: s.local.name.to_string(),
                    }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    hir::ImportSpecifier::Namespace {
                        name: s.local.name.to_string(),
                    }
                }
            })
            .collect()
    });
    hir::ModuleItem::Import(hir::Import {
        source: i.source.value.to_string(),
        specifiers: specs,
        type_only: false,
    })
}

fn export_default_to_module(kind: &ExportDefaultDeclarationKind) -> Option<hir::ModuleItem> {
    match kind {
        ExportDefaultDeclarationKind::FunctionDeclaration(f) => Some(func_to_hir(f)),
        _ => None,
    }
}

pub fn convert_module_item(stmt: &Statement) -> Option<hir::ModuleItem> {
    match stmt {
        Statement::FunctionDeclaration(f) => Some(func_to_hir(f)),
        Statement::VariableDeclaration(_) => Some(hir::ModuleItem::Decl(hir::Decl::Variable(
            hir::VariableDecl {
                name: String::new(),
                kind: hir::VariableKind::Let,
                type_: None,
                init: None,
                pattern: None,
            },
        ))),
        Statement::ImportDeclaration(i) => Some(import_to_hir(i)),
        Statement::TSInterfaceDeclaration(i) => {
            Some(hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
                name: i.id.name.to_string(),
                generics: vec![],
                type_: hir::Type::Object { members: vec![] },
            })))
        }
        Statement::ExportDefaultDeclaration(e) => export_default_to_module(&e.declaration),
        _ => None,
    }
}

pub fn convert_stmt_to_stmt(_stmt: &Statement) -> Option<hir::Stmt> {
    None
}
