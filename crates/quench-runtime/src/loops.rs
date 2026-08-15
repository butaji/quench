use std::collections::HashMap;

mod counted_for;
include!("loops_for_of.rs");

use oxc::ast::ast::{DoWhileStatement, ForInStatement, ForOfStatement, WhileStatement};

use crate::{
    facts::ProgramDb,
    ops::{Constant, Op},
};

use counted_for::{reduce_body, reduce_fragment};

pub(crate) use counted_for::reduce_for;

pub(crate) fn reduce_update(
    update: &oxc::ast::ast::UpdateExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let mut place = crate::reduce::reduce_assignments::reduce_simple_place(
        &update.argument,
        ops,
        facts,
        next_register,
        locals,
    )?;
    crate::reduce::reduce_assignments::capture_name_target(&mut place, ops, next_register);
    crate::reduce::reduce_assignments::prepare_get(&mut place, ops, next_register);
    let old = crate::reduce::reduce_assignments::get(&place, ops, next_register)?;
    let one = emit_one(ops, next_register);
    let updated = emit_member_update_value(ops, next_register, update, old, one);
    crate::reduce::reduce_assignments::put(place, updated, ops)?;
    if update.prefix {
        Some(updated)
    } else {
        let numeric_old = emit_numeric(ops, next_register, old);
        Some(numeric_old)
    }
}

fn emit_numeric(ops: &mut Vec<Op>, next_register: &mut u16, src: u16) -> u16 {
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Unary {
        dst,
        operator: crate::ops::UnaryOp::ToNumeric,
        src,
    });
    dst
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
        oxc::syntax::operator::UpdateOperator::Increment => crate::ops::BinaryOp::NumericAdd,
        oxc::syntax::operator::UpdateOperator::Decrement => crate::ops::BinaryOp::NumericSubtract,
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
    let (slot, per_iteration) = for_in_slot(&statement.left, next_slot, locals)?;
    let object =
        crate::reduce::reduce_expression(&statement.right, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported for-in object".to_string()])?;
    let body = crate::branch::reduce(&statement.body, facts, locals)?;
    ops.push(Op::ForIn {
        label: None,
        object,
        slot,
        body: crate::machine::FunctionCode::from_ops(body),
        per_iteration,
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
    let (slot, per_iteration, pattern) = for_of_slot(&statement.left, next_slot, locals)?;
    let iterable =
        crate::reduce::reduce_expression(&statement.right, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported for-of iterable".to_string()])?;
    let mut body = crate::branch::reduce(&statement.body, facts, locals)?;
    if let Some(pattern) = pattern {
        prepend_for_of_binding(pattern, slot, &mut body, facts, next_register, locals)?;
    }
    ops.push(Op::ForOf {
        label: None,
        iterable,
        slot,
        body: crate::machine::FunctionCode::from_ops(body),
        per_iteration,
    });
    Ok(())
}

fn for_in_slot(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(u16, bool), Vec<String>> {
    let (name, per_iteration) = match left {
        oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) => {
            let Some(declarator) = declaration.declarations.first() else {
                return Err(vec!["Missing for-in binding".to_string()]);
            };
            let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) =
                &declarator.id.kind
            else {
                return Err(vec!["Unsupported for-in binding".to_string()]);
            };
            (
                identifier.name.to_string(),
                declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var,
            )
        }
        oxc::ast::ast::ForStatementLeft::AssignmentTargetIdentifier(identifier) => {
            (identifier.name.to_string(), false)
        }
        _ => return Err(vec!["Unsupported for-in binding".to_string()]),
    };
    if let Some(slot) = locals.get(&name) {
        return Ok((*slot, per_iteration));
    }
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(name, slot);
    Ok((slot, per_iteration))
}

pub(crate) fn execute_for_in(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, slot, body, per_iteration, keys) = unpack_for_in(registers, op)?;
    iterate_loop_keys(registers, label, slot, body, per_iteration, keys)
}

pub(crate) fn execute_for_of(
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, slot, body, per_iteration, iterable) = unpack_for_of(registers, op)?;
    let iterator = crate::collections::iterator::open(iterable)?;
    iterate_loop_values(registers, label, slot, body, per_iteration, iterator)
}

type ForInLoopData<'a> = (
    &'a Option<String>,
    u16,
    &'a crate::machine::FunctionCode,
    bool,
    Vec<String>,
);
type ForOfLoopData<'a> = (
    &'a Option<String>,
    u16,
    &'a crate::machine::FunctionCode,
    bool,
    crate::value::Value,
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
        per_iteration,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let keys = for_in_keys(crate::execute::read_register(registers, *object)?)?;
    Ok((label, *slot, body, *per_iteration, keys))
}

fn for_in_keys(value: crate::value::Value) -> Result<Vec<String>, crate::execute::VmError> {
    match value {
        value @ (crate::value::Value::Object(_) | crate::value::Value::ObjectAlias(_)) => {
            crate::own_keys::enumerable_key_strings_result(Some(&value))
        }
        crate::value::Value::Array(values) => crate::own_keys::enumerable_key_strings_result(Some(
            &crate::value::Value::Array(values),
        )),
        _ => Ok(Vec::new()),
    }
}

