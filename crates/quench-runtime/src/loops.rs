use std::collections::HashMap;

mod counted_for;

use oxc::ast::ast::{DoWhileStatement, Expression, ForInStatement, ForOfStatement, WhileStatement};

use crate::{
    facts::ProgramDb,
    ops::{Constant, Op},
};

use counted_for::{reduce_body, reduce_fragment};

pub(crate) use counted_for::reduce_for;

pub(crate) fn reduce_update(
    update: &oxc::ast::ast::UpdateExpression<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let oxc::ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) =
        &update.argument
    else {
        return reduce_member_update(update, ops, next_register);
    };
    let slot = *locals.get(identifier.name.as_str())?;
    let old = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::LoadLocal { dst: old, slot });
    let one = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: one,
        value: Constant::Number(1.0),
    });
    let updated = *next_register;
    *next_register = next_register.saturating_add(1);
    let operator = match update.operator {
        oxc::syntax::operator::UpdateOperator::Increment => crate::ops::BinaryOp::Add,
        oxc::syntax::operator::UpdateOperator::Decrement => crate::ops::BinaryOp::Subtract,
    };
    ops.push(Op::Binary {
        dst: updated,
        operator,
        lhs: old,
        rhs: one,
    });
    ops.push(Op::StoreLocal { slot, src: updated });
    Some(if update.prefix { updated } else { old })
}

fn reduce_member_update(
    update: &oxc::ast::ast::UpdateExpression<'_>,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
) -> Option<u16> {
    let member = this_static_member(update)?;
    let object = crate::reduce_support::emit_undefined(ops, next_register);
    let key = member.property.name.to_string();
    let old = emit_member_load(ops, next_register, object, &key);
    let one = emit_one(ops, next_register);
    let updated = emit_member_update_value(ops, next_register, update, old, one);
    ops.push(Op::SetProperty {
        object,
        key,
        src: updated,
    });
    Some(if update.prefix { updated } else { old })
}

fn this_static_member<'a>(
    update: &'a oxc::ast::ast::UpdateExpression<'_>,
) -> Option<&'a oxc::ast::ast::StaticMemberExpression<'a>> {
    let oxc::ast::ast::SimpleAssignmentTarget::StaticMemberExpression(member) = &update.argument
    else {
        return None;
    };
    matches!(member.object, Expression::ThisExpression(_)).then_some(member)
}

fn emit_member_load(ops: &mut Vec<Op>, next_register: &mut u16, object: u16, key: &str) -> u16 {
    let old = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::GetProperty {
        dst: old,
        object,
        key: key.to_string(),
    });
    old
}

fn emit_one(ops: &mut Vec<Op>, next_register: &mut u16) -> u16 {
    let one = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: one,
        value: Constant::Number(1.0),
    });
    one
}

fn emit_member_update_value(
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    update: &oxc::ast::ast::UpdateExpression<'_>,
    old: u16,
    one: u16,
) -> u16 {
    let updated = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Binary {
        dst: updated,
        operator: update_operator(update.operator),
        lhs: old,
        rhs: one,
    });
    updated
}

fn update_operator(operator: oxc::syntax::operator::UpdateOperator) -> crate::ops::BinaryOp {
    match operator {
        oxc::syntax::operator::UpdateOperator::Increment => crate::ops::BinaryOp::Add,
        oxc::syntax::operator::UpdateOperator::Decrement => crate::ops::BinaryOp::Subtract,
    }
}

