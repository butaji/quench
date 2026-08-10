use std::collections::HashMap;

use crate::{facts::ProgramDb, ops::Op};

pub(super) fn reduce(
    tagged: &oxc::ast::ast::TaggedTemplateExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let callee = super::reduce_expression(&tagged.tag, ops, facts, next_register, locals)?;
    let cooked = reduce_parts(&tagged.quasi, true, ops, next_register)?;
    let raw = reduce_parts(&tagged.quasi, false, ops, next_register)?;
    ops.push(Op::SetProperty {
        object: cooked,
        key: "raw".to_string(),
        src: raw,
    });
    let mut args = vec![cooked];
    for expression in &tagged.quasi.expressions {
        args.push(super::reduce_expression(
            expression,
            ops,
            facts,
            next_register,
            locals,
        )?);
    }
    let spreads = vec![false; args.len()];
    let dst = take_register(next_register);
    ops.push(Op::Call {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}

fn reduce_parts(
    template: &oxc::ast::ast::TemplateLiteral<'_>,
    cooked: bool,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
) -> Option<u16> {
    let mut elements = Vec::with_capacity(template.quasis.len());
    for quasi in &template.quasis {
        let value = if cooked {
            quasi.value.cooked.as_ref()?.as_str()
        } else {
            quasi.value.raw.as_str()
        };
        elements.push(emit_string(ops, next_register, value));
    }
    let dst = take_register(next_register);
    ops.push(Op::MakeArray { dst, elements });
    Some(dst)
}

pub(super) fn emit_string(ops: &mut Vec<Op>, next_register: &mut u16, value: &str) -> u16 {
    let dst = take_register(next_register);
    ops.push(Op::Const {
        dst,
        value: crate::ops::Constant::String(value.to_string()),
    });
    dst
}

fn take_register(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}
