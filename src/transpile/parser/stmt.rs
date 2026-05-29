//! Statement conversion
// allow:complexity

use crate::transpile::hir;
use crate::transpile::parser::expr::convert_expr;
use oxc_ast::ast::*;

fn var_to_decl(v: &VariableDeclaration) -> hir::Decl {
    let decl = match v.declarations.first() {
        Some(d) => d,
        None => {
            return hir::Decl::Variable(hir::VariableDecl {
                name: String::new(),
                kind: hir::VariableKind::Const,
                type_: None,
                init: None,
                pattern: None,
            })
        }
    };
    let name = match &decl.id {
        BindingPattern::BindingIdentifier(i) => i.name.to_string(),
        _ => String::new(),
    };
    let init = decl.init.as_ref().and_then(convert_expr);
    hir::Decl::Variable(hir::VariableDecl {
        name,
        kind: hir::VariableKind::Const,
        type_: None,
        init,
        pattern: None,
    })
}

fn func_to_decl(f: &Function) -> hir::Decl {
    hir::Decl::Function(hir::FunctionDecl {
        name: f
            .id
            .as_ref()
            .map(|i| i.name.to_string())
            .unwrap_or_default(),
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
    let specs = i.specifiers.as_ref().map_or(vec![], |v| {
        v.iter()
            .map(|s| match s {
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

fn class_to_hir(c: &Class) -> hir::Decl {
    let methods: Vec<hir::ClassMethod> = c.body.body.iter().filter_map(|m| {
        if let ClassElement::MethodDefinition(def) = m {
            let name = match &def.key {
                PropertyKey::StaticIdentifier(i) => i.name.to_string(),
                PropertyKey::PrivateIdentifier(i) => format!("#{}", i.name),
                _ => String::new(),
            };
            // def.value is a Function struct
            let func = &*def.value;
            let body = if let Some(body_box) = &func.body {
                // Extract expression from first statement
                if let Some(stmt) = body_box.statements.first() {
                    match stmt {
                        Statement::ExpressionStatement(e) => convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined),
                        Statement::ReturnStatement(r) => r.argument.as_ref().and_then(|a| convert_expr(a)).unwrap_or(hir::Expr::Undefined),
                        _ => hir::Expr::Undefined,
                    }
                } else {
                    hir::Expr::Undefined
                }
            } else {
                hir::Expr::Undefined
            };
            let params: Vec<hir::Param> = func.params.items.iter().filter_map(|param| {
                if let BindingPattern::BindingIdentifier(i) = &param.pattern {
                    Some(hir::Param { name: i.name.to_string(), type_: None, default: None, optional: param.optional, pattern: None })
                } else { None }
            }).collect();
            Some(hir::ClassMethod { name, params, body, kind: hir::MethodKind::Method })
        } else { None }
    }).collect();
    hir::Decl::Class(hir::ClassDecl { name: c.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(), extends: None, members: vec![], generics: vec![], methods })
}

pub fn convert_module_item(stmt: &Statement) -> Option<hir::ModuleItem> {
    // Handle class expression (oxc parses class declarations as VariableDeclaration with ClassExpression init)
    if let Statement::VariableDeclaration(v) = stmt {
        if let Some(decl) = v.declarations.first() {
            if let BindingPattern::BindingIdentifier(_id) = &decl.id {
                if let Some(init) = &decl.init {
                    if matches!(init, Expression::ClassExpression(_)) {
                        if let Expression::ClassExpression(c) = init {
                            return Some(hir::ModuleItem::Decl(class_to_hir(c)));
                        }
                    }
                }
            }
        }
    }
    match stmt {
        Statement::ClassDeclaration(c) => Some(hir::ModuleItem::Decl(class_to_hir(c))),
        Statement::FunctionDeclaration(f) => Some(hir::ModuleItem::Decl(func_to_decl(f))),
        Statement::VariableDeclaration(v) => Some(hir::ModuleItem::Decl(var_to_decl(v))),
        Statement::ImportDeclaration(i) => Some(import_to_hir(i)),
        Statement::TSInterfaceDeclaration(i) => {
            Some(hir::ModuleItem::Decl(hir::Decl::Type(hir::TypeDecl {
                name: i.id.name.to_string(),
                generics: vec![],
                type_: hir::Type::Object { members: vec![] },
            })))
        }
        _ => None,
    }
}
