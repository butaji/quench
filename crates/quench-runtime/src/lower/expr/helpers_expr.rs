//! Expression-lowering helper functions.
//! Contains binary/logical/unary/update/assign/member/call/new/seq/cond lowerers
//! plus assignment target lowering.

use super::super::helpers::LowerError;
use super::super::helpers::{assign_op_to_bin, lower_bin_op, lower_logical_op, lower_unary_op};
use crate::ast::{Expression, PropertyKey};
use oxc::ast::ast;
use oxc::syntax::operator::{AssignmentOperator, UpdateOperator};

// Re-export lower_expr for use by other helpers in this module
use super::helpers::lower_expr as lower_expr_inner;

fn lower_bin_expr(bin: &ast::BinaryExpression) -> Result<Expression, LowerError> {
    let left = lower_expr_inner(&bin.left)?;
    let right = lower_expr_inner(&bin.right)?;
    let op = lower_bin_op(&bin.operator)?;
    Ok(Expression::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn lower_logical_expr(logical: &ast::LogicalExpression) -> Result<Expression, LowerError> {
    let left = lower_expr_inner(&logical.left)?;
    let right = lower_expr_inner(&logical.right)?;
    let op = lower_logical_op(&logical.operator)?;
    Ok(Expression::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn lower_unary_expr(unary: &ast::UnaryExpression) -> Result<Expression, LowerError> {
    let arg = lower_expr_inner(&unary.argument)?;
    let op = lower_unary_op(&unary.operator)?;
    Ok(Expression::Unary {
        op,
        argument: Box::new(arg),
    })
}

fn lower_update_expr(update: &ast::UpdateExpression) -> Result<Expression, LowerError> {
    let arg = lower_simple_assignment_target(&update.argument)?;
    let op = if update.operator == UpdateOperator::Increment {
        crate::ast::UpdateOp::Increment
    } else {
        crate::ast::UpdateOp::Decrement
    };
    Ok(Expression::Update {
        op,
        argument: Box::new(arg),
        prefix: update.prefix,
    })
}

fn is_logical_compound_op(op: &AssignmentOperator) -> bool {
    matches!(
        op,
        AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalNullish
    )
}

fn lower_assign_expr(assign: &ast::AssignmentExpression) -> Result<Expression, LowerError> {
    let left = lower_assignment_target(&assign.left)?;
    let right = lower_expr_inner(&assign.right)?;
    if assign.operator == AssignmentOperator::Assign {
        Ok(Expression::Assignment {
            left: Box::new(left),
            right: Box::new(right),
        })
    } else if is_logical_compound_op(&assign.operator) {
        let comp_op = assign_op_to_bin(&assign.operator)?;
        Ok(Expression::LogicalCompoundAssignment {
            op: comp_op,
            left: Box::new(left),
            right: Box::new(right),
        })
    } else {
        let bin_op = assign_op_to_bin(&assign.operator)?;
        Ok(Expression::CompoundAssignment {
            op: bin_op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }
}

fn lower_static_member_expr(
    member: &ast::StaticMemberExpression,
) -> Result<Expression, LowerError> {
    if matches!(member.object, ast::Expression::Super(_)) {
        let property = PropertyKey::Ident(member.property.name.as_str().to_string());
        return Ok(Expression::Member {
            object: Box::new(Expression::Identifier("super".to_string())),
            property,
            computed: false,
        });
    }
    let obj = lower_expr_inner(&member.object)?;
    let property = PropertyKey::Ident(member.property.name.as_str().to_string());
    Ok(Expression::Member {
        object: Box::new(obj),
        property,
        computed: false,
    })
}

fn lower_computed_member_expr(
    member: &ast::ComputedMemberExpression,
) -> Result<Expression, LowerError> {
    if matches!(member.object, ast::Expression::Super(_)) {
        let property = PropertyKey::Computed(Box::new(lower_expr_inner(&member.expression)?));
        return Ok(Expression::Member {
            object: Box::new(Expression::Identifier("super".to_string())),
            property,
            computed: true,
        });
    }
    let obj = lower_expr_inner(&member.object)?;
    let property = PropertyKey::Computed(Box::new(lower_expr_inner(&member.expression)?));
    Ok(Expression::Member {
        object: Box::new(obj),
        property,
        computed: true,
    })
}

fn private_field_prop_key(name: &str) -> PropertyKey {
    PropertyKey::Ident(crate::value::private_name_key(name))
}

fn lower_private_field_expr(
    member: &ast::PrivateFieldExpression,
) -> Result<Expression, LowerError> {
    let obj = lower_expr_inner(&member.object)?;
    let property = private_field_prop_key(member.field.name.as_str());
    Ok(Expression::Member {
        object: Box::new(obj),
        property,
        computed: false,
    })
}

fn lower_call_expr(call: &ast::CallExpression) -> Result<Expression, LowerError> {
    let callee = match &call.callee {
        ast::Expression::ImportExpression(_) => {
            return Err(LowerError::new("import() not supported"));
        }
        ast::Expression::Super(_) => Expression::Identifier("super".to_string()),
        _ => lower_expr_inner(&call.callee)?,
    };
    let mut args = Vec::new();
    for arg in &call.arguments {
        let expr = match arg {
            ast::Argument::SpreadElement(spread) => {
                Expression::Spread(Box::new(lower_expr_inner(&spread.argument)?))
            }
            arg => lower_expr_inner(
                arg.as_expression()
                    .ok_or(LowerError::new("Invalid argument"))?,
            )?,
        };
        args.push(expr);
    }
    Ok(Expression::Call {
        callee: Box::new(callee),
        arguments: args,
    })
}

fn lower_new_expr(new_expr: &ast::NewExpression) -> Result<Expression, LowerError> {
    let constructor = lower_expr_inner(&new_expr.callee)?;
    let mut args = Vec::new();
    for arg in &new_expr.arguments {
        let expr = match arg {
            ast::Argument::SpreadElement(spread) => {
                Expression::Spread(Box::new(lower_expr_inner(&spread.argument)?))
            }
            arg => lower_expr_inner(
                arg.as_expression()
                    .ok_or(LowerError::new("Invalid argument"))?,
            )?,
        };
        args.push(expr);
    }
    Ok(Expression::New {
        constructor: Box::new(constructor),
        arguments: args,
    })
}

fn lower_seq_expr(seq: &ast::SequenceExpression) -> Result<Expression, LowerError> {
    let exprs: Vec<Expression> = seq
        .expressions
        .iter()
        .filter_map(|e| lower_expr_inner(e).ok())
        .collect();
    Ok(Expression::Sequence(exprs))
}

fn lower_cond_expr(cond: &ast::ConditionalExpression) -> Result<Expression, LowerError> {
    let test = lower_expr_inner(&cond.test)?;
    let consequent = lower_expr_inner(&cond.consequent)?;
    let alternate = lower_expr_inner(&cond.alternate)?;
    Ok(Expression::Conditional {
        condition: Box::new(test),
        consequent: Box::new(consequent),
        alternate: Box::new(alternate),
    })
}

// Re-export for use by lower_expr in helpers.rs
pub fn lower_bin_expr_pub(bin: &ast::BinaryExpression) -> Result<Expression, LowerError> {
    lower_bin_expr(bin)
}

pub fn lower_logical_expr_pub(logical: &ast::LogicalExpression) -> Result<Expression, LowerError> {
    lower_logical_expr(logical)
}

pub fn lower_unary_expr_pub(unary: &ast::UnaryExpression) -> Result<Expression, LowerError> {
    lower_unary_expr(unary)
}

pub fn lower_update_expr_pub(update: &ast::UpdateExpression) -> Result<Expression, LowerError> {
    lower_update_expr(update)
}

pub fn lower_assign_expr_pub(assign: &ast::AssignmentExpression) -> Result<Expression, LowerError> {
    lower_assign_expr(assign)
}

pub fn lower_static_member_expr_pub(
    member: &ast::StaticMemberExpression,
) -> Result<Expression, LowerError> {
    lower_static_member_expr(member)
}

pub fn lower_computed_member_expr_pub(
    member: &ast::ComputedMemberExpression,
) -> Result<Expression, LowerError> {
    lower_computed_member_expr(member)
}

pub fn lower_private_field_expr_pub(
    member: &ast::PrivateFieldExpression,
) -> Result<Expression, LowerError> {
    lower_private_field_expr(member)
}

pub fn lower_call_expr_pub(call: &ast::CallExpression) -> Result<Expression, LowerError> {
    lower_call_expr(call)
}

pub fn lower_new_expr_pub(new_expr: &ast::NewExpression) -> Result<Expression, LowerError> {
    lower_new_expr(new_expr)
}

pub fn lower_seq_expr_pub(seq: &ast::SequenceExpression) -> Result<Expression, LowerError> {
    lower_seq_expr(seq)
}

pub fn lower_cond_expr_pub(cond: &ast::ConditionalExpression) -> Result<Expression, LowerError> {
    lower_cond_expr(cond)
}

// ─── Assignment Target Lowering ───────────────────────────────────────────────

pub fn lower_assignment_target(target: &ast::AssignmentTarget) -> Result<Expression, LowerError> {
    if let Some(binding) = crate::lower::pattern::lower_assignment_target_to_binding(target) {
        return Ok(crate::lower::pattern::binding_to_expr(binding));
    }
    match target {
        ast::AssignmentTarget::AssignmentTargetIdentifier(ident) => {
            Ok(Expression::Identifier(ident.name.as_str().to_string()))
        }
        ast::AssignmentTarget::StaticMemberExpression(sm) => {
            let obj = lower_expr_inner(&sm.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Ident(sm.property.name.as_str().to_string()),
                computed: false,
            })
        }
        ast::AssignmentTarget::ComputedMemberExpression(cm) => {
            let obj = lower_expr_inner(&cm.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Computed(Box::new(lower_expr_inner(&cm.expression)?)),
                computed: true,
            })
        }
        ast::AssignmentTarget::PrivateFieldExpression(pf) => {
            let obj = lower_expr_inner(&pf.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: private_field_prop_key(pf.field.name.as_str()),
                computed: false,
            })
        }
        ast::AssignmentTarget::TSAsExpression(e) => lower_expr_inner(&e.expression),
        ast::AssignmentTarget::TSSatisfiesExpression(e) => lower_expr_inner(&e.expression),
        ast::AssignmentTarget::TSNonNullExpression(e) => lower_expr_inner(&e.expression),
        ast::AssignmentTarget::TSTypeAssertion(e) => lower_expr_inner(&e.expression),
        ast::AssignmentTarget::TSInstantiationExpression(e) => lower_expr_inner(&e.expression),
        _ => Err(LowerError::new("Unsupported assignment target")),
    }
}

pub fn lower_simple_assignment_target(
    target: &ast::SimpleAssignmentTarget,
) -> Result<Expression, LowerError> {
    match target {
        ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
            Ok(Expression::Identifier(ident.name.as_str().to_string()))
        }
        ast::SimpleAssignmentTarget::StaticMemberExpression(sm) => {
            let obj = lower_expr_inner(&sm.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Ident(sm.property.name.as_str().to_string()),
                computed: false,
            })
        }
        ast::SimpleAssignmentTarget::ComputedMemberExpression(cm) => {
            let obj = lower_expr_inner(&cm.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: PropertyKey::Computed(Box::new(lower_expr_inner(&cm.expression)?)),
                computed: true,
            })
        }
        ast::SimpleAssignmentTarget::PrivateFieldExpression(pf) => {
            let obj = lower_expr_inner(&pf.object)?;
            Ok(Expression::Member {
                object: Box::new(obj),
                property: private_field_prop_key(pf.field.name.as_str()),
                computed: false,
            })
        }
        ast::SimpleAssignmentTarget::TSAsExpression(e) => lower_expr_inner(&e.expression),
        ast::SimpleAssignmentTarget::TSSatisfiesExpression(e) => lower_expr_inner(&e.expression),
        ast::SimpleAssignmentTarget::TSNonNullExpression(e) => lower_expr_inner(&e.expression),
        ast::SimpleAssignmentTarget::TSTypeAssertion(e) => lower_expr_inner(&e.expression),
        ast::SimpleAssignmentTarget::TSInstantiationExpression(e) => {
            lower_expr_inner(&e.expression)
        }
    }
}
