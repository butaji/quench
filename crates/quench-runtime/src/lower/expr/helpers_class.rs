//! Private helper functions for class lowering.

use super::super::helpers::LowerError;
use super::super::stmt::lower_formal_params;
use crate::ast::{Class, ClassMember, Expression, PropertyKey};
use oxc::ast::ast;

fn lower_class_member(member: &ast::ClassElement) -> Result<ClassMember, LowerError> {
    match member {
        ast::ClassElement::MethodDefinition(method) => {
            if method.kind == ast::MethodDefinitionKind::Constructor {
                lower_constructor(method)
            } else {
                lower_method(method)
            }
        }
        ast::ClassElement::PropertyDefinition(prop) => lower_class_prop(prop),
        ast::ClassElement::StaticBlock(_) => Err(LowerError::new("Static blocks not supported")),
        ast::ClassElement::AccessorProperty(_) => {
            Err(LowerError::new("Accessor properties not supported"))
        }
        _ => Err(LowerError::new("Unsupported class member")),
    }
}

fn lower_constructor(constructor: &ast::MethodDefinition) -> Result<ClassMember, LowerError> {
    let ps: Vec<String> = constructor
        .value
        .params
        .items
        .iter()
        .filter_map(|p| match &p.pattern.kind {
            ast::BindingPatternKind::BindingIdentifier(ident) => {
                Some(ident.name.as_str().to_string())
            }
            _ => None,
        })
        .collect();
    let body = constructor
        .value
        .body
        .as_ref()
        .map(|b| {
            b.statements
                .iter()
                .filter_map(super::super::stmt::lower_stmt)
                .collect()
        })
        .unwrap_or_default();
    Ok(ClassMember::Constructor { params: ps, body })
}

fn lower_method(method: &ast::MethodDefinition) -> Result<ClassMember, LowerError> {
    let name = lower_prop_name_key_oxc(&method.key)?;
    let is_static = method.r#static;
    let is_async = method.value.r#async;
    let is_generator = method.value.generator;
    let ps: Vec<crate::ast::Param> = lower_formal_params(&method.value.params);
    let body = method
        .value
        .body
        .as_ref()
        .map(|b| {
            b.statements
                .iter()
                .filter_map(super::super::stmt::lower_stmt)
                .collect()
        })
        .unwrap_or_default();
    match method.kind {
        ast::MethodDefinitionKind::Get => Ok(ClassMember::Getter { name, body }),
        ast::MethodDefinitionKind::Set => {
            let param = ps.first().map(|p| p.name.clone()).unwrap_or_default();
            Ok(ClassMember::Setter { name, param, body })
        }
        _ => {
            if is_static {
                Ok(ClassMember::StaticMethod {
                    name,
                    params: ps,
                    body,
                    is_async,
                    is_generator,
                })
            } else {
                Ok(ClassMember::Method {
                    name,
                    params: ps,
                    body,
                    is_async,
                    is_generator,
                })
            }
        }
    }
}

fn lower_class_prop(prop: &ast::PropertyDefinition) -> Result<ClassMember, LowerError> {
    let name = lower_prop_name_key_oxc(&prop.key)?;
    let value = match &prop.value {
        Some(expr) => lower_expr(expr)?,
        None => Expression::Undefined,
    };
    if prop.r#static {
        Ok(ClassMember::StaticField { name, value })
    } else {
        Ok(ClassMember::Field { name, value })
    }
}

pub fn lower_prop_name_key_oxc(key: &ast::PropertyKey) -> Result<PropertyKey, LowerError> {
    match key {
        ast::PropertyKey::StaticIdentifier(i) => {
            Ok(PropertyKey::Ident(i.name.as_str().to_string()))
        }
        ast::PropertyKey::PrivateIdentifier(i) => Ok(PropertyKey::Ident(format!("#{}", i.name))),
        ast::PropertyKey::StringLiteral(s) => Ok(PropertyKey::String(s.value.to_string())),
        ast::PropertyKey::NumericLiteral(n) => Ok(PropertyKey::Number(n.value)),
        ast::PropertyKey::BigIntLiteral(b) => Ok(PropertyKey::String(b.raw.to_string())),
        ast::PropertyKey::BooleanLiteral(b) => Ok(PropertyKey::String(b.value.to_string())),
        ast::PropertyKey::NullLiteral(_) => Ok(PropertyKey::String("null".to_string())),
        ast::PropertyKey::TemplateLiteral(t) if t.expressions.is_empty() => {
            let static_part = t
                .quasis
                .first()
                .map(|q| {
                    q.value
                        .cooked
                        .as_ref()
                        .map(|c| c.as_str())
                        .unwrap_or(q.value.raw.as_str())
                        .to_string()
                })
                .unwrap_or_default();
            Ok(PropertyKey::String(static_part))
        }
        ast::PropertyKey::TemplateLiteral(_) => {
            let expr = key.to_expression();
            Ok(PropertyKey::Computed(Box::new(lower_expr(expr)?)))
        }
        _ => {
            let expr = key.to_expression();
            Ok(PropertyKey::Computed(Box::new(lower_expr(expr)?)))
        }
    }
}

pub fn lower_class(class: &ast::Class) -> Result<Class, LowerError> {
    let super_class = class.super_class.as_ref().and_then(|e| lower_expr(e).ok());
    let body = class
        .body
        .body
        .iter()
        .map(lower_class_member)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Class {
        name: None,
        super_class: super_class.map(Box::new),
        body,
    })
}

pub fn lower_class_expr(class_expr: &ast::Class) -> Result<Expression, LowerError> {
    let class = lower_class(class_expr)?;
    let name: Option<String> = class_expr.id.as_ref().map(|i| i.name.as_str().to_string());
    Ok(Expression::Class(Class {
        name,
        super_class: class.super_class,
        body: class.body,
    }))
}

// Need to import lower_expr from sibling module
pub use super::helpers::lower_expr;