pub(crate) fn reduce_for_in(
    statement: &ForInStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let slot = for_in_slot(&statement.left, next_slot, locals)?;
    let object =
        crate::reduce::reduce_expression(&statement.right, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported for-in object".to_string()])?;
    let body = crate::branch::reduce(&statement.body, facts, locals)?;
    ops.push(Op::ForIn {
        label: None,
        object,
        slot,
        body,
    });
    Ok(())
}

pub(crate) fn reduce_for_of(
    statement: &ForOfStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let slot = for_in_slot(&statement.left, next_slot, locals)?;
    let iterable =
        crate::reduce::reduce_expression(&statement.right, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported for-of iterable".to_string()])?;
    let body = crate::branch::reduce(&statement.body, facts, locals)?;
    ops.push(Op::ForOf {
        label: None,
        iterable,
        slot,
        body,
    });
    Ok(())
}

fn for_in_slot(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<u16, Vec<String>> {
    let name = match left {
        oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) => {
            let Some(declarator) = declaration.declarations.first() else {
                return Err(vec!["Missing for-in binding".to_string()]);
            };
            let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) =
                &declarator.id.kind
            else {
                return Err(vec!["Unsupported for-in binding".to_string()]);
            };
            identifier.name.to_string()
        }
        oxc::ast::ast::ForStatementLeft::AssignmentTargetIdentifier(identifier) => {
            identifier.name.to_string()
        }
        _ => return Err(vec!["Unsupported for-in binding".to_string()]),
    };
    if let Some(slot) = locals.get(&name) {
        return Ok(*slot);
    }
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(name, slot);
    Ok(slot)
}

pub(crate) fn execute_for_in(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let (label, slot, body, keys) = unpack_for_in(registers, op)?;
    iterate_loop_keys(registers, label, slot, body, keys)
}

pub(crate) fn execute_for_of(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let (label, slot, body, values) = unpack_for_of(registers, op)?;
    iterate_loop_values(registers, label, slot, body, values)
}

type ForInLoopData<'a> = (&'a Option<String>, u16, &'a Vec<Op>, Vec<String>);
type ForOfLoopData<'a> = (
    &'a Option<String>,
    u16,
    &'a Vec<Op>,
    Vec<crate::value::Value>,
);

fn unpack_for_in<'a>(
    registers: &mut [crate::value::Value],
    op: &'a Op,
) -> Result<ForInLoopData<'a>, crate::execute::VmError> {
    let Op::ForIn {
        label,
        object,
        slot,
        body,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let keys = for_in_keys(crate::execute::read_register(registers, *object)?);
    Ok((label, *slot, body, keys))
}

fn for_in_keys(value: crate::value::Value) -> Vec<String> {
    match value {
        crate::value::Value::Object(properties) => {
            properties.iter().map(|(key, _)| key.clone()).collect()
        }
        crate::value::Value::Array(values) => (0..values.len()).map(|i| i.to_string()).collect(),
        _ => Vec::new(),
    }
}

fn iterate_loop_keys(
    registers: &mut Vec<crate::value::Value>,
    label: &Option<String>,
    slot: u16,
    body: &[Op],
    keys: Vec<String>,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    for key in keys {
        crate::execute::write_value(registers, slot, crate::value::Value::String(key));
        if let Some(result) = execute_loop_body(registers, label, body)? {
            return Ok(result);
        }
    }
    Ok(None)
}

fn unpack_for_of<'a>(
    registers: &mut [crate::value::Value],
    op: &'a Op,
) -> Result<ForOfLoopData<'a>, crate::execute::VmError> {
    let Op::ForOf {
        label,
        iterable,
        slot,
        body,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let values = for_of_values(crate::execute::read_register(registers, *iterable)?)?;
    Ok((label, *slot, body, values))
}

fn for_of_values(
    value: crate::value::Value,
) -> Result<Vec<crate::value::Value>, crate::execute::VmError> {
    match value {
        crate::value::Value::Array(values) => Ok(values.iter().cloned().collect()),
        crate::value::Value::String(value) => Ok(value
            .chars()
            .map(|character| crate::value::Value::String(character.to_string()))
            .collect()),
        _ => Err(crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::TypeError,
            &[crate::value::Value::String(
                "value is not iterable".to_string(),
            )],
        ))),
    }
}

fn iterate_loop_values(
    registers: &mut Vec<crate::value::Value>,
    label: &Option<String>,
    slot: u16,
    body: &[Op],
    values: Vec<crate::value::Value>,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    for value in values {
        crate::execute::write_value(registers, slot, value);
        if let Some(result) = execute_loop_body(registers, label, body)? {
            return Ok(result);
        }
    }
    Ok(None)
}

