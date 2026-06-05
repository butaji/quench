//! Statement declarations - var_to_decl, func_to_decl, import_to_hir

use crate::transpile::hir;
use crate::transpile::parser::expr::{convert_binding_pattern, convert_expr, conv_arrow};

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

pub fn func_to_decl(f: &Function) -> hir::Decl {
    let params = convert_func_params(f);
    let body = f.body.as_ref().map(|body_box| {
        hir::Block(body_box.statements.iter().map(stmt_to_hir_stmt).collect())
    });
    hir::Decl::Function(hir::FunctionDecl {
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

fn convert_func_params(f: &Function) -> Vec<hir::Param> {
    let mut params: Vec<hir::Param> = f.params.items.iter().filter_map(convert_param).collect();
    if let Some(rest) = f.params.rest.as_ref() {
        if let BindingPattern::BindingIdentifier(binding_id) = &rest.rest.argument {
            params.push(make_rest_param(&binding_id.name));
        }
    }
    params
}

fn convert_param(p: &FormalParameter) -> Option<hir::Param> {
    if let BindingPattern::BindingIdentifier(i) = &p.pattern {
        return Some(make_param(&i.name, p.optional, None));
    }
    let pattern = convert_binding_pattern(&p.pattern)?;
    Some(make_param("", p.optional, Some(pattern)))
}

fn make_param(name: &str, optional: bool, pattern: Option<hir::Pat>) -> hir::Param {
    hir::Param { name: name.to_string(), type_: None, default: None, optional, pattern, ownership: hir::Ownership::Owned }
}

fn make_rest_param(name: &str) -> hir::Param {
    hir::Param {
        name: name.to_string(),
        type_: None,
        default: None,
        optional: false,
        pattern: Some(hir::Pat::Rest { arg: Box::new(hir::Pat::Ident { name: name.to_string(), type_: None }) }),
        ownership: hir::Ownership::Owned,
    }
}

pub fn import_to_hir(i: &ImportDeclaration) -> hir::ModuleItem {
    let specs = i.specifiers.as_ref().map_or(vec![], |v| {
        v.iter()
            .map(|s| match s {
                oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    let imported_name = match &s.imported {
                        ModuleExportName::IdentifierName(i) => i.name.to_string(),
                        ModuleExportName::IdentifierReference(i) => i.name.to_string(),
                        ModuleExportName::StringLiteral(s) => s.value.to_string(),
                    };
                    let local_name = s.local.name.to_string();
                    let alias = if imported_name == local_name { None } else { Some(local_name) };
                    hir::ImportSpecifier::Named { name: imported_name, alias }
                }
                oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    hir::ImportSpecifier::Default { name: s.local.name.to_string() }
                }
                oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    hir::ImportSpecifier::Namespace { name: s.local.name.to_string() }
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

// Re-export for use by other modules
pub use super::stmt_convert::stmt_to_hir_stmt;
