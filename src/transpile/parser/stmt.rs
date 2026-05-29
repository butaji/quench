//! Statement conversion

use crate::transpile::hir;
use crate::transpile::parser::expr::convert_expr;
use oxc_ast::ast::*;

fn var_to_decl(v: &VariableDeclaration) -> hir::Decl {
    let decl = v.declarations.first();
    let name = decl.and_then(|d| match &d.id {
        BindingPattern::BindingIdentifier(i) => Some(i.name.to_string()),
        _ => None,
    }).unwrap_or_default();
    let init = decl.and_then(|d| d.init.as_ref().and_then(|e| convert_expr(e)));
    hir::Decl::Variable(hir::VariableDecl { name, kind: hir::VariableKind::Const, type_: None, init, pattern: None })
}

fn func_to_decl(f: &Function) -> hir::Decl {
    hir::Decl::Function(hir::FunctionDecl {
        name: f.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(),
        generics: vec![],
        params: vec![],
        return_type: None,
        body: None,
        is_async: f.r#async,
        is_generator: false,
        decorators: vec![],
    })
}

fn import_to_hir(i: &ImportDeclaration) -> hir::ModuleItem {
    let specs = i.specifiers.as_ref().map_or(vec![], |v| v.iter().map(|s| match s {
        ImportDeclarationSpecifier::ImportSpecifier(s) => hir::ImportSpecifier::Named { name: s.local.name.to_string(), alias: None },
        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => hir::ImportSpecifier::Default { name: s.local.name.to_string() },
        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => hir::ImportSpecifier::Namespace { name: s.local.name.to_string() },
    }).collect());
    hir::ModuleItem::Import(hir::Import { source: i.source.value.to_string(), specifiers: specs, type_only: false })
}

pub fn convert_module_item(stmt: &Statement) -> Option<hir::ModuleItem> {
    match stmt {
        Statement::FunctionDeclaration(f) => Some(hir::ModuleItem::Decl(func_to_decl(f))),
        Statement::VariableDeclaration(v) => Some(hir::ModuleItem::Decl(var_to_decl(v))),
        Statement::ImportDeclaration(i) => Some(import_to_hir(i)),
        Statement::TSInterfaceDeclaration(i) => Some(hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl { name: i.id.name.to_string(), generics: vec![], type_: hir::Type::Object { members: vec![] } }))),
        _ => None,
    }
}