fn iterate_loop_keys(
    registers: &mut Vec<crate::value::Value>,
    label: &Option<String>,
    slot: u16,
    body: &crate::machine::FunctionCode,
    per_iteration: bool,
    keys: Vec<String>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let Some(body) = body.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    for key in keys {
        let value = crate::value::Value::String(key);
        let _binding = bind_iteration(slot, value, per_iteration);
        match execute_loop_body(registers, label, body)? {
            crate::completion::LoopTransition::Continue => {}
            crate::completion::LoopTransition::Break => {
                return Ok(crate::completion::Completion::Normal)
            }
            crate::completion::LoopTransition::Propagate(completion) => return Ok(completion),
        }
    }
    Ok(crate::completion::Completion::Normal)
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
        per_iteration,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let iterable = crate::execute::read_register(registers, *iterable)?;
    Ok((label, *slot, body, *per_iteration, iterable))
}

fn iterate_loop_values(
    registers: &mut Vec<crate::value::Value>,
    label: &Option<String>,
    slot: u16,
    body: &crate::machine::FunctionCode,
    per_iteration: bool,
    iterator: crate::value::Value,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let Some(body) = body.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    loop {
        let value = match crate::collections::iterator::step_value(&iterator) {
            Ok(value) => value,
            Err(crate::execute::VmError::Thrown(reason)) => {
                return crate::collections::iterator::close(
                    iterator,
                    crate::completion::Completion::Throw(reason),
                );
            }
            Err(error) => return Err(error),
        };
        let Some(value) = value else {
            return Ok(crate::completion::Completion::Normal);
        };
        let _binding = bind_iteration(slot, value, per_iteration);
        match execute_loop_body(registers, label, body)? {
            crate::completion::LoopTransition::Continue => {}
            crate::completion::LoopTransition::Break => {
                return crate::collections::iterator::close(
                    iterator,
                    crate::completion::Completion::Normal,
                );
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                return crate::collections::iterator::close(iterator, completion);
            }
        }
    }
}

fn bind_iteration(
    slot: u16,
    value: crate::value::Value,
    per_iteration: bool,
) -> Option<crate::locals::IterationBinding> {
    if per_iteration {
        Some(crate::locals::IterationBinding::install(slot, value))
    } else {
        crate::locals::write(slot, value);
        None
    }
}

fn execute_loop_body(
    registers: &mut Vec<crate::value::Value>,
    label: &Option<String>,
    body: &[Op],
) -> Result<crate::completion::LoopTransition, crate::execute::VmError> {
    Ok(crate::completion::Completion::into_loop_transition(
        crate::execute::execute_completion_in_place(body, registers)?,
        label,
    ))
}

pub(crate) fn execute(
    registers: &mut Vec<crate::value::Value>,
    op: &crate::ops::Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, init, test, body, update, post_test) = match op {
        crate::ops::Op::Loop {
            label,
            init,
            test,
            body,
            update,
            post_test,
        } => (label, init, test, body, update, post_test),
        _ => return Err(crate::execute::VmError::MissingReturn),
    };
    let Some(body) = body.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(init) = init.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(test) = test.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let Some(update) = update.ops() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    run_loop(label, init, test, body, update, *post_test, registers)
}

fn run_loop(
    label: &Option<String>,
    init: &[Op],
    test: &[Op],
    body: &[Op],
    update: &[Op],
    post_test: bool,
    registers: &mut Vec<crate::value::Value>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    run_fragment(init, registers)?;
    loop {
        if !post_test && !loop_test(test, registers)? {
            break;
        }
        match execute_loop_body(registers, label, body)? {
            crate::completion::LoopTransition::Continue => {}
            crate::completion::LoopTransition::Break => break,
            crate::completion::LoopTransition::Propagate(completion) => return Ok(completion),
        }
        run_fragment(update, registers)?;
        if post_test && !loop_test(test, registers)? {
            break;
        }
    }
    Ok(crate::completion::Completion::Normal)
}

fn loop_test(
    test: &[Op],
    registers: &mut Vec<crate::value::Value>,
) -> Result<bool, crate::execute::VmError> {
    crate::execute::execute_in_place(test, registers).map(|value| crate::execute::is_truthy(&value))
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
    let [init, test, body, update] =
        crate::machine::FunctionCode::from_ops_many(vec![Vec::new(), test, body, Vec::new()])
            .try_into()
            .expect("four loop bodies");
    ops.push(Op::Loop {
        label: None,
        init,
        test,
        body,
        update,
        post_test: false,
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
    let test = reduce_fragment(Some(&statement.test), ops, facts, next_register, locals)?;
    let [init, test, body, update] =
        crate::machine::FunctionCode::from_ops_many(vec![Vec::new(), test, body, Vec::new()])
            .try_into()
            .expect("four loop bodies");
    ops.push(Op::Loop {
        label: None,
        init,
        test,
        body,
        update,
        post_test: true,
    });
    Ok(())
}
