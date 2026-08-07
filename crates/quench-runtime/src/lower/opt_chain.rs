//! Optional chaining lowering
//!
//! Converts OXC optional chain expressions to conditional expressions.

use super::expr::lower_expr;
use super::helpers::LowerError;
use crate::ast::{Expression, PropertyKey};
use oxc::ast::ast;

/// Lower optional chaining expression
/// Produces: base == null || base == undefined ? undefined : chain_result
pub fn lower_opt_chain(chain: &ast::ChainExpression) -> Result<Expression, LowerError> {
    // Recursively lower the chain, tracking the base expression
    lower_chain_recursive(&chain.expression, None)
}

/// Recursively lower chain elements, building up the expression
fn lower_chain_recursive(
    element: &ast::ChainElement,
    prev_expr: Option<&Expression>,
) -> Result<Expression, LowerError> {
    match element {
        ast::ChainElement::StaticMemberExpression(member) => {
            // Build: prev?.prop or obj.prop (for first element)
            let object_expr = lower_chain_base(&member.object)?;
            let property = PropertyKey::Ident(member.property.name.as_str().to_string());

            if let Some(prev) = prev_expr {
                // Chained: check prev for nullish, then access property
                let undefined = Expression::Undefined;
                let is_nullish = make_nullish_check(prev);
                let member_access = Expression::Member {
                    object: Box::new(prev.clone()),
                    property,
                    computed: false,
                };
                Ok(Expression::Conditional {
                    condition: Box::new(is_nullish),
                    consequent: Box::new(undefined),
                    alternate: Box::new(member_access),
                })
            } else if member.optional {
                let undefined = Expression::Undefined;
                let is_nullish = make_nullish_check(&object_expr);
                let member_access = Expression::Member {
                    object: Box::new(object_expr),
                    property,
                    computed: false,
                };
                Ok(Expression::Conditional {
                    condition: Box::new(is_nullish),
                    consequent: Box::new(undefined),
                    alternate: Box::new(member_access),
                })
            } else {
                // First element: direct member access
                Ok(Expression::Member {
                    object: Box::new(object_expr),
                    property,
                    computed: false,
                })
            }
        }
        ast::ChainElement::ComputedMemberExpression(member) => {
            let object_expr = lower_chain_base(&member.object)?;
            let prop_expr = lower_expr(&member.expression)?;

            if let Some(prev) = prev_expr {
                let undefined = Expression::Undefined;
                let is_nullish = make_nullish_check(prev);
                let member_access = Expression::Member {
                    object: Box::new(prev.clone()),
                    property: PropertyKey::Computed(Box::new(prop_expr)),
                    computed: true,
                };
                Ok(Expression::Conditional {
                    condition: Box::new(is_nullish),
                    consequent: Box::new(undefined),
                    alternate: Box::new(member_access),
                })
            } else if member.optional {
                let undefined = Expression::Undefined;
                let is_nullish = make_nullish_check(&object_expr);
                let member_access = Expression::Member {
                    object: Box::new(object_expr),
                    property: PropertyKey::Computed(Box::new(prop_expr)),
                    computed: true,
                };
                Ok(Expression::Conditional {
                    condition: Box::new(is_nullish),
                    consequent: Box::new(undefined),
                    alternate: Box::new(member_access),
                })
            } else {
                Ok(Expression::Member {
                    object: Box::new(object_expr),
                    property: PropertyKey::Computed(Box::new(prop_expr)),
                    computed: true,
                })
            }
        }
        ast::ChainElement::CallExpression(call) => {
            // Determine the object to check for nullish
            let base_for_check = if let Some(prev) = prev_expr {
                // Previous element's result is what we check
                prev.clone()
            } else {
                // First element - extract from callee if it's a member access
                extract_base_from_callee(&call.callee)?
            };

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

            // Build the full call expression: base(args)
            let full_call = Expression::Call {
                callee: Box::new(lower_expr(&call.callee)?),
                arguments: args,
            };

            // Check base for nullish
            let undefined = Expression::Undefined;
            let is_nullish = make_nullish_check(&base_for_check);

            Ok(Expression::Conditional {
                condition: Box::new(is_nullish),
                consequent: Box::new(undefined),
                alternate: Box::new(full_call),
            })
        }
        ast::ChainElement::PrivateFieldExpression(member) => {
            let base_expr = lower_chain_base(&member.object)?;
            let property =
                PropertyKey::Ident(crate::value::private_name_key(member.field.name.as_str()));

            if let Some(prev) = prev_expr {
                let undefined = Expression::Undefined;
                let is_nullish = make_nullish_check(prev);
                let member_access = Expression::Member {
                    object: Box::new(prev.clone()),
                    property,
                    computed: false,
                };
                Ok(Expression::Conditional {
                    condition: Box::new(is_nullish),
                    consequent: Box::new(undefined),
                    alternate: Box::new(member_access),
                })
            } else {
                let undefined = Expression::Undefined;
                let is_nullish = make_nullish_check(&base_expr);
                let member_access = Expression::Member {
                    object: Box::new(base_expr),
                    property,
                    computed: false,
                };
                Ok(Expression::Conditional {
                    condition: Box::new(is_nullish),
                    consequent: Box::new(undefined),
                    alternate: Box::new(member_access),
                })
            }
        }
        // TypeScript non-null assertion in optional chain
        ast::ChainElement::TSNonNullExpression(e) => lower_expr(&e.expression),
    }
}

