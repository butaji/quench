use std::collections::HashMap;

use oxc::ast::ast::TemplateLiteral;

use crate::{
    facts::ProgramDb,
    ops::{BinaryOp, Constant, Op, UnaryOp},
};

pub(crate) fn reduce(
    template: &TemplateLiteral<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let first = template.quasis.first()?.value.cooked.as_ref()?.to_string();
    let mut result = emit_string(&first, ops, facts, next_register);
    for (index, expression) in template.expressions.iter().enumerate() {
        let value =
            crate::reduce::reduce_expression(expression, ops, facts, next_register, locals)?;
        let string = *next_register;
        *next_register = next_register.saturating_add(1);
        ops.push(Op::Unary {
            dst: string,
            operator: UnaryOp::ToString,
            src: value,
        });
        result = emit_binary(result, string, ops, next_register);
        let quasi = template
            .quasis
            .get(index + 1)?
            .value
            .cooked
            .as_ref()?
            .to_string();
        let suffix = emit_string(&quasi, ops, facts, next_register);
        result = emit_binary(result, suffix, ops, next_register);
    }
    Some(result)
}

fn emit_string(value: &str, ops: &mut Vec<Op>, _facts: &mut ProgramDb, next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    ops.push(Op::Const {
        dst: register,
        value: Constant::String(value.to_string()),
    });
    register
}

fn emit_binary(left: u16, right: u16, ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let register = *next;
    *next = next.saturating_add(1);
    ops.push(Op::Binary {
        dst: register,
        operator: BinaryOp::Add,
        lhs: left,
        rhs: right,
    });
    register
}
