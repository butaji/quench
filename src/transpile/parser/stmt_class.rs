//! Class conversion - class_to_hir

use crate::transpile::hir;
use crate::transpile::parser::expr::{convert_binding_pattern, convert_expr};

use oxc_ast::ast::*;

/// Convert oxc decorators to HIR decorators
fn convert_decorators(decorators: &[Decorator]) -> Vec<hir::Decorator> {
    decorators.iter().filter_map(|d| convert_decorator_expr(d)).collect()
}

/// Convert a single decorator expression to HIR
fn convert_decorator_expr(d: &Decorator) -> Option<hir::Decorator> {
    let expr = convert_expr(&d.expression).ok()?;
    Some(hir::Decorator { expr })
}

pub fn class_to_hir(c: &Class) -> hir::Decl {
    let ctor_param_props = extract_ctor_param_properties(c);
    let (members, methods) = build_class_members(c, &ctor_param_props);

    hir::Decl::Class(hir::ClassDecl {
        name: c.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(),
        extends: None,
        members,
        generics: vec![],
        methods,
        decorators: convert_decorators(&c.decorators),
    })
}

fn build_class_members(
    c: &Class,
    ctor_param_props: &[(String, bool)],
) -> (Vec<hir::ClassMember>, Vec<hir::ClassMethod>) {
    let mut members: Vec<hir::ClassMember> = Vec::new();
    let mut methods: Vec<hir::ClassMethod> = Vec::new();

    for m in &c.body.body {
        match m {
            ClassElement::MethodDefinition(def) => {
                let is_ctor = is_constructor_def(def);
                let method = convert_method_def(def, is_ctor.then_some(ctor_param_props));
                if let Some(m) = method { methods.push(m); }
            }
            ClassElement::PropertyDefinition(prop) => {
                if let Some(member) = convert_prop_def(prop) { members.push(member); }
            }
            _ => {}
        }
    }

    for (name, is_private) in ctor_param_props {
        members.push(hir::ClassMember {
            name: name.clone(),
            type_: None,
            is_static: false,
            is_async: false,
            is_private: *is_private,
            decorators: vec![],
        });
    }

    (members, methods)
}

fn is_constructor_def(def: &MethodDefinition) -> bool {
    matches!(&def.key, PropertyKey::StaticIdentifier(i) if i.name == "constructor")
}

/// Extract parameter property names and privacy flags from constructor.
fn extract_ctor_param_properties(c: &Class) -> Vec<(String, bool)> {
    let mut result = Vec::new();
    for m in &c.body.body {
        if let ClassElement::MethodDefinition(def) = m {
            if let PropertyKey::StaticIdentifier(i) = &def.key {
                if i.name == "constructor" {
                    for param in &def.value.params.items {
                        if param.accessibility.is_some() || param.readonly {
                            if let BindingPattern::BindingIdentifier(id) = &param.pattern {
                                let is_private = matches!(param.accessibility, Some(TSAccessibility::Private));
                                result.push((id.name.to_string(), is_private));
                            }
                        }
                    }
                }
            }
        }
    }
    result
}

fn convert_method_def(
    def: &MethodDefinition,
    param_props: Option<&[(String, bool)]>,
) -> Option<hir::ClassMethod> {
    let name = match &def.key {
        PropertyKey::StaticIdentifier(i) => i.name.to_string(),
        PropertyKey::PrivateIdentifier(i) => format!("#{}", i.name),
        _ => return None,
    };
    let func = &*def.value;
    let body = if let Some(props) = param_props {
        build_ctor_body_with_param_props(func, props)
    } else {
        extract_method_body(func)
    };
    let params = convert_func_params(func);
    let kind = if name == "constructor" { hir::MethodKind::Constructor } else { hir::MethodKind::Method };
    Some(hir::ClassMethod {
        name,
        params,
        body,
        kind,
        decorators: convert_decorators(&def.decorators),
    })
}

fn build_ctor_body_with_param_props(
    func: &Function,
    props: &[(String, bool)],
) -> hir::Expr {
    let mut exprs: Vec<hir::Expr> = Vec::new();
    for (name, _) in props {
        exprs.push(hir::Expr::Assign {
            op: hir::AssignOp::Assign,
            left: Box::new(hir::Expr::StaticMember {
                obj: Box::new(hir::Expr::Ident { name: "this".to_string() }),
                property: name.clone(),
                optional: false,
            }),
            right: Box::new(hir::Expr::Ident { name: name.clone() }),
        });
    }
    let existing = extract_method_body(func);
    build_seq_expr(exprs, existing)
}

fn build_seq_expr(mut exprs: Vec<hir::Expr>, tail: hir::Expr) -> hir::Expr {
    exprs.push(tail);
    let mut it = exprs.into_iter().rev();
    let last = it.next().unwrap_or(hir::Expr::Undefined);
    it.fold(last, |acc, expr| hir::Expr::Seq {
        left: Box::new(expr),
        right: Box::new(acc),
    })
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
        let default = param.initializer.as_ref().and_then(|e| convert_expr(e).ok());
        if let BindingPattern::BindingIdentifier(i) = &param.pattern {
            Some(hir::Param {
                name: i.name.to_string(),
                type_: None,
                default,
                optional: param.optional,
                pattern: None,
                ownership: hir::Ownership::Owned,
            })
        } else {
            convert_binding_pattern(&param.pattern).map(|pattern| hir::Param {
                name: String::new(),
                type_: None,
                default,
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
    Some(hir::ClassMember {
        name,
        type_: None,
        is_static: prop.r#static,
        is_async: false,
        is_private,
        decorators: convert_decorators(&prop.decorators),
    })
}
