//! Statement declarations - var_to_decl, func_to_decl, import_to_hir

use crate::transpile::hir;
use crate::transpile::parser::expr::{convert_binding_pattern, convert_expr};

use oxc_ast::ast::*;

pub fn var_to_decl(v: &VariableDeclaration) -> Vec<hir::Decl> {
    let kind = match v.kind {
        VariableDeclarationKind::Const => hir::VariableKind::Const,
        VariableDeclarationKind::Let => hir::VariableKind::Let,
        VariableDeclarationKind::Var => hir::VariableKind::Var,
        VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => hir::VariableKind::Var,
    };
    v.declarations
        .iter()
        .filter_map(|decl| {
            let (name, pattern) = match &decl.id {
                BindingPattern::BindingIdentifier(i) => (i.name.to_string(), None),
                BindingPattern::ArrayPattern(_) | BindingPattern::ObjectPattern(_) | BindingPattern::AssignmentPattern(_) => {
                    let pat = convert_binding_pattern(&decl.id)?;
                    (String::new(), Some(pat))
                }
            };
            let init = decl.init.as_ref().and_then(|e| convert_expr(e).ok());
            Some(hir::Decl::Variable(hir::VariableDecl {
                name,
                kind: kind.clone(),
                type_: None,
                init,
                pattern,
            }))
        })
        .collect()
}
