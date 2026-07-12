//! Optional chaining lowering
//!
//! Converts swc optional chain expressions to conditional expressions.

use super::expr::{lower_expr, lower_member_prop};
use super::helpers::{atom_to_string, LowerError};
use crate::ast::{BinaryOp, Expression};
use swc_ecma_ast as swc;

/// Lower optional chaining expression
pub fn lower_opt_chain(opt_chain: &swc::OptChainExpr) -> Result<Expression, LowerError> {
    let base_expr = match &*opt_chain.base {
        swc::OptChainBase::Member(member) => lower_expr(&member.obj)?,
        swc::OptChainBase::Call(opt_call) => match &*opt_call.callee {
            swc::Expr::Member(member) => lower_expr(&member.obj)?,
            swc::Expr::Ident(ident) => Expression::Identifier(atom_to_string(&ident.sym)),
            _ => return Err(LowerError::new("Unsupported optional call base")),
        },
    };
    process_opt_chain_expr(opt_chain, base_expr)
}

fn process_opt_chain_expr(
    expr: &swc::OptChainExpr,
    base_expr: Expression,
) -> Result<Expression, LowerError> {
    match &*expr.base {
        swc::OptChainBase::Member(member) => process_opt_chain_member(member, base_expr),
        swc::OptChainBase::Call(opt_call) => process_opt_chain_call(opt_call, base_expr),
    }
}

fn process_opt_chain_member(
    member: &swc::MemberExpr,
    base_expr: Expression,
) -> Result<Expression, LowerError> {
    let (property, computed) = lower_member_prop(&member.prop)?;
    let member_expr = Expression::Member {
        object: Box::new(base_expr.clone()),
        property,
        computed,
    };
    make_optional_check(base_expr, member_expr)
}

fn process_opt_chain_call(
    opt_call: &swc::OptCall,
    base_expr: Expression,
) -> Result<Expression, LowerError> {
    match &*opt_call.callee {
        swc::Expr::OptChain(nested) => {
            let inner = process_opt_chain_expr(nested, base_expr)?;
            let args = lower_call_args(opt_call);
            Ok(Expression::Call {
                callee: Box::new(inner),
                arguments: args,
            })
        }
        swc::Expr::Member(member) => process_opt_chain_member_call(member, opt_call, base_expr),
        swc::Expr::Ident(ident) => {
            let args = lower_call_args(opt_call);
            let callee = Expression::Identifier(atom_to_string(&ident.sym));
            let call_expr = Expression::Call {
                callee: Box::new(callee),
                arguments: args,
            };
            make_optional_check(base_expr, call_expr)
        }
        _ => Err(LowerError::new("Unsupported optional call callee")),
    }
}

fn process_opt_chain_member_call(
    member: &swc::MemberExpr,
    opt_call: &swc::OptCall,
    base_expr: Expression,
) -> Result<Expression, LowerError> {
    let inner_obj = lower_expr(&member.obj)?;
    let (property, computed) = lower_member_prop(&member.prop)?;
    let inner_checked = make_optional_check(
        inner_obj,
        Expression::Member {
            object: Box::new(base_expr.clone()),
            property,
            computed,
        },
    )?;
    let args = lower_call_args(opt_call);
    let call_expr = Expression::Call {
        callee: Box::new(inner_checked),
        arguments: args,
    };
    make_optional_check(base_expr, call_expr)
}

fn lower_call_args(opt_call: &swc::OptCall) -> Vec<Expression> {
    opt_call
        .args
        .iter()
        .filter_map(|arg| lower_expr(&arg.expr).ok())
        .collect()
}

fn make_optional_check(obj: Expression, expr: Expression) -> Result<Expression, LowerError> {
    let null_check = Expression::Binary {
        op: BinaryOp::Or,
        left: Box::new(Expression::Binary {
            op: BinaryOp::StrictEq,
            left: Box::new(obj.clone()),
            right: Box::new(Expression::Null),
        }),
        right: Box::new(Expression::Binary {
            op: BinaryOp::StrictEq,
            left: Box::new(obj),
            right: Box::new(Expression::Undefined),
        }),
    };
    Ok(Expression::Conditional {
        condition: Box::new(null_check),
        consequent: Box::new(Expression::Undefined),
        alternate: Box::new(expr),
    })
}
