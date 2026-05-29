//! Statement conversion

use crate::transpile::hir;
use oxc_ast::ast::*;

fn var_decl_to_hir(v: &VariableDeclaration) -> Option<hir::VariableDecl> {
    let decl = v.declarations.first()?;
    let name = match &decl.id {
        BindingPattern::BindingIdentifier(i) => i.name.to_string(),
        _ => return None,
    };
    let kind = match v.kind {
        VariableDeclarationKind::Const => hir::VariableKind::Const,
        VariableDeclarationKind::Let => hir::VariableKind::Let,
        _ => hir::VariableKind::Var,
    };
    let init = decl.init.as_ref().map(|_| hir::Expr::Null);
    Some(hir::VariableDecl { name, kind, type_: None, init, pattern: None })
}

pub fn convert_module_item(stmt: &Statement) -> Option<hir::ModuleItem> {
    match stmt {
        Statement::FunctionDeclaration(f) => Some(hir::ModuleItem::Decl(hir::Decl::Function(hir::FunctionDecl { name: f.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(), generics: vec![], params: vec![], return_type: None, body: None, is_async: f.r#async, is_generator: false, decorators: vec![] }))),
        Statement::VariableDeclaration(v) => var_decl_to_hir(v).map(|v| hir::ModuleItem::Decl(hir::Decl::Variable(v))),
        Statement::ImportDeclaration(i) => Some(hir::ModuleItem::Import(hir::Import { source: i.source.value.to_string(), specifiers: vec![], type_only: false })),
        Statement::ExportDefaultDeclaration(e) => match &e.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(f) => Some(hir::ModuleItem::Decl(hir::Decl::Function(hir::FunctionDecl { name: f.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(), generics: vec![], params: vec![], return_type: None, body: None, is_async: f.r#async, is_generator: false, decorators: vec![] }))),
            _ => None,
        },
        _ => None,
    }
}
