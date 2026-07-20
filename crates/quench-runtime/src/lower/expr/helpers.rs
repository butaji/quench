//! Private helper functions for expression lowering.
//! Main dispatch (lower_expr) lives here; specialized lowerers live in helpers_expr.rs.

use super::super::helpers::LowerError;
use super::super::jsx::{lower_jsx_element, lower_jsx_fragment};
use super::super::literals::{lower_tagged_template, lower_template_literal};
use super::super::opt_chain::lower_opt_chain;
use super::super::stmt::lower_formal_params;
use super::helpers_expr as expr_helpers;
use crate::ast::Statement;
use crate::ast::{ArrowBody, Expression, PropertyKey, PropertyValue};
use oxc::ast::ast;

pub fn lower_expr(expr: &ast::Expression) -> Result<Expression, LowerError> {
    match expr {
        ast::Expression::Identifier(ident) => {
            Ok(Expression::Identifier(ident.name.as_str().to_string()))
        }
        ast::Expression::ThisExpression(_) => Ok(Expression::Identifier("this".to_string())),
        ast::Expression::ArrayExpression(arr) => lower_array_expr(arr),
        ast::Expression::ObjectExpression(obj) => lower_object_expr(obj),
        ast::Expression::FunctionExpression(func) => lower_fn_expr(func),
        ast::Expression::ArrowFunctionExpression(arrow) => lower_arrow_expr(arrow),
        ast::Expression::YieldExpression(yield_expr) => lower_yield_expr(yield_expr),
        ast::Expression::MetaProperty(meta) => {
            if meta.meta.name == "new" && meta.property.name == "target" {
                Ok(Expression::Identifier("new.target".to_string()))
            } else {
                Ok(Expression::Undefined)
            }
        }
        ast::Expression::AwaitExpression(await_expr) => lower_expr(&await_expr.argument),
        ast::Expression::ParenthesizedExpression(paren) => lower_expr(&paren.expression),
        ast::Expression::BinaryExpression(bin) => expr_helpers::lower_bin_expr_pub(bin),
        ast::Expression::LogicalExpression(logical) => {
            expr_helpers::lower_logical_expr_pub(logical)
        }
        ast::Expression::UnaryExpression(unary) => expr_helpers::lower_unary_expr_pub(unary),
        ast::Expression::UpdateExpression(update) => expr_helpers::lower_update_expr_pub(update),
        ast::Expression::AssignmentExpression(assign) => {
            expr_helpers::lower_assign_expr_pub(assign)
        }
        ast::Expression::StaticMemberExpression(member) => {
            expr_helpers::lower_static_member_expr_pub(member)
        }
        ast::Expression::ComputedMemberExpression(member) => {
            expr_helpers::lower_computed_member_expr_pub(member)
        }
        ast::Expression::PrivateFieldExpression(member) => {
            expr_helpers::lower_private_field_expr_pub(member)
        }
        ast::Expression::Super(_) => Ok(Expression::Undefined),
        ast::Expression::CallExpression(call) => expr_helpers::lower_call_expr_pub(call),
        ast::Expression::NewExpression(new_expr) => expr_helpers::lower_new_expr_pub(new_expr),
        ast::Expression::SequenceExpression(seq) => expr_helpers::lower_seq_expr_pub(seq),
        ast::Expression::ConditionalExpression(cond) => expr_helpers::lower_cond_expr_pub(cond),
        ast::Expression::ChainExpression(chain) => lower_opt_chain(chain),
        ast::Expression::StringLiteral(s) => Ok(Expression::String(s.value.to_string())),
        ast::Expression::NumericLiteral(n) => Ok(Expression::Number(n.value)),
        ast::Expression::BooleanLiteral(b) => Ok(Expression::Boolean(b.value)),
        ast::Expression::NullLiteral(_) => Ok(Expression::Null),
        ast::Expression::RegExpLiteral(r) => Ok(Expression::RegExp {
            pattern: r.regex.pattern.to_string(),
            flags: r.regex.flags.to_string(),
        }),
        ast::Expression::BigIntLiteral(b) => Ok(Expression::BigInt(b.raw.to_string())),
        ast::Expression::TaggedTemplateExpression(tagged) => lower_tagged_template(tagged),
        ast::Expression::TemplateLiteral(tpl) => lower_template_literal(tpl),
        ast::Expression::ClassExpression(class_expr) => lower_class_expr(class_expr),
        ast::Expression::JSXElement(elem) => lower_jsx_element(elem),
        ast::Expression::JSXFragment(frag) => lower_jsx_fragment(frag),
        ast::Expression::TSAsExpression(e) => lower_expr(&e.expression),
        ast::Expression::TSSatisfiesExpression(e) => lower_expr(&e.expression),
        ast::Expression::TSTypeAssertion(e) => lower_expr(&e.expression),
        ast::Expression::TSNonNullExpression(e) => lower_expr(&e.expression),
        ast::Expression::TSInstantiationExpression(e) => lower_expr(&e.expression),
        _ => Ok(Expression::Undefined),
    }
}

