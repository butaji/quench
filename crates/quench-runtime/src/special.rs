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
        Expression::LogicalExpression(value) => {
            crate::logical::reduce_expression(value, ops, facts, next_register, locals)
        }
        Expression::FunctionExpression(value) => {
            crate::functions::reduce_expression(value, ops, facts, next_register, locals)
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

fn reduce_secondary(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    match expression {
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => {
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
            crate::loops::reduce_update(value, ops, next_register, locals)
        }
        Expression::AssignmentExpression(value) => {
            crate::reduce::reduce_assignment(value, ops, facts, next_register, locals)
        }
        _ => None,
    }
}
