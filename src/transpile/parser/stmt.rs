//! Statement conversion

use crate::transpile::hir as hir;
use crate::transpile::parser::expr::convert_expr;
use crate::transpile::parser::types::{convert_ts_type, extract_binding_name, convert_param};

use oxc_ast::ast::*;

pub fn convert_module_item(stmt: &Statement) -> Option<hir::ModuleItem> {
    match stmt {
        Statement::FunctionDeclaration(func) => convert_function_decl(func),
        Statement::VariableDeclaration(var_decl) => convert_var_decl_module(var_decl),
        Statement::ImportDeclaration(decl) => convert_import(decl),
        Statement::ExportNamedDeclaration(decl) => convert_export_named(decl),
        Statement::ExportDefaultDeclaration(_) => None,
        Statement::TSTypeAliasDeclaration(alias) => convert_type_alias(alias),
        Statement::TSInterfaceDeclaration(iface) => convert_interface(iface),
        Statement::ClassDeclaration(cls) => convert_class_decl(cls),
        _ => None,
    }
}

fn convert_function_decl(func: &Function) -> Option<hir::ModuleItem> {
    let name = func.id.as_ref()?.name.to_string();
    let params: Vec<_> = func.params.items.iter().map(convert_param).collect();
    let body = func.body.as_ref()
        .map(|b| hir::Block(b.statements.iter().filter_map(convert_stmt_to_stmt).collect()))
        .unwrap_or_default();
    let return_type = func.return_type.as_ref()
        .map(|t| convert_ts_type(&t.type_annotation));
    let decl = hir::FunctionDecl {
        name, generics: vec![], params, return_type,
        body: Some(body), is_async: func.r#async,
        is_generator: func.generator, decorators: vec![],
    };
    Some(hir::ModuleItem::Decl(hir::Decl::Function(decl)))
}

fn convert_var_decl_module(var_decl: &VariableDeclaration) -> Option<hir::ModuleItem> {
    if let Some(decl) = var_decl.declarations.first() {
        let name = extract_binding_name(&decl.id);
        let init = decl.init.as_ref().and_then(convert_expr);
        let kind = match var_decl.kind {
            VariableDeclarationKind::Const => hir::VariableKind::Const,
            VariableDeclarationKind::Let => hir::VariableKind::Let,
            _ => hir::VariableKind::Var,
        };
        return Some(hir::ModuleItem::Decl(hir::Decl::Variable(hir::VariableDecl {
            name, kind, type_: None, init, pattern: None,
        })));
    }
    None
}

fn convert_import(decl: &ImportDeclaration) -> Option<hir::ModuleItem> {
    let src = decl.source.value.to_string();
    let specs: Vec<_> = decl.specifiers.as_ref().map_or(vec![], |specs| {
        specs.iter().filter_map(|s| match s {
            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                Some(hir::ImportSpecifier::Named { name: s.local.name.to_string(), alias: None })
            }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                Some(hir::ImportSpecifier::Default { name: s.local.name.to_string() })
            }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                Some(hir::ImportSpecifier::Namespace { name: s.local.name.to_string() })
            },
        }).collect()
    });
    let type_only = matches!(decl.import_kind, ImportOrExportKind::Type);
    Some(hir::ModuleItem::Import(hir::Import { source: src, specifiers: specs, type_only }))
}

fn convert_export_named(decl: &ExportNamedDeclaration) -> Option<hir::ModuleItem> {
    if let Some(d) = &decl.declaration {
        match d {
            Declaration::FunctionDeclaration(func) => {
                let name = func.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default();
                let params: Vec<_> = func.params.items.iter().map(convert_param).collect();
                let body = func.body.as_ref()
                    .map(|b| hir::Block(b.statements.iter().filter_map(convert_stmt_to_stmt).collect()))
                    .unwrap_or_default();
                let return_type = func.return_type.as_ref()
                    .map(|t| convert_ts_type(&t.type_annotation));
                let decl = hir::FunctionDecl {
                    name: name.clone(), generics: vec![], params, return_type,
                    body: Some(body), is_async: func.r#async,
                    is_generator: func.generator, decorators: vec![],
                };
                return Some(hir::ModuleItem::Export(hir::Export::NamedWithValue {
                    name,
                    value: hir::Expr::Function { decl },
                }));
            }
            Declaration::VariableDeclaration(var) => {
                if let Some(v) = var.declarations.first() {
                    let name = extract_binding_name(&v.id);
                    let init = v.init.as_ref().and_then(convert_expr);
                    let kind = match var.kind {
                        VariableDeclarationKind::Const => hir::VariableKind::Const,
                        VariableDeclarationKind::Let => hir::VariableKind::Let,
                        _ => hir::VariableKind::Var,
                    };
                    return Some(hir::ModuleItem::Decl(hir::Decl::Variable(hir::VariableDecl {
                        name, kind, type_: None, init, pattern: None,
                    })));
                }
            }
            _ => {}
        }
    }
    None
}

