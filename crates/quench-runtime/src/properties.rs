use std::collections::HashMap;

use oxc::ast::ast::Expression;

use crate::{facts::ProgramDb, literal::reduce_literal, ops::Op};

pub(crate) fn reduce(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (object, key) = match expression {
        Expression::StaticMemberExpression(member) => {
            (&member.object, member.property.name.to_string())
        }
        Expression::ComputedMemberExpression(member) => {
            let literal = reduce_literal(&member.expression)?;
            (&member.object, property_key(&literal.op)?)
        }
        _ => return None,
    };
    let object = crate::reduce::reduce_expression(object, ops, facts, next_register, locals)?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetProperty {
        dst: register,
        object,
        key,
    });
    Some(register)
}

fn property_key(value: &crate::ops::Constant) -> Option<String> {
    match value {
        crate::ops::Constant::String(value) => Some(value.clone()),
        crate::ops::Constant::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

pub(crate) fn reduce_assignment(
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (object, key) = match &assignment.left {
        oxc::ast::ast::AssignmentTarget::StaticMemberExpression(member) => {
            (&member.object, member.property.name.to_string())
        }
        oxc::ast::ast::AssignmentTarget::ComputedMemberExpression(member) => {
            let key = property_key(&reduce_literal(&member.expression)?.op)?;
            (&member.object, key)
        }
        _ => return None,
    };
    if assignment.operator != oxc::syntax::operator::AssignmentOperator::Assign {
        return None;
    }
    let object = crate::reduce::reduce_expression(object, ops, facts, next_register, locals)?;
    let value =
        crate::reduce::reduce_expression(&assignment.right, ops, facts, next_register, locals)?;
    ops.push(Op::SetProperty {
        object,
        key,
        src: value,
    });
    Some(value)
}

pub(crate) fn reduce_method_call(
    call: &oxc::ast::ast::CallExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (object, key) = match &call.callee {
        Expression::StaticMemberExpression(member) => {
            (&member.object, member.property.name.to_string())
        }
        Expression::ComputedMemberExpression(member) => {
            let key = property_key(&reduce_literal(&member.expression)?.op)?;
            (&member.object, key)
        }
        _ => return None,
    };
    let object = crate::reduce::reduce_expression(object, ops, facts, next_register, locals)?;
    let mut args = Vec::new();
    for argument in &call.arguments {
        args.push(crate::reduce::reduce_expression(
            argument.as_expression()?,
            ops,
            facts,
            next_register,
            locals,
        )?);
    }
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::CallMethod {
        dst,
        object,
        key,
        args,
    });
    Some(dst)
}
