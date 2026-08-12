use crate::{facts::ProgramDb, ops::Op};
use oxc::ast::ast::Expression;
use std::collections::HashMap;

pub(crate) fn reduce(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    reduce_primary(expression, ops, facts, next_register, locals)
        .or_else(|| reduce_secondary(expression, ops, facts, next_register, locals))
}

fn reduce_primary(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    match expression {
        Expression::ChainExpression(chain) => {
            reduce_chain(&chain.expression, ops, facts, next_register, locals)
        }
        Expression::LogicalExpression(value) => {
            crate::logical::reduce_expression(value, ops, facts, next_register, locals)
        }
        Expression::FunctionExpression(value) => {
            crate::functions::reduce_expression(value, ops, facts, next_register, locals)
        }
        Expression::ClassExpression(value) => {
            crate::classes::reduce_expression(value, ops, facts, next_register, locals)
        }
        Expression::ArrowFunctionExpression(value) => {
            crate::functions::reduce_arrow(value, ops, facts, next_register, locals)
        }
        Expression::ObjectExpression(value) => {
            crate::objects::reduce(value, ops, facts, next_register, locals)
        }
        Expression::TemplateLiteral(value) => {
            crate::templates::reduce(value, ops, facts, next_register, locals)
        }
        Expression::SequenceExpression(value) => {
            crate::sequences::reduce(value, ops, facts, next_register, locals)
        }
        _ => None,
    }
}

fn reduce_chain(
    chain: &oxc::ast::ast::ChainElement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    use oxc::ast::ast::ChainElement;
    match chain {
        ChainElement::CallExpression(call) => {
            reduce_optional_call(call, ops, facts, next_register, locals)
        }
        ChainElement::StaticMemberExpression(member) => {
            reduce_static_chain(member, ops, facts, next_register, locals)
        }
        ChainElement::ComputedMemberExpression(member) => {
            reduce_computed_chain(member, ops, facts, next_register, locals)
        }
        _ => None,
    }
}

fn reduce_static_chain(
    member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let object = crate::reduce::reduce_expression(&member.object, ops, facts, next, locals)?;
    let dst = *next;
    *next = next.saturating_add(1);
    ops.push(Op::OptionalGet {
        dst,
        object,
        key: member.property.name.to_string(),
    });
    Some(dst)
}

fn reduce_computed_chain(
    member: &oxc::ast::ast::ComputedMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let object = crate::reduce::reduce_expression(&member.object, ops, facts, next, locals)?;
    let dst = *next;
    *next = next.saturating_add(1);
    if member.optional {
        reduce_guarded_computed_chain(member, object, dst, ops, facts, next, locals)?;
    } else {
        let key = crate::reduce::reduce_expression(&member.expression, ops, facts, next, locals)?;
        ops.push(Op::OptionalGetDynamic { dst, object, key });
    }
    Some(dst)
}

fn reduce_guarded_computed_chain(
    member: &oxc::ast::ast::ComputedMemberExpression<'_>,
    object: u16,
    dst: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let condition = *next;
    *next = next.saturating_add(1);
    ops.push(Op::Unary {
        dst: condition,
        operator: crate::ops::UnaryOp::IsNullish,
        src: object,
    });
    let mut then_ops = vec![Op::Const {
        dst,
        value: crate::ops::Constant::Undefined,
    }];
    let mut else_ops = Vec::new();
    let key =
        crate::reduce::reduce_expression(&member.expression, &mut else_ops, facts, next, locals)?;
    else_ops.push(Op::OptionalGetDynamic { dst, object, key });
    ops.push(Op::Branch {
        condition,
        then_ops: std::mem::take(&mut then_ops),
        else_ops,
    });
    Some(())
}

pub(crate) fn reduce_optional_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (callee, receiver, receiver_guard) =
        reduce_optional_callee(&call.callee, ops, facts, next, locals)?;
    let dst = *next;
    *next = next.saturating_add(1);
    if receiver_guard {
        return reduce_guarded_optional_call(
            call,
            (callee, receiver?, dst),
            ops,
            facts,
            next,
            locals,
        );
    }
    let (args, spreads) = crate::reduce::reduce_expressions::calls_reduce::reduce_arguments(
        &call.arguments,
        ops,
        facts,
        next,
        locals,
    )?;
    ops.push(Op::OptionalCall {
        dst,
        callee,
        receiver,
        guard_receiver: receiver_guard,
        args,
        spreads,
    });
    Some(dst)
}

fn reduce_guarded_optional_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    registers: (u16, u16, u16),
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (callee, receiver, dst) = registers;
    let condition = *next;
    *next = next.saturating_add(1);
    emit_nullish_guard(ops, condition, receiver);
    let mut else_ops = Vec::new();
    let (args, spreads) = crate::reduce::reduce_expressions::calls_reduce::reduce_arguments(
        &call.arguments,
        &mut else_ops,
        facts,
        next,
        locals,
    )?;
    else_ops.push(Op::OptionalCall {
        dst,
        callee,
        receiver: Some(receiver),
        guard_receiver: true,
        args,
        spreads,
    });
    emit_optional_branch(ops, condition, dst, else_ops);
    Some(dst)
}

fn emit_nullish_guard(ops: &mut Vec<Op>, dst: u16, src: u16) {
    ops.push(Op::Unary {
        dst,
        operator: crate::ops::UnaryOp::IsNullish,
        src,
    });
}

fn emit_optional_branch(ops: &mut Vec<Op>, condition: u16, dst: u16, else_ops: Vec<Op>) {
    ops.push(Op::Branch {
        condition,
        then_ops: vec![Op::Const {
            dst,
            value: crate::ops::Constant::Undefined,
        }],
        else_ops,
    });
}

