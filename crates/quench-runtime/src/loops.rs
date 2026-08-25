use std::collections::HashMap;

mod counted_for;
include!("loops_for_of.rs");

use oxc::ast::ast::{DoWhileStatement, ForInStatement, ForOfStatement, WhileStatement};

use crate::{
    facts::ProgramDb,
    ops::{Constant, Op},
};

use counted_for::reduce_fragment;

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
) -> Result<Option<u16>, Vec<String>> {
    let outer_locals = locals.clone();
    let (slot, per_iteration, pattern) = for_of_slot(&statement.left, next_slot, locals)?;
    emit_for_in_initializer(
        &statement.left,
        slot,
        ops,
        facts,
        next_register,
        locals,
        facts.strict,
    )?;
    if per_iteration {
        emit_for_head_tdz(&statement.left, ops, locals);
    }
    let object =
        crate::reduce::reduce_expression(&statement.right, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported for-in object".to_string()])?;
    let dst = crate::switch::take_completion_register(ops, next_register);
    let mut body_locals = locals.clone();
    let slot = if per_iteration {
        refresh_iteration_locals(&statement.left, next_slot, &mut body_locals)
    } else {
        slot
    };
    let (mut body, _) = crate::switch::with_completion(dst, || {
        let mut body = Vec::new();
        let last = crate::loops::reduce_loop_body(
            &statement.body,
            &mut body,
            facts,
            next_register,
            next_slot,
            &mut body_locals,
            dst,
        )?;
        Ok::<_, Vec<String>>((body, last))
    })?;
    if let Some(pattern) = pattern {
        prepend_for_of_binding(pattern, slot, &mut body, facts, next_register, &body_locals)?;
    }
    if for_left_immutable(&statement.left) {
        body.insert(0, Op::MarkImmutable { slot });
    }
    *locals = outer_locals;
    ops.push(Op::ForIn {
        label: None,
        object,
        slot,
        body: crate::machine::FunctionCode::pending(body),
        per_iteration,
        dst,
    });
    Ok(Some(dst))
}

fn for_left_immutable(left: &oxc::ast::ast::ForStatementLeft<'_>) -> bool {
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        return false;
    };
    matches!(
        declaration.kind,
        oxc::ast::ast::VariableDeclarationKind::Const
            | oxc::ast::ast::VariableDeclarationKind::Using
            | oxc::ast::ast::VariableDeclarationKind::AwaitUsing
    )
}

fn emit_for_in_initializer(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    slot: u16,
    ops: &mut Vec<Op>,
    facts: &mut crate::facts::ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
    strict: bool,
) -> Result<(), Vec<String>> {
    if strict {
        return Ok(());
    }
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        return Ok(());
    };
    if declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var {
        return Ok(());
    }
    let Some(init) = declaration
        .declarations
        .first()
        .and_then(|d| d.init.as_ref())
    else {
        return Ok(());
    };
    let src = crate::reduce::reduce_expression(init, ops, facts, next_register, locals)
        .ok_or_else(|| vec!["Unsupported for-in initializer".to_string()])?;
    ops.push(Op::StoreLocal { slot, src });
    Ok(())
}

pub(crate) fn reduce_for_of(
    statement: &ForOfStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let outer_locals = locals.clone();
    let (slot, per_iteration, pattern) = for_of_slot(&statement.left, next_slot, locals)?;
    if per_iteration {
        emit_for_head_tdz(&statement.left, ops, locals);
    }
    let iterable =
        crate::reduce::reduce_expression(&statement.right, ops, facts, next_register, locals)
            .ok_or_else(|| vec!["Unsupported for-of iterable".to_string()])?;
    let dst = crate::switch::take_completion_register(ops, next_register);
    let mut body_locals = locals.clone();
    let has_pattern = pattern.is_some();
    let binding_slot = if per_iteration {
        refresh_iteration_locals(&statement.left, next_slot, &mut body_locals)
    } else {
        slot
    };
    let iteration_slot = if has_pattern { slot } else { binding_slot };
    let (mut body, _) = crate::switch::with_completion(dst, || {
        let mut body = Vec::new();
        let last = crate::loops::reduce_loop_body(
            &statement.body,
            &mut body,
            facts,
            next_register,
            next_slot,
            &mut body_locals,
            dst,
        )?;
        Ok::<_, Vec<String>>((body, last))
    })?;
    if let Some(pattern) = pattern {
        prepend_for_of_binding(
            pattern,
            iteration_slot,
            &mut body,
            facts,
            next_register,
            &body_locals,
        )?;
    }
    if for_left_immutable(&statement.left) && !has_pattern {
        body.insert(0, Op::MarkImmutable { slot: binding_slot });
    }
    *locals = outer_locals;
    ops.push(Op::ForOf {
        label: None,
        iterable,
        slot: iteration_slot,
        body: crate::machine::FunctionCode::pending(body),
        per_iteration,
        r#await: statement.r#await,
        dst,
    });
    Ok(Some(dst))
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
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let data = unpack_for_in(registers, op)?;
    iterate_loop_keys(registers, data)
}

