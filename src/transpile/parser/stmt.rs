//! Statement conversion

use crate::transpile::hir;
use oxc_ast::ast::*;

pub fn convert_module_item(stmt: &Statement) -> Option<hir::ModuleItem> {
    use Statement::*;
    match stmt {
        FunctionDeclaration(func) => {
            let name = func.id.as_ref()?.name.to_string();
            Some(hir::ModuleItem::Decl(hir::Decl::Function(
                hir::FunctionDecl {
                    name,
                    generics: vec![],
                    params: vec![],
                    return_type: None,
                    body: None,
                    is_async: func.r#async,
                    is_generator: false,
                    decorators: vec![],
                },
            )))
        }
        VariableDeclaration(v) => Some(hir::ModuleItem::Decl(hir::Decl::Variable(
            hir::VariableDecl {
                name: String::new(),
                kind: hir::VariableKind::Let,
                type_: None,
                init: None,
                pattern: None,
            },
        ))),
        ImportDeclaration(i) => Some(hir::ModuleItem::Import(hir::Import {
            source: i.source.value.to_string(),
            specifiers: vec![],
            type_only: false,
        })),
        _ => None,
    }
}
pub fn convert_stmt_to_stmt(_stmt: &Statement) -> Option<hir::Stmt> {
    None
}
