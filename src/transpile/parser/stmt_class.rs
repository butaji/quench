//! Class conversion - class_to_hir

use crate::transpile::hir;
use crate::transpile::parser::expr::{convert_binding_pattern, convert_expr};

use oxc_ast::ast::*;

pub fn class_to_hir(c: &Class) -> hir::Decl {
    let mut members: Vec<hir::ClassMember> = Vec::new();
    let mut methods: Vec<hir::ClassMethod> = Vec::new();

    for m in &c.body.body {
        match m {
            ClassElement::MethodDefinition(def) => {
                if let Some(method) = convert_method_def(def) {
                    methods.push(method);
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if let Some(member) = convert_prop_def(prop) {
                    members.push(member);
                }
            }
            _ => {}
        }
    }
    hir::Decl::Class(hir::ClassDecl {
        name: c.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(),
        extends: None,
        members,
        generics: vec![],
        methods,
    })
}

fn convert_method_def(def: &MethodDefinition) -> Option<hir::ClassMethod> {
    let name = match &def.key {
        PropertyKey::StaticIdentifier(i) => i.name.to_string(),
        PropertyKey::PrivateIdentifier(i) => format!("#{}", i.name),
        _ => return None,
    };
    let func = &*def.value;
    let body = extract_method_body(func);
    let params = convert_func_params(func);
    let kind = if name == "constructor" { hir::MethodKind::Constructor } else { hir::MethodKind::Method };
    Some(hir::ClassMethod { name, params, body, kind })
}

fn extract_method_body(func: &Function) -> hir::Expr {
    if let Some(body_box) = &func.body {
        if let Some(stmt) = body_box.statements.first() {
            return match stmt {
                Statement::ExpressionStatement(e) => convert_expr(&e.expression).unwrap_or(hir::Expr::Undefined),
                Statement::ReturnStatement(r) => r.argument.as_ref().and_then(|a| convert_expr(a).ok()).unwrap_or(hir::Expr::Undefined),
                _ => hir::Expr::Undefined,
            };
        }
    }
    hir::Expr::Undefined
}

fn convert_func_params(func: &Function) -> Vec<hir::Param> {
    func.params.items.iter().filter_map(|param| {
        if let BindingPattern::BindingIdentifier(i) = &param.pattern {
            Some(hir::Param {
                name: i.name.to_string(),
                type_: None,
                default: None,
                optional: param.optional,
                pattern: None,
                ownership: hir::Ownership::Owned,
            })
        } else {
            convert_binding_pattern(&param.pattern).map(|pattern| hir::Param {
                name: String::new(),
                type_: None,
                default: None,
                optional: param.optional,
                pattern: Some(pattern),
                ownership: hir::Ownership::Owned,
            })
        }
    }).collect()
}

fn convert_prop_def(prop: &PropertyDefinition) -> Option<hir::ClassMember> {
    let (name, is_private) = match &prop.key {
        PropertyKey::StaticIdentifier(i) => (i.name.to_string(), false),
        PropertyKey::PrivateIdentifier(i) => (format!("#{}", i.name), true),
        PropertyKey::StringLiteral(s) => (s.value.to_string(), false),
        PropertyKey::NumericLiteral(n) => (n.value.to_string(), false),
        _ => return None,
    };
    Some(hir::ClassMember { name, type_: None, is_static: prop.r#static, is_async: false, is_private })
}
