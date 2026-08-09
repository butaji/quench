use std::collections::HashMap;

use oxc::syntax::operator::LogicalOperator;

use crate::{facts::ProgramDb, literal::reduce_literal, ops::Constant, ops::Op};

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
    if matches!(logical.operator, LogicalOperator::Coalesce) {
        return None;
    }
    let left = crate::reduce::reduce_expression(&logical.left, ops, facts, next_register, locals)?;
    let mut right_ops = Vec::new();
    let right = crate::reduce::reduce_expression(
        &logical.right,
        &mut right_ops,
        facts,
        next_register,
        locals,
    )?;
    let (consequent, alternate) = dynamic_branches(logical.operator, left, right, right_ops)?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Conditional {
        dst,
        condition: left,
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
        LogicalOperator::Coalesce => return None,
    }
    Some((consequent, alternate))
}

fn is_truthy(value: &Constant) -> bool {
    match value {
        Constant::Boolean(value) => *value,
        Constant::Number(value) => *value != 0.0 && !value.is_nan(),
        Constant::String(value) => !value.is_empty(),
        Constant::Null | Constant::Undefined => false,
    }
}
