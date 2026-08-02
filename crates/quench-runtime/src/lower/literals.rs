//! Literal expression lowering
//!
//! Handles lowering of literals and template literals.

use super::expr::lower_expr;
use super::helpers::LowerError;
use crate::ast::{Expression, PropertyKey, PropertyValue};
use oxc::ast::ast;

/// Lower a template literal expression
pub fn lower_template_literal(tpl: &ast::TemplateLiteral) -> Result<Expression, LowerError> {
    use crate::ast::BinaryOp;

    if tpl.expressions.is_empty() {
        let mut result = String::new();
        for elem in &tpl.quasis {
            result.push_str(elem.value.raw.as_ref());
        }
        return Ok(Expression::String(result));
    }

    let mut exprs: Vec<Expression> = Vec::new();
    let quasi_count = tpl.quasis.len();
    let expr_count = tpl.expressions.len();

    for i in 0..quasi_count {
        let quasi = &tpl.quasis[i];
        let s = quasi.value.raw.to_string();
        if !s.is_empty() {
            exprs.push(Expression::String(s));
        }
        if i < expr_count {
            exprs.push(Expression::Call {
                callee: Box::new(Expression::Identifier("String".to_string())),
                arguments: vec![lower_template_expr(&tpl.expressions[i])?],
            });
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

fn lower_template_expr(expr: &ast::Expression) -> Result<Expression, LowerError> {
    super::lower_expr(expr)
}

/// Lower a tagged template expression: `tag`s0${x}s1`` → `tag(["s0", "s1"], x)`.
///
/// The strings array does not carry a `.raw` property; tags that need raw
/// strings are out of scope until a harness/test requires them.
pub fn lower_tagged_template(
    tagged: &ast::TaggedTemplateExpression,
) -> Result<Expression, LowerError> {
    let callee = lower_expr(&tagged.tag)?;
    let mut arguments = Vec::with_capacity(tagged.quasi.expressions.len() + 1);
    arguments.push(Expression::String(format!(
        "\0quench-template-site:{}",
        tagged.span.start
    )));
    let cooked = Expression::Array(
        tagged
            .quasi
            .quasis
            .iter()
            .map(|q| {
                q.value
                    .cooked
                    .as_ref()
                    .map(|value| Expression::String(value.to_string()))
                    .unwrap_or(Expression::Undefined)
            })
            .collect(),
    );
    let raw = Expression::Array(
        tagged
            .quasi
            .quasis
            .iter()
            .map(|q| Expression::String(q.value.raw.to_string()))
            .collect(),
    );
    let descriptor = Expression::Object(vec![
        (
            PropertyKey::Ident("value".to_string()),
            PropertyValue::Value(freeze_object(raw)),
        ),
        (
            PropertyKey::Ident("enumerable".to_string()),
            PropertyValue::Value(Expression::Boolean(false)),
        ),
        (
            PropertyKey::Ident("writable".to_string()),
            PropertyValue::Value(Expression::Boolean(false)),
        ),
        (
            PropertyKey::Ident("configurable".to_string()),
            PropertyValue::Value(Expression::Boolean(false)),
        ),
    ]);
    let template = Expression::Call {
        callee: Box::new(Expression::Member {
            object: Box::new(Expression::Identifier("Object".to_string())),
            property: PropertyKey::Ident("defineProperty".to_string()),
            computed: false,
        }),
        arguments: vec![cooked, Expression::String("raw".to_string()), descriptor],
    };
    arguments.push(freeze_object(template));
    for expr in &tagged.quasi.expressions {
        arguments.push(lower_expr(expr)?);
    }
    Ok(Expression::Call {
        callee: Box::new(callee),
        arguments,
    })
}

fn freeze_object(object: Expression) -> Expression {
    Expression::Call {
        callee: Box::new(Expression::Member {
            object: Box::new(Expression::Identifier("Object".to_string())),
            property: PropertyKey::Ident("freeze".to_string()),
            computed: false,
        }),
        arguments: vec![object],
    }
}
