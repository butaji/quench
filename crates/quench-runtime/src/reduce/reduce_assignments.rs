use std::collections::HashMap;

use oxc::ast::ast::{AssignmentOperator, AssignmentTarget};

use crate::{facts::ProgramDb, literal::reduce_operator, ops::Op, properties};

pub fn reduce_assignment(
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let AssignmentTarget::AssignmentTargetIdentifier(identifier) = &assignment.left else {
        return properties::reduce_assignment(assignment, ops, facts, next, locals);
    };
    let Some(slot) = locals.get(identifier.name.as_str()).copied() else {
        return reduce_unresolved(identifier, assignment, ops, facts, next, locals);
    };
    let lhs = local_lhs(assignment.operator, slot, ops, next);
    let rhs = crate::reduce::reduce_expression(&assignment.right, ops, facts, next, locals)?;
    let value = assignment_value(assignment.operator, lhs, rhs, ops, next)?;
    ops.push(Op::StoreLocal { slot, src: value });
    Some(value)
}

fn reduce_unresolved(
    identifier: &oxc::ast::ast::IdentifierReference<'_>,
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let lhs = (assignment.operator != AssignmentOperator::Assign)
        .then(|| crate::identifiers::reduce(identifier, ops, facts, next, locals))
        .flatten();
    let rhs = crate::reduce::reduce_expression(&assignment.right, ops, facts, next, locals)?;
    let value = assignment_value(assignment.operator, lhs, rhs, ops, next)?;
    ops.push(Op::SetName {
        key: identifier.name.to_string(),
        src: value,
        strict: facts.strict,
    });
    Some(value)
}

fn assignment_value(
    assignment: AssignmentOperator,
    lhs: Option<u16>,
    rhs: u16,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> Option<u16> {
    if assignment == AssignmentOperator::Assign {
        return Some(rhs);
    }
    binary_value(assignment, lhs?, rhs, ops, next)
}

fn local_lhs(
    assignment: AssignmentOperator,
    slot: u16,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> Option<u16> {
    if assignment == AssignmentOperator::Assign {
        return None;
    }
    let lhs = take_register(next);
    ops.push(Op::LoadLocal { dst: lhs, slot });
    Some(lhs)
}

fn binary_value(
    assignment: AssignmentOperator,
    lhs: u16,
    rhs: u16,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> Option<u16> {
    let dst = take_register(next);
    let operator = reduce_operator(assignment.to_binary_operator()?)?;
    ops.push(Op::Binary {
        dst,
        operator,
        lhs,
        rhs,
    });
    Some(dst)
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}
