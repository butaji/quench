//! Literal expression lowering
//!
//! Handles lowering of literals, template literals, and property getters/setters.

use swc_ecma_ast as swc;
use crate::ast::{Expression, PropertyKey, PropertyValue};
use super::helpers::{wtf8_atom_to_string, LowerError};
use super::stmt::lower_stmt;

/// Lower a literal expression
pub fn lower_literal(lit: &swc::Lit) -> Result<Expression, LowerError> {
    match lit {
        swc::Lit::Num(n) => Ok(Expression::Number(n.value)),
        swc::Lit::Str(s) => Ok(Expression::String(wtf8_atom_to_string(&s.value))),
        swc::Lit::Bool(b) => Ok(Expression::Boolean(b.value)),
        swc::Lit::Null(_) => Ok(Expression::Null),
        swc::Lit::Regex(regex) => Ok(Expression::String(format!("/{}/{}", regex.exp, regex.flags))),
        swc::Lit::BigInt(_) => Err(LowerError::new("BigInt not supported")),
        swc::Lit::JSXText(t) => Ok(Expression::String(t.value.to_string())),
    }
}

/// Lower a template literal expression
pub fn lower_template_literal(tpl: &swc::Tpl) -> Result<Expression, LowerError> {
    use crate::ast::BinaryOp;

    if tpl.exprs.is_empty() {
        let mut result = String::new();
        for elem in &tpl.quasis {
            if let Some(cooked) = &elem.cooked {
                result.push_str(&wtf8_atom_to_string(cooked));
            }
        }
        return Ok(Expression::String(result));
    }

    let mut exprs: Vec<Expression> = Vec::new();
    let quasi_count = tpl.quasis.len();
    let expr_count = tpl.exprs.len();

    for i in 0..quasi_count {
        if let Some(cooked) = &tpl.quasis.get(i).and_then(|q| q.cooked.as_ref()) {
            let s = wtf8_atom_to_string(cooked);
            if !s.is_empty() {
                exprs.push(Expression::String(s));
            }
        }
        if i < expr_count {
            exprs.push(lower_template_expr(&tpl.exprs[i])?);
        }
    }

    if exprs.len() == 1 {
        return Ok(exprs.remove(0));
    }

    let mut result = exprs.remove(0);
    while !exprs.is_empty() {
        let right = exprs.remove(0);
        result = Expression::Binary {
            op: BinaryOp::Add,
            left: Box::new(result),
            right: Box::new(right),
        };
    }
    Ok(result)
}

fn lower_template_expr(expr: &swc::Expr) -> Result<Expression, LowerError> {
    // Import the main lowering function
    super::lower_expr(expr)
}

/// Lower a getter property
pub fn lower_getter_prop(getter: &swc::GetterProp) -> Result<(PropertyKey, PropertyValue), LowerError> {
    let key = lower_prop_name_key(&getter.key)?;
    let body = getter.body.as_ref()
        .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    Ok((key, PropertyValue::Getter { params: vec![], body }))
}

/// Lower a setter property
pub fn lower_setter_prop(setter: &swc::SetterProp) -> Result<(PropertyKey, PropertyValue), LowerError> {
    use super::helpers::atom_to_string;

    let key = lower_prop_name_key(&setter.key)?;
    let param = match &*setter.param {
        swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
        _ => "value".to_string(),
    };
    let body = setter.body.as_ref()
        .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    Ok((key, PropertyValue::Setter { param, body }))
}

/// Lower a method property
pub fn lower_method_prop(method: &swc::MethodProp) -> Result<(PropertyKey, PropertyValue), LowerError> {
    use super::helpers::atom_to_string;

    let key = lower_prop_name_key(&method.key)?;
    let params = method.function.params.iter().map(|p| {
        match &p.pat {
            swc::Pat::Ident(ident) => atom_to_string(&ident.id.sym),
            _ => "arg".to_string(),
        }
    }).collect();
    let body = method.function.body.as_ref()
        .map(|b| b.stmts.iter().filter_map(lower_stmt).collect())
        .unwrap_or_default();
    Ok((key, PropertyValue::Value(Expression::FunctionExpression { name: None, params, body })))
}

/// Lower a property name to PropertyKey
fn lower_prop_name_key(key: &swc::PropName) -> Result<PropertyKey, LowerError> {
    use super::helpers::lower_prop_name;
    lower_prop_name(key)
}