pub(crate) fn execute_for_of(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, slot, body, per_iteration, await_values, iterable, dst) =
        unpack_for_of(registers, op)?;
    let iterator = if await_values {
        crate::collections::iterator::open_async(iterable)?
    } else {
        crate::collections::iterator::open(iterable)?
    };
    iterate_loop_values(
        registers,
        label,
        slot,
        body,
        per_iteration,
        await_values,
        iterator,
        dst,
    )
}

/// Active `for-of` iterators are execution bookkeeping, never observable
/// through the JavaScript object model. Keep this stack in an `UnsafeCell`
/// rather than imposing a borrow-checking runtime guard on every VM step.
///
/// The VM is single-threaded and this value is thread-local, so callers must
/// only access it through the methods below.
struct LiveForOf(std::cell::UnsafeCell<Vec<crate::value::Value>>);

impl LiveForOf {
    const fn new() -> Self {
        Self(std::cell::UnsafeCell::new(Vec::new()))
    }

    fn push(&self, iterator: crate::value::Value) {
        unsafe { &mut *self.0.get() }.push(iterator);
    }

    fn last(&self) -> Option<crate::value::Value> {
        unsafe { (&*self.0.get()).last().cloned() }
    }

    fn pop(&self) -> Option<crate::value::Value> {
        unsafe { (&mut *self.0.get()).pop() }
    }
}

thread_local! {
    static LIVE_FOR_OF: LiveForOf = const { LiveForOf::new() };
}

fn remember_for_of(iterator: crate::value::Value) {
    LIVE_FOR_OF.with(|live| live.push(iterator));
}

pub(crate) fn live_for_of() -> Option<crate::value::Value> {
    LIVE_FOR_OF.with(LiveForOf::last)
}

pub(crate) fn take_live_for_of() -> Option<crate::value::Value> {
    LIVE_FOR_OF.with(LiveForOf::pop)
}

type ForInLoopData<'a> = (
    &'a Option<String>,
    u16,
    &'a crate::machine::FunctionCode,
    bool,
    Vec<String>,
    u16,
    crate::value::Value,
);
type ForOfLoopData<'a> = (
    &'a Option<String>,
    u16,
    &'a crate::machine::FunctionCode,
    bool,
    bool,
    crate::value::Value,
    u16,
);

fn unpack_for_in<'a>(
    registers: &mut crate::register_file::RegisterFile,
    op: &'a Op,
) -> Result<ForInLoopData<'a>, crate::execute::VmError> {
    let Op::ForIn {
        label,
        object,
        slot,
        body,
        per_iteration,
        dst,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value = crate::execute::read_register(registers, *object)?;
    let keys = for_in_keys(&value);
    Ok((label, *slot, body, *per_iteration, keys, *dst, value))
}

fn for_in_keys(value: &crate::value::Value) -> Vec<String> {
    if matches!(
        value,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) {
        return Vec::new();
    }
    if matches!(value, crate::value::Value::Proxy(_)) {
        if let Ok(crate::value::Value::Array(keys)) = crate::proxy::proxy_own_keys(value) {
            return keys
                .snapshot()
                .into_iter()
                .filter_map(|key| match key {
                    crate::value::Value::String(key) => Some(key),
                    _ => None,
                })
                .filter(|key| crate::own_keys::is_enumerable_property(value, key))
                .collect();
        }
        return Vec::new();
    }
    crate::own_keys::enumerate_object_properties(value)
}

