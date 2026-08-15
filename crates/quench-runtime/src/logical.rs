use std::collections::HashMap;

use oxc::syntax::operator::AssignmentOperator;
use oxc::syntax::operator::LogicalOperator;

use crate::{facts::ProgramDb, literal::reduce_literal, ops::Constant, ops::Op};

pub(crate) fn reduce_assignment(
    assignment: &oxc::ast::ast::AssignmentExpression<'_>,
    mut place: crate::reduce::reduce_assignments::Place,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let operator = assignment.operator;
    crate::reduce::reduce_assignments::prepare_get(&mut place, ops, next);
    let left = crate::reduce::reduce_assignments::get(&place, ops, next)?;
    let condition = assignment_condition(operator, left, ops, next)?;
    let mut right_ops = Vec::new();
    let right =
        crate::reduce::reduce_expression(&assignment.right, &mut right_ops, facts, next, locals)?;
    crate::reduce::reduce_assignments::infer_assignment_name(
        assignment,
        right,
        facts,
        &mut right_ops,
    );
    crate::reduce::reduce_assignments::put(place, right, &mut right_ops)?;
    right_ops.push(Op::Return { src: right });
    let left_ops = vec![Op::Return { src: left }];
    let (consequent, alternate) = assignment_branches(operator, right_ops, left_ops)?;
    let dst = take_register(next);
    let mut branches = crate::machine::FunctionCode::from_ops_many(vec![consequent, alternate]);
    let alternate = branches.pop()?;
    let consequent = branches.pop()?;
    ops.push(Op::Conditional {
        dst,
        condition,
        consequent,
        alternate,
    });
    Some(dst)
}

fn assignment_condition(
    operator: AssignmentOperator,
    left: u16,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> Option<u16> {
    if operator != AssignmentOperator::LogicalNullish {
        return Some(left);
    }
    Some(nullish_condition(left, ops, next))
}

fn nullish_condition(left: u16, ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let null = take_register(next);
    ops.push(Op::Const {
        dst: null,
        value: crate::ops::Constant::Null,
    });
    let dst = take_register(next);
    ops.push(Op::Binary {
        dst,
        operator: crate::ops::BinaryOp::Equal,
        lhs: left,
        rhs: null,
    });
    dst
}

fn assignment_branches(
    operator: AssignmentOperator,
    right: Vec<Op>,
    left: Vec<Op>,
) -> Option<(Vec<Op>, Vec<Op>)> {
    match operator {
        AssignmentOperator::LogicalAnd | AssignmentOperator::LogicalNullish => Some((right, left)),
        AssignmentOperator::LogicalOr => Some((left, right)),
        _ => None,
    }
}

fn take_register(next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    register
}

pub(crate) fn reduce_expression(
    logical: &oxc::ast::ast::LogicalExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let Some(literal) = reduce_literal(&logical.left) else {
        return reduce_dynamic(logical, ops, facts, next_register, locals);
    };
    let left = crate::reduce::reduce_expression(&logical.left, ops, facts, next_register, locals)?;
    let use_left = match logical.operator {
        LogicalOperator::Or => is_truthy(&literal.op),
        LogicalOperator::And => !is_truthy(&literal.op),
        LogicalOperator::Coalesce => !matches!(literal.op, Constant::Null),
    };
    if use_left {
        Some(left)
    } else {
        crate::reduce::reduce_expression(&logical.right, ops, facts, next_register, locals)
    }
}

fn reduce_dynamic(
    logical: &oxc::ast::ast::LogicalExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let left = crate::reduce::reduce_expression(&logical.left, ops, facts, next_register, locals)?;
    let mut right_ops = Vec::new();
    let right = crate::reduce::reduce_expression(
        &logical.right,
        &mut right_ops,
        facts,
        next_register,
        locals,
    )?;
    let condition = if matches!(logical.operator, LogicalOperator::Coalesce) {
        nullish_condition(left, ops, next_register)
    } else {
        left
    };
    let (consequent, alternate) = dynamic_branches(logical.operator, left, right, right_ops)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    let mut branches = crate::machine::FunctionCode::from_ops_many(vec![consequent, alternate]);
    let alternate = branches.pop()?;
    let consequent = branches.pop()?;
    ops.push(Op::Conditional {
        dst,
        condition,
        consequent,
        alternate,
    });
    Some(dst)
}

fn dynamic_branches(
    operator: LogicalOperator,
    left: u16,
    right: u16,
    mut right_ops: Vec<Op>,
) -> Option<(Vec<Op>, Vec<Op>)> {
    let mut consequent = Vec::new();
    let mut alternate = Vec::new();
    match operator {
        LogicalOperator::Or => {
            consequent.push(Op::Return { src: left });
            right_ops.push(Op::Return { src: right });
            alternate = right_ops;
        }
        LogicalOperator::And => {
            right_ops.push(Op::Return { src: right });
            consequent = right_ops;
            alternate.push(Op::Return { src: left });
        }
        LogicalOperator::Coalesce => {
            right_ops.push(Op::Return { src: right });
            consequent = right_ops;
            alternate.push(Op::Return { src: left });
        }
    }
    Some((consequent, alternate))
}

fn is_truthy(value: &Constant) -> bool {
    match value {
        Constant::Boolean(value) => *value,
        Constant::Number(value) => *value != 0.0 && !value.is_nan(),
        Constant::String(value) => !value.is_empty(),
        Constant::BigInt(value) => value != "0",
        Constant::Null | Constant::Undefined => false,
        Constant::StringUnits(value) => value.is_empty(),
    }
}