fn convert_type_alias(alias: &TSTypeAliasDeclaration) -> Option<hir::ModuleItem> {
    let name = alias.id.name.to_string();
    let type_ = convert_ts_type(&alias.type_annotation);
    Some(hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
        name, generics: vec![], type_,
    })))
}

fn convert_interface(iface: &TSInterfaceDeclaration) -> Option<hir::ModuleItem> {
    let name = iface.id.name.to_string();
    let members: Vec<_> = iface.body.body.iter().filter_map(|m| {
        if let TSSignature::TSPropertySignature(p) = m {
            let p = p.as_ref();
            Some(hir::ObjectMember {
                key: p.key.name().map(|n| n.to_string()).unwrap_or_default(),
                optional: p.optional,
                readonly: p.readonly,
                type_: p.type_annotation.as_ref()
                    .map(|t| convert_ts_type(&t.type_annotation))
                    .unwrap_or(hir::Type::Unknown),
            })
        } else { None }
    }).collect();
    Some(hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
        name, generics: vec![], type_: hir::Type::Object { members },
    })))
}

fn convert_class_decl(cls: &Class) -> Option<hir::ModuleItem> {
    let name = cls.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default();
    let extends = cls.super_class.as_ref().and_then(|e| {
        if let Expression::Identifier(id) = e {
            Some(hir::Type::Ref { name: id.name.to_string(), generics: vec![] })
        } else { None }
    });
    Some(hir::ModuleItem::Decl(hir::Decl::Class(hir::ClassDecl {
        name, extends, implements: vec![], members: vec![],
    })))
}

pub fn convert_stmt_to_stmt(stmt: &Statement) -> Option<hir::Stmt> {
    use oxc_ast::ast::*;

    match stmt {
        Statement::ExpressionStatement(expr_stmt) => {
            convert_expr(&expr_stmt.expression).map(|expr| hir::Stmt::Expr { expr })
        }
        Statement::ReturnStatement(ret) => {
            Some(hir::Stmt::Return { arg: ret.argument.as_ref().and_then(convert_expr) })
        }
        Statement::VariableDeclaration(var_decl) => {
            if let Some(decl) = var_decl.declarations.first() {
                let name = extract_binding_name(&decl.id);
                let init = decl.init.as_ref().and_then(convert_expr);
                return Some(hir::Stmt::Variable { decl: hir::VariableDecl {
                    name,
                    kind: match var_decl.kind {
                        VariableDeclarationKind::Const => hir::VariableKind::Const,
                        VariableDeclarationKind::Let => hir::VariableKind::Let,
                        _ => hir::VariableKind::Var,
                    },
                    type_: None, init, pattern: None,
                }});
            }
            Some(hir::Stmt::Empty)
        }
        Statement::BlockStatement(block) => {
            Some(hir::Stmt::Block(
                block.body.iter().filter_map(convert_stmt_to_stmt).collect()
            ))
        }
        Statement::BreakStatement(_) => Some(hir::Stmt::Break { label: None }),
        Statement::ContinueStatement(_) => Some(hir::Stmt::Continue { label: None }),
        Statement::EmptyStatement(_) => None,
        Statement::FunctionDeclaration(func) => {
            let name = func.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default();
            let params: Vec<_> = func.params.items.iter().map(convert_param).collect();
            let body = func.body.as_ref()
                .map(|b| hir::Block(b.statements.iter().filter_map(convert_stmt_to_stmt).collect()))
                .unwrap_or_default();
            let return_type = func.return_type.as_ref()
                .map(|t| convert_ts_type(&t.type_annotation));
            Some(hir::Stmt::Function { decl: hir::FunctionDecl {
                name, generics: vec![], params, return_type,
                body: Some(body), is_async: func.r#async,
                is_generator: func.generator, decorators: vec![],
            }})
        }
        _ => None,
    }
}