fn iterate_loop_keys(
    registers: &mut crate::register_file::RegisterFile,
    data: ForInLoopData<'_>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, slot, body, per_iteration, keys, dst, object) = data;
    let Some(body) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    for key in keys {
        let resolved = crate::locals::resolved_replacement(object.clone());
        if !crate::own_keys::is_enumerable_property(&resolved, &key) {
            continue;
        }
        let value = crate::value::Value::String(key);
        let _binding = bind_iteration(slot, value, per_iteration);
        match execute_loop_body(registers, label, body)? {
            crate::completion::LoopTransition::Continue(_) => {}
            crate::completion::LoopTransition::Break(value) => {
                // Per spec §13.7.5.13, a `break` produces a normal
                // completion whose value is V (undefined) unless a value
                // was supplied. Write the value into dst so callers can
                // read it as the loop's completion.
                let value = value.unwrap_or(crate::value::Value::Undefined);
                crate::execute::write_value(registers, dst, value.clone());
                return Ok(crate::completion::Completion::Normal);
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                return attach_loop_completion(registers, dst, completion);
            }
        }
    }
    Ok(crate::completion::Completion::Normal)
}

fn attach_loop_completion(
    registers: &crate::register_file::RegisterFile,
    dst: u16,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let value = crate::execute::read_register(registers, dst)?;
    Ok(completion.update_empty(value))
}

fn unpack_for_of<'a>(
    registers: &mut crate::register_file::RegisterFile,
    op: &'a Op,
) -> Result<ForOfLoopData<'a>, crate::execute::VmError> {
    let Op::ForOf {
        label,
        iterable,
        slot,
        body,
        per_iteration,
        r#await,
        dst,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let iterable = crate::execute::read_register(registers, *iterable)?;
    Ok((label, *slot, body, *per_iteration, *r#await, iterable, *dst))
}

fn iterate_loop_values(
    registers: &mut crate::register_file::RegisterFile,
    label: &Option<String>,
    slot: u16,
    body: &crate::machine::FunctionCode,
    per_iteration: bool,
    await_values: bool,
    iterator: crate::value::Value,
    dst: u16,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let Some(body) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    loop {
        let value = match if await_values {
            crate::collections::iterator::step_value_await(&iterator)
        } else {
            crate::collections::iterator::step_value(&iterator)
        } {
            Ok(value) => value,
            Err(crate::execute::VmError::Thrown(reason)) => {
                // IteratorClose applies to abrupt completion from the loop
                // body, not to an abrupt IteratorStep/IteratorValue.
                return Ok(crate::completion::Completion::Throw(reason));
            }
            Err(error) => return Err(error),
        };
        let Some(value) = value else {
            return Ok(crate::completion::Completion::Normal);
        };
        let _binding = bind_iteration(slot, value, per_iteration);
        match execute_loop_body(registers, label, body)? {
            crate::completion::LoopTransition::Continue(_) => {}
            crate::completion::LoopTransition::Break(_) => {
                return crate::collections::iterator::close(
                    iterator,
                    crate::completion::Completion::Normal,
                );
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                if completion.is_suspension() {
                    remember_for_of(iterator);
                    return Ok(completion);
                }
                let completion = attach_loop_completion(registers, dst, completion)?;
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
    registers: &mut crate::register_file::RegisterFile,
    label: &Option<String>,
    body: crate::machine::CodeView<'_>,
) -> Result<crate::completion::LoopTransition, crate::execute::VmError> {
    Ok(crate::completion::Completion::into_loop_transition(
        crate::vm::execute_code_completion_in_current_frame(body, registers)?,
        label,
    ))
}

include!("loops_run.rs");
include!("loops_numeric_kernel.rs");
include!("loops_crypto_kernel.rs");
include!("loops_advect_kernel.rs");
include!("loops_packed_kernels.rs");
include!("loops_body.rs");
include!("loops_while.rs");

#[cfg(test)]
mod tests {
    use super::{live_for_of, take_live_for_of, LIVE_FOR_OF};
    use crate::value::Value;

    #[test]
    fn live_for_of_stack_is_lifo_and_empty_after_pop() {
        LIVE_FOR_OF.with(|live| {
            live.push(Value::Number(1.0));
            live.push(Value::Number(2.0));
        });
        assert_eq!(live_for_of(), Some(Value::Number(2.0)));
        assert_eq!(take_live_for_of(), Some(Value::Number(2.0)));
        assert_eq!(take_live_for_of(), Some(Value::Number(1.0)));
        assert_eq!(take_live_for_of(), None);
    }
}
