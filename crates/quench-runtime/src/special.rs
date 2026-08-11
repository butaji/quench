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
    if let ChainElement::CallExpression(call) = chain {
        return reduce_optional_call(call, ops, facts, next_register, locals);
    }
    let (object, key) = match chain {
        ChainElement::StaticMemberExpression(member) => {
            (&member.object, member.property.name.to_string())
        }
        _ => return None,
    };
    let object = crate::reduce::reduce_expression(object, ops, facts, next_register, locals)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::OptionalGet { dst, object, key });
    Some(dst)
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
    let (args, spreads) = crate::reduce::reduce_expressions::calls_reduce::reduce_arguments(
        &call.arguments,
        ops,
        facts,
        next,
        locals,
    )?;
    let dst = *next;
    *next = next.saturating_add(1);
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

fn reduce_optional_callee(
    callee: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(u16, Option<u16>, bool)> {
    match callee {
        Expression::StaticMemberExpression(member) => {
            let object =
                crate::reduce::reduce_expression(&member.object, ops, facts, next, locals)?;
            let callee = *next;
            *next = next.saturating_add(1);
            if member.optional {
                ops.push(Op::OptionalGet {
                    dst: callee,
                    object,
                    key: member.property.name.to_string(),
                });
            } else {
                ops.push(Op::GetProperty {
                    dst: callee,
                    object,
                    key: member.property.name.to_string(),
                });
            }
            Some((callee, Some(object), member.optional))
        }
        _ => Some((
            crate::reduce::reduce_expression(callee, ops, facts, next, locals)?,
            None,
            false,
        )),
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
