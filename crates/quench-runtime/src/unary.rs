use std::collections::HashMap;

use oxc::ast::ast::Expression;

use crate::{facts::ProgramDb, ops::Constant, ops::Op};

pub(crate) fn is_unresolved_identifier(
    expression: &Expression<'_>,
    locals: &HashMap<String, u16>,
) -> bool {
    match expression {
        Expression::Identifier(identifier) => {
            !locals.contains_key(identifier.name.as_str())
                && !crate::globals::is_defined(identifier.name.as_str())
        }
        Expression::ParenthesizedExpression(parenthesized) => {
            is_unresolved_identifier(&parenthesized.expression, locals)
        }
        _ => false,
    }
}

pub(crate) fn reduce_delete(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if let Some(result) = reduce_eval_delete(expression, ops, facts, next_register) {
        return Some(result);
    }
    if !is_member_expression(expression) {
        return Some(emit_constant(
            ops,
            next_register,
            !matches!(expression, Expression::Identifier(_)),
        ));
    }
    let (object, key) = match expression {
        Expression::ComputedMemberExpression(member) => (
            crate::reduce::reduce_expression(&member.object, ops, facts, next_register, locals)?,
            crate::reduce::reduce_expression(
                &member.expression,
                ops,
                facts,
                next_register,
                locals,
            )?,
        ),
        Expression::StaticMemberExpression(member) => {
            reduce_static_delete_member(member, ops, facts, next_register, locals)?
        }
        _ => return None,
    };
    push_delete_property(ops, facts, next_register, object, key)
}

fn push_delete_property(
    ops: &mut Vec<Op>,
    facts: &ProgramDb,
    next_register: &mut u16,
    object: u16,
    key: u16,
) -> Option<u16> {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::DeleteProperty {
        dst,
        object,
        key,
        strict: facts.strict,
    });
    Some(dst)
}

fn is_member_expression(expression: &Expression<'_>) -> bool {
    matches!(
        expression,
        Expression::ComputedMemberExpression(_)
            | Expression::StaticMemberExpression(_)
            | Expression::PrivateFieldExpression(_)
    )
}

fn reduce_static_delete_member(
    member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<(u16, u16)> {
    let name = member.property.name.to_string();
    let object =
        crate::reduce::reduce_expression(&member.object, ops, facts, next_register, locals)?;
    let key = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: key,
        value: Constant::String(name),
    });
    Some((object, key))
}

fn reduce_eval_delete(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &ProgramDb,
    next: &mut u16,
) -> Option<u16> {
    let Expression::Identifier(identifier) = expression else {
        return None;
    };
    let (name, slot) = facts
        .eval_deletable
        .iter()
        .find(|(name, _)| name == identifier.name.as_str())?;
    let dst = *next;
    *next = next.saturating_add(1);
    ops.push(Op::DeleteEvalBinding {
        dst,
        name: name.clone(),
        slot: *slot,
    });
    Some(dst)
}

fn emit_constant(ops: &mut Vec<Op>, next_register: &mut u16, value: bool) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst,
        value: Constant::Boolean(value),
    });
    dst
}
