//! Optional chaining lowering
//!
//! Converts OXC optional chain expressions to conditional expressions.

use super::expr::lower_expr;
use super::helpers::LowerError;
use crate::ast::{Expression, PropertyKey};
use oxc::ast::ast;

/// Lower optional chaining expression
pub fn lower_opt_chain(chain: &ast::ChainExpression) -> Result<Expression, LowerError> {
    lower_chain_element(&chain.expression)
}

fn lower_chain_element(element: &ast::ChainElement) -> Result<Expression, LowerError> {
    match element {
        ast::ChainElement::CallExpression(call) => {
            // In OXC, ChainElement::CallExpression contains a CallExpression
            // The callee is a regular Expression (can be a MemberExpression from previous opt chain)
            let callee_expr = lower_expr(&call.callee)?;

            // Lower arguments
            let args: Vec<Expression> = call
                .arguments
                .iter()
                .map(|arg| {
                    if let Some(expr) = arg.as_expression() {
                        lower_expr(expr)
                    } else if let ast::Argument::SpreadElement(spread) = arg {
                        Ok(Expression::Spread(Box::new(lower_expr(&spread.argument)?)))
                    } else {
                        Err(LowerError::new("Invalid argument in optional chain"))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;

            Ok(Expression::Call {
                callee: Box::new(callee_expr),
                arguments: args,
            })
        }
        ast::ChainElement::StaticMemberExpression(member) => {
            let obj_expr = lower_expr(&member.object)?;
            let property = PropertyKey::Ident(member.property.name.as_str().to_string());
            Ok(Expression::Member {
                object: Box::new(obj_expr),
                property,
                computed: false,
            })
        }
        ast::ChainElement::ComputedMemberExpression(member) => {
            let obj_expr = lower_expr(&member.object)?;
            let property = PropertyKey::Computed(Box::new(lower_expr(&member.expression)?));
            Ok(Expression::Member {
                object: Box::new(obj_expr),
                property,
                computed: true,
            })
        }
        ast::ChainElement::PrivateFieldExpression(member) => {
            let obj_expr = lower_expr(&member.object)?;
            let property = PropertyKey::Ident(member.field.name.as_str().to_string());
            Ok(Expression::Member {
                object: Box::new(obj_expr),
                property,
                computed: false,
            })
        }
        // TypeScript non-null assertion in optional chain
        ast::ChainElement::TSNonNullExpression(e) => lower_expr(&e.expression),
    }
}