fn lower_array_expr(arr: &ast::ArrayExpression) -> Result<Expression, LowerError> {
    let elements: Vec<Expression> = arr
        .elements
        .iter()
        .map(|elem| match elem {
            ast::ArrayExpressionElement::SpreadElement(spread) => {
                Ok(Expression::Spread(Box::new(lower_expr(&spread.argument)?)))
            }
            ast::ArrayExpressionElement::Elision(_) => Ok(Expression::Elision),
            elem => lower_expr(
                elem.as_expression()
                    .ok_or(LowerError::new("Invalid array element"))?,
            ),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Expression::Array(elements))
}

fn lower_object_expr(obj: &ast::ObjectExpression) -> Result<Expression, LowerError> {
    let ast_props: Vec<_> = obj.properties.iter().collect();
    let mut result: Vec<(PropertyKey, PropertyValue)> = Vec::new();

    for ast_prop in &ast_props {
        match ast_prop {
            ast::ObjectPropertyKind::ObjectProperty(prop) => {
                let (lowered_key, lowered_val) = lower_object_prop(prop)?;
                result.push((lowered_key, lowered_val));
            }
            ast::ObjectPropertyKind::SpreadProperty(_) => {}
        }
    }

    Ok(Expression::Object(result))
}

fn lower_object_prop(
    prop: &ast::ObjectProperty,
) -> Result<(PropertyKey, PropertyValue), LowerError> {
    let key = lower_prop_name_key_oxc(&prop.key)?;

    // OXC uses PropertyKind::Get for BOTH { get() {} } (getter shorthand) and
    // { get: fn } (method named "get"). Only treat as accessor if value is a
    // FunctionExpression — { get: fn } has kind=Get but value is NOT a FunctionExpression.
    if prop.kind == ast::PropertyKind::Get
        && matches!(&prop.value, ast::Expression::FunctionExpression(_))
    {
        let body = if let ast::Expression::FunctionExpression(func) = &prop.value {
            func.body
                .as_ref()
                .map(|b| super::super::helpers::lower_fn_body(b))
                .unwrap_or_default()
        } else {
            vec![]
        };
        return Ok((
            key,
            PropertyValue::Getter {
                params: vec![],
                body,
            },
        ));
    }

    if prop.kind == ast::PropertyKind::Set
        && matches!(&prop.value, ast::Expression::FunctionExpression(_))
    {
        let param = match &prop.value {
            ast::Expression::FunctionExpression(func) => func
                .params
                .items
                .first()
                .and_then(|p| {
                    if let ast::BindingPatternKind::BindingIdentifier(ident) = &p.pattern.kind {
                        Some(ident.name.as_str().to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "value".to_string()),
            _ => "value".to_string(),
        };
        let body = match &prop.value {
            ast::Expression::FunctionExpression(func) => func
                .body
                .as_ref()
                .map(|b| super::super::helpers::lower_fn_body(b))
                .unwrap_or_default(),
            ast::Expression::ArrowFunctionExpression(arrow) => {
                if arrow.expression {
                    vec![]
                } else {
                    let stmts = arrow
                        .body
                        .statements
                        .iter()
                        .filter_map(super::super::stmt::lower_stmt)
                        .collect();
                    stmts
                }
            }
            _ => vec![],
        };
        return Ok((key, PropertyValue::Setter { param, body }));
    }

    // Shorthand method syntax: `method=true` + `kind=Init`.
    // OXC uses this for `{ get() {} }`, `{ set(v) {} }`, and `{ foo() {} }`.
    // Real getter/setter accessors (`{ get key() {} }`, `{ set key(v) {} }`)
    // are handled above via `kind: Get/Set` — they also have `method: true`
    // but are caught before this branch by the kind check.
    // `{ get() {} }` is a concise method named "get", NOT a getter accessor.
    if prop.method {
        return lower_method_prop_from_value(&prop.key, &prop.value);
    }

    let value = lower_expr(&prop.value)?;
    Ok((key, PropertyValue::Value(value)))
}

fn lower_method_prop_from_value(
    key: &ast::PropertyKey,
    value: &ast::Expression,
) -> Result<(PropertyKey, PropertyValue), LowerError> {
    let key = lower_prop_name_key_oxc(key)?;
    if let ast::Expression::FunctionExpression(func) = value {
        let params = lower_formal_params(&func.params);
        let body = func
            .body
            .as_ref()
            .map(|b| super::super::helpers::lower_fn_body(b))
            .unwrap_or_default();
        Ok((
            key,
            PropertyValue::Value(Expression::FunctionExpression {
                name: None,
                params,
                body,
                is_async: false,
                is_generator: false,
            }),
        ))
    } else {
        let value = lower_expr(value)?;
        Ok((key, PropertyValue::Value(value)))
    }
}

fn lower_fn_expr(func: &ast::Function) -> Result<Expression, LowerError> {
    let name = func.id.as_ref().map(|i| i.name.as_str().to_string());
    let params = lower_formal_params(&func.params);
    let body = func
        .body
        .as_ref()
        .map(|b| super::super::helpers::lower_fn_body(b))
        .unwrap_or_default();
    Ok(Expression::FunctionExpression {
        name,
        params,
        body,
        is_async: func.r#async,
        is_generator: func.generator,
    })
}

fn lower_arrow_expr(arrow: &ast::ArrowFunctionExpression) -> Result<Expression, LowerError> {
    let params = lower_formal_params(&arrow.params);
    let body = if arrow.expression {
        let stmts = arrow
            .body
            .statements
            .iter()
            .filter_map(super::super::stmt::lower_stmt)
            .collect::<Vec<_>>();
        if stmts.len() == 1 {
            match &stmts[0] {
                Statement::Expression(expr) => ArrowBody::Expression(*expr.clone()),
                Statement::Return(Some(expr)) => ArrowBody::Expression(*expr.clone()),
                _ => ArrowBody::Block(std::rc::Rc::new(stmts)),
            }
        } else {
            ArrowBody::Block(std::rc::Rc::new(stmts))
        }
    } else {
        ArrowBody::Block(std::rc::Rc::new(super::super::helpers::lower_fn_body(
            &arrow.body,
        )))
    };
    Ok(Expression::ArrowFunction {
        params,
        body: Box::new(body),
    })
}

fn lower_yield_expr(yield_expr: &ast::YieldExpression) -> Result<Expression, LowerError> {
    if yield_expr.delegate {
        return Err(LowerError::new("Yield delegate not supported"));
    }
    match &yield_expr.argument {
        Some(expr) => lower_expr(expr),
        None => Ok(Expression::Undefined),
    }
}

fn lower_class_expr(class_expr: &ast::Class) -> Result<Expression, LowerError> {
    super::helpers_class::lower_class_expr(class_expr)
}

// Re-export class helpers for circular reference
pub use super::helpers_class::{
    lower_class, lower_class_expr as lower_class_expr_reexport, lower_prop_name_key_oxc,
};