fn reduce_optional_callee(
    callee: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(u16, Option<u16>, bool)> {
    match callee {
        Expression::ParenthesizedExpression(parenthesized) => {
            reduce_optional_callee(&parenthesized.expression, ops, facts, next, locals)
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            oxc::ast::ast::ChainElement::StaticMemberExpression(member) => {
                reduce_static_callee(member, ops, facts, next, locals)
            }
            oxc::ast::ast::ChainElement::ComputedMemberExpression(member) => {
                reduce_computed_callee(member, ops, facts, next, locals)
            }
            _ => reduce_chain_callee_without_static_member(callee, ops, facts, next, locals),
        },
        Expression::StaticMemberExpression(member) => {
            reduce_static_callee(member, ops, facts, next, locals)
        }
        Expression::ComputedMemberExpression(member) => {
            reduce_computed_callee(member, ops, facts, next, locals)
        }
        _ => Some((
            crate::reduce::reduce_expression(callee, ops, facts, next, locals)?,
            None,
            false,
        )),
    }
}

fn reduce_chain_callee_without_static_member(
    callee: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(u16, Option<u16>, bool)> {
    let start = ops.len();
    let value = crate::reduce::reduce_expression(callee, ops, facts, next, locals)?;
    let receiver = ops[start..].iter().rev().find_map(optional_receiver);
    Some((value, receiver, receiver.is_some()))
}

fn optional_receiver(op: &Op) -> Option<u16> {
    match op {
        Op::OptionalGet { object, .. } | Op::OptionalGetDynamic { object, .. } => Some(*object),
        _ => None,
    }
}

fn reduce_static_callee(
    member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(u16, Option<u16>, bool)> {
    if matches!(member.object, Expression::Super(_)) {
        return reduce_super_static_callee(member, ops, next, locals);
    }
    let object = crate::reduce::reduce_expression(&member.object, ops, facts, next, locals)?;
    let callee = *next;
    *next = next.saturating_add(1);
    let op = if member.optional || has_optional_chain_object(&member.object) {
        Op::OptionalGet {
            dst: callee,
            object,
            key: member.property.name.to_string(),
        }
    } else {
        Op::GetProperty {
            dst: callee,
            object,
            key: member.property.name.to_string(),
        }
    };
    ops.push(op);
    Some((
        callee,
        Some(object),
        member.optional || has_optional_chain_object(&member.object),
    ))
}

fn reduce_super_static_callee(
    member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ops: &mut Vec<Op>,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(u16, Option<u16>, bool)> {
    let callee = *next;
    *next = next.saturating_add(1);
    ops.push(Op::GetSuperProperty {
        dst: callee,
        key: member.property.name.to_string(),
    });
    let receiver = emit_this_receiver(ops, next, locals)?;
    Some((callee, Some(receiver), member.optional))
}

fn emit_this_receiver(
    ops: &mut Vec<Op>,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let slot = locals
        .get("this")
        .or_else(|| locals.get("\0script_this"))
        .copied()?;
    let receiver = *next;
    *next = next.saturating_add(1);
    ops.push(Op::LoadLocal {
        dst: receiver,
        slot,
    });
    Some(receiver)
}

fn reduce_computed_callee(
    member: &oxc::ast::ast::ComputedMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(u16, Option<u16>, bool)> {
    let object = crate::reduce::reduce_expression(&member.object, ops, facts, next, locals)?;
    let key = crate::reduce::reduce_expression(&member.expression, ops, facts, next, locals)?;
    let callee = *next;
    *next = next.saturating_add(1);
    let op = if member.optional || has_optional_chain_object(&member.object) {
        Op::OptionalGetDynamic {
            dst: callee,
            object,
            key,
        }
    } else {
        Op::GetPropertyDynamic {
            dst: callee,
            object,
            key,
        }
    };
    ops.push(op);
    Some((
        callee,
        Some(object),
        member.optional || has_optional_chain_object(&member.object),
    ))
}

fn has_optional_chain_object(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ChainExpression(_) => true,
        Expression::ParenthesizedExpression(parenthesized) => {
            has_optional_chain_object(&parenthesized.expression)
        }
        Expression::StaticMemberExpression(member) => {
            member.optional || has_optional_chain_object(&member.object)
        }
        Expression::ComputedMemberExpression(member) => {
            member.optional || has_optional_chain_object(&member.object)
        }
        _ => false,
    }
}

fn reduce_secondary(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    match expression {
        Expression::StaticMemberExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::PrivateFieldExpression(_) => {
            crate::properties::reduce(expression, ops, facts, next_register, locals)
        }
        Expression::ConditionalExpression(value) => {
            crate::conditional::reduce_expression(value, ops, facts, next_register, locals)
        }
        Expression::UnaryExpression(value) => {
            crate::reduce::reduce_unary(value, ops, facts, next_register, locals)
        }
        Expression::CallExpression(value) => {
            crate::reduce::reduce_call(value, ops, facts, next_register, locals)
        }
        Expression::NewExpression(value) => {
            crate::construct::reduce(value, ops, facts, next_register, locals)
        }
        Expression::UpdateExpression(value) => {
            crate::loops::reduce_update(value, ops, facts, next_register, locals)
        }
        Expression::YieldExpression(value) => {
            crate::generator::reduce_yield(value, ops, facts, next_register, locals)
        }
        Expression::AssignmentExpression(value) => {
            crate::reduce::reduce_assignment(value, ops, facts, next_register, locals)
        }
        _ => None,
    }
}
