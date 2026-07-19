//! Literal expression lowering
//!
//! Handles lowering of literals and template literals.

use super::expr::lower_expr;
use super::helpers::{wtf8_atom_to_string, LowerError};
use crate::ast::Expression;
use oxc::ast::ast;

/// Lower a template literal expression
pub fn lower_template_literal(tpl: &ast::TemplateLiteral) -> Result<Expression, LowerError> {
    use crate::ast::BinaryOp;

    if tpl.expressions.is_empty() {
        let mut result = String::new();
        for elem in &tpl.quasis {
            result.push_str(&elem.value.raw.to_string());
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
            exprs.push(lower_template_expr(&tpl.expressions[i])?);
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
    let strings: Vec<Expression> = tagged
        .quasi
        .quasis
        .iter()
        .map(|q| Expression::String(q.value.raw.to_string()))
        .collect();
    arguments.push(Expression::Array(strings));
    for expr in &tagged.quasi.expressions {
        arguments.push(lower_expr(expr)?);
    }
    Ok(Expression::Call {
        callee: Box::new(callee),
        arguments,
    })
}