fn execute_loop_body(
    registers: &mut Vec<crate::value::Value>,
    label: &Option<String>,
    body: &[Op],
) -> Result<Option<Option<crate::value::Value>>, crate::execute::VmError> {
    match crate::execute::execute_in_place(body, registers) {
        Ok(value) => Ok(Some(Some(value))),
        Err(crate::execute::VmError::MissingReturn) => Ok(None),
        Err(crate::execute::VmError::Continue(continue_label))
            if continue_matches(label, &continue_label) =>
        {
            Ok(None)
        }
        Err(crate::execute::VmError::Continue(continue_label)) => {
            Err(crate::execute::VmError::Continue(continue_label))
        }
        Err(crate::execute::VmError::Break(break_label)) if break_matches(label, &break_label) => {
            Ok(Some(None))
        }
        Err(crate::execute::VmError::Break(break_label)) => {
            Err(crate::execute::VmError::Break(break_label))
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn execute(
    registers: &mut Vec<crate::value::Value>,
    op: &crate::ops::Op,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let crate::ops::Op::Loop {
        label,
        init,
        test,
        body,
        update,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    run_fragment(init, registers)?;
    while crate::execute::is_truthy(&crate::execute::execute_in_place(test, registers)?) {
        match crate::execute::execute_in_place(body, registers) {
            Ok(value) => return Ok(Some(value)),
            Err(crate::execute::VmError::MissingReturn) => {}
            Err(crate::execute::VmError::Continue(continue_label))
                if continue_matches(label, &continue_label) => {}
            Err(crate::execute::VmError::Continue(continue_label)) => {
                return Err(crate::execute::VmError::Continue(continue_label));
            }
            Err(crate::execute::VmError::Break(break_label))
                if break_matches(label, &break_label) =>
            {
                break
            }
            Err(crate::execute::VmError::Break(break_label)) => {
                return Err(crate::execute::VmError::Break(break_label));
            }
            Err(error) => return Err(error),
        }
        run_fragment(update, registers)?;
    }
    Ok(None)
}

fn break_matches(loop_label: &Option<String>, break_label: &Option<String>) -> bool {
    match break_label {
        None => true,
        Some(label) => loop_label.as_ref() == Some(label),
    }
}

fn continue_matches(loop_label: &Option<String>, continue_label: &Option<String>) -> bool {
    continue_label.is_none() || loop_label == continue_label
}

/// Run a loop fragment. An empty fragment (no init/update, e.g. a `while`
/// loop) is a no-op; a non-empty fragment must return normally.
fn run_fragment(
    ops: &[crate::ops::Op],
    registers: &mut Vec<crate::value::Value>,
) -> Result<(), crate::execute::VmError> {
    if ops.is_empty() {
        return Ok(());
    }
    crate::execute::execute_in_place(ops, registers)?;
    Ok(())
}

pub(crate) fn reduce_while(
    statement: &WhileStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let test = reduce_fragment(Some(&statement.test), ops, facts, next_register, locals)?;
    let body = {
        let mut fragment = Vec::new();
        reduce_body(
            &statement.body,
            &mut fragment,
            facts,
            next_register,
            next_slot,
            locals,
        )?;
        fragment
    };
    ops.push(Op::Loop {
        label: None,
        init: Vec::new(),
        test,
        body,
        update: Vec::new(),
    });
    Ok(())
}

pub(crate) fn reduce_do_while(
    statement: &DoWhileStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let mut body = Vec::new();
    reduce_body(
        &statement.body,
        &mut body,
        facts,
        next_register,
        next_slot,
        locals,
    )?;
    let condition =
        crate::reduce::reduce_expression(&statement.test, &mut body, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported do-while condition".to_string()])?;
    body.push(Op::Branch {
        condition,
        then_ops: Vec::new(),
        else_ops: vec![Op::Break { label: None }],
    });
    ops.push(Op::Loop {
        label: None,
        init: Vec::new(),
        test: always_true(next_register),
        body,
        update: Vec::new(),
    });
    Ok(())
}

fn always_true(next_register: &mut u16) -> Vec<Op> {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    vec![
        Op::Const {
            dst: register,
            value: Constant::Boolean(true),
        },
        Op::Return { src: register },
    ]
}