/// Lower the object side of a chain element, preserving `?.` on nested members.
fn lower_chain_base(object: &ast::Expression) -> Result<Expression, LowerError> {
    match object {
        ast::Expression::StaticMemberExpression(member) if member.optional => {
            let object_expr = lower_expr(&member.object)?;
            let property = PropertyKey::Ident(member.property.name.as_str().to_string());
            let member_access = Expression::Member {
                object: Box::new(object_expr.clone()),
                property,
                computed: false,
            };
            Ok(Expression::Conditional {
                condition: Box::new(make_nullish_check(&object_expr)),
                consequent: Box::new(Expression::Undefined),
                alternate: Box::new(member_access),
            })
        }
        ast::Expression::ComputedMemberExpression(member) if member.optional => {
            let object_expr = lower_expr(&member.object)?;
            let prop_expr = lower_expr(&member.expression)?;
            let member_access = Expression::Member {
                object: Box::new(object_expr.clone()),
                property: PropertyKey::Computed(Box::new(prop_expr)),
                computed: true,
            };
            Ok(Expression::Conditional {
                condition: Box::new(make_nullish_check(&object_expr)),
                consequent: Box::new(Expression::Undefined),
                alternate: Box::new(member_access),
            })
        }
        ast::Expression::ChainExpression(chain) => lower_opt_chain(chain),
        _ => lower_expr(object),
    }
}

/// Extract the base expression from a callee for nullish checking
fn extract_base_from_callee(callee: &ast::Expression) -> Result<Expression, LowerError> {
    match callee {
        ast::Expression::StaticMemberExpression(member) => {
            // For a?.b, the base is a
            lower_expr(&member.object)
        }
        ast::Expression::ComputedMemberExpression(member) => lower_expr(&member.object),
        ast::Expression::PrivateFieldExpression(member) => lower_expr(&member.object),
        ast::Expression::Identifier(ident) => {
            // For foo?.(), the base is foo
            Ok(Expression::Identifier(ident.name.as_str().to_string()))
        }
        ast::Expression::ChainExpression(chain) => {
            // Recursively extract from nested chain
            lower_opt_chain(chain)
        }
        _ => lower_expr(callee),
    }
}

/// Create a nullish check: base == null || base == undefined
fn make_nullish_check(base: &Expression) -> Expression {
    let null_check = Expression::Binary {
        op: crate::ast::BinaryOp::Eq,
        left: Box::new(base.clone()),
        right: Box::new(Expression::Null),
    };
    let undefined_check = Expression::Binary {
        op: crate::ast::BinaryOp::Eq,
        left: Box::new(base.clone()),
        right: Box::new(Expression::Undefined),
    };
    Expression::Binary {
        op: crate::ast::BinaryOp::Or,
        left: Box::new(null_check),
        right: Box::new(undefined_check),
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{BinaryOp, Expression, PropertyKey};

    #[test]
    fn test_expression_equality() {
        let a = Expression::Number(1.0);
        let b = Expression::Number(1.0);
        let c = Expression::Number(2.0);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_expression_null_undefined() {
        assert_eq!(Expression::Null, Expression::Null);
        assert_eq!(Expression::Undefined, Expression::Undefined);
        assert_ne!(Expression::Null, Expression::Undefined);
    }

    #[test]
    fn test_property_key_comparison() {
        let k1 = PropertyKey::Ident("a".to_string());
        let k2 = PropertyKey::Ident("a".to_string());
        let k3 = PropertyKey::Ident("b".to_string());
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_binary_op_precedence_different_ops() {
        assert_eq!(BinaryOp::And.precedence(), 2);
        assert_eq!(BinaryOp::Or.precedence(), 1);
        assert!(BinaryOp::And.precedence() > BinaryOp::Or.precedence());
    }

    #[test]
    fn test_expression_member() {
        let expr = Expression::Member {
            object: Box::new(Expression::Identifier("obj".to_string())),
            property: PropertyKey::Ident("prop".to_string()),
            computed: false,
        };
        assert_eq!(expr, expr);
    }

    #[test]
    fn test_expression_call() {
        let expr = Expression::Call {
            callee: Box::new(Expression::Identifier("fn".to_string())),
            arguments: vec![Expression::Number(1.0)],
        };
        assert_eq!(expr, expr);
    }
}
