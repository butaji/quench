//! Expression lowering - convert OXC expressions to runtime AST expressions

pub mod helpers;
pub mod helpers_class;
pub mod helpers_expr;

use super::helpers::LowerError;
use crate::ast::{Class, Expression, PropertyKey};
use oxc::ast::ast;

// Re-export public API (note: lower_expr and lower_assignment_target also have pub fn wrappers below)

/// Lower an OXC Expression to our Expression
#[allow(clippy::complexity)]
pub fn lower_expr(expr: &ast::Expression) -> Result<Expression, LowerError> {
    helpers::lower_expr(expr)
}

/// Lower an assignment target to an expression
pub fn lower_assignment_target(target: &ast::AssignmentTarget) -> Result<Expression, LowerError> {
    helpers_expr::lower_assignment_target(target)
}

/// Lower an OXC Class to our Class
pub fn lower_class(class: &ast::Class) -> Result<Class, LowerError> {
    helpers_class::lower_class(class)
}

#[allow(dead_code)]
pub(crate) fn lower_member_prop(
    prop: &ast::IdentifierName,
) -> Result<(PropertyKey, bool), LowerError> {
    Ok((PropertyKey::Ident(prop.name.as_str().to_string()), false))
}
