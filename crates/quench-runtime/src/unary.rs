use std::collections::HashMap;

use oxc::ast::ast::Expression;

use crate::{facts::ProgramDb, ops::Constant, ops::Op};

pub(crate) fn reduce_delete(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let Expression::ComputedMemberExpression(member) = expression else {
        return Some(emit_constant(
            ops,
            next_register,
            !matches!(expression, Expression::Identifier(_)),
        ));
    };
    let object =
        crate::reduce::reduce_expression(&member.object, ops, facts, next_register, locals)?;
    let key =
        crate::reduce::reduce_expression(&member.expression, ops, facts, next_register, locals)?;
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

fn emit_constant(ops: &mut Vec<Op>, next_register: &mut u16, value: bool) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst,
        value: Constant::Boolean(value),
    });
    dst
}
