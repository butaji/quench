use std::{cell::RefCell, collections::HashMap};

use oxc::ast::ast::TemplateLiteral;

use crate::{
    facts::ProgramDb,
    ops::{BinaryOp, Constant, Op, UnaryOp},
    register_file::RegisterFile,
};

thread_local! {
    static TAGGED_TEMPLATE_CACHE:
        RefCell<HashMap<(crate::ops::RealmId, u64), crate::value::Value>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn reset_tagged_template_cache() {
    TAGGED_TEMPLATE_CACHE.with(|cache| cache.borrow_mut().clear());
}

pub(crate) fn execute_tagged_template(
    registers: &mut RegisterFile,
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    let crate::ops::Op::TemplateObject {
        dst,
        cooked,
        raw,
        site,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let key = (crate::vm::current_context_or_default().realm(), *site);
    if let Some(value) = TAGGED_TEMPLATE_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        crate::execute::write_value(registers, *dst, value);
        return Ok(());
    }
    let cooked_value = crate::execute::read_register(registers, *cooked)?.clone();
    let raw_value = crate::execute::read_register(registers, *raw)?.clone();
    let raw_value = crate::properties::integrity_apply(Some(&raw_value), true)?;
    let descriptor = vec![
        ("value".to_string(), raw_value),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        (
            "enumerable".to_string(),
            crate::value::Value::Boolean(false),
        ),
        (
            "configurable".to_string(),
            crate::value::Value::Boolean(true),
        ),
    ];
    let cooked_value = crate::builtins::define_own_property(&cooked_value, "raw", &descriptor)?;
    let cooked_value = crate::properties::integrity_apply(Some(&cooked_value), true)?;
    TAGGED_TEMPLATE_CACHE.with(|cache| {
        cache.borrow_mut().insert(key, cooked_value.clone());
    });
    crate::execute::write_value(registers, *dst, cooked_value);
    Ok(())
}

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
