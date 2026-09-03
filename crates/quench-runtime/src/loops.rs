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
    prepend_iteration_immutability(&statement.left, slot, &body_locals, &mut body);
    if let Some(pattern) = pattern {
        prepend_for_of_binding(pattern, slot, &mut body, facts, next_register, &body_locals)?;
    }
    *locals = outer_locals;
    ops.push(Op::ForIn {
        label: None,
        object,
        slot,
        body: crate::machine::FunctionCode::pending(body),
        per_iteration,
        iteration_slots: iteration_binding_slots(&statement.left, &body_locals),
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

fn prepend_iteration_immutability(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    slot: u16,
    locals: &HashMap<String, u16>,
    body: &mut Vec<Op>,
) {
    if !for_left_immutable(left) {
        return;
    }
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        body.insert(0, Op::MarkImmutable { slot });
        return;
    };
    let Some(declarator) = declaration.declarations.first() else {
        body.insert(0, Op::MarkImmutable { slot });
        return;
    };
    if matches!(
        declarator.id.kind,
        oxc::ast::ast::BindingPatternKind::BindingIdentifier(_)
    ) {
        body.insert(0, Op::MarkImmutable { slot });
        return;
    }
    for name in crate::binding_patterns::names(&declarator.id)
        .into_iter()
        .rev()
    {
        if let Some(&name_slot) = locals.get(&name) {
            body.insert(0, Op::MarkImmutable { slot: name_slot });
        }
    }
}

fn iteration_binding_slots(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    locals: &HashMap<String, u16>,
) -> Vec<u16> {
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        return Vec::new();
    };
    let Some(declarator) = declaration.declarations.first() else {
        return Vec::new();
    };
    crate::binding_patterns::names(&declarator.id)
        .into_iter()
        .filter_map(|name| locals.get(&name).copied())
        .collect()
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
    prepend_iteration_immutability(&statement.left, slot, &body_locals, &mut body);
    if let Some(pattern) = pattern {
        prepend_for_of_binding(pattern, slot, &mut body, facts, next_register, &body_locals)?;
    }
    *locals = outer_locals;
    ops.push(Op::ForOf {
        label: None,
        iterable,
        slot,
        body: crate::machine::FunctionCode::pending(body),
        per_iteration,
        iteration_slots: iteration_binding_slots(&statement.left, &body_locals),
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
    let (label, slot, body, per_iteration, iteration_slots, await_values, iterable, dst) =
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
        iteration_slots,
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
    static PENDING_ASYNC_FOR_OF: std::cell::UnsafeCell<Vec<crate::value::AsyncForOfState>> =
        const { std::cell::UnsafeCell::new(Vec::new()) };
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

fn remember_pending_async_for_of(state: crate::value::AsyncForOfState) {
    PENDING_ASYNC_FOR_OF.with(|pending| unsafe { (&mut *pending.get()).push(state) });
}

pub(crate) fn take_pending_async_for_of() -> Option<crate::value::AsyncForOfState> {
    PENDING_ASYNC_FOR_OF.with(|pending| unsafe { (&mut *pending.get()).pop() })
}

type ForInLoopData<'a> = (
    &'a Option<String>,
    u16,
    &'a crate::machine::FunctionCode,
    bool,
    &'a [u16],
    Vec<String>,
    u16,
    crate::value::Value,
);
type ForOfLoopData<'a> = (
    &'a Option<String>,
    u16,
    &'a crate::machine::FunctionCode,
    bool,
    &'a [u16],
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
        iteration_slots,
        dst,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value = crate::execute::read_register(registers, *object)?;
    let keys = for_in_keys(&value);
    Ok((
        label,
        *slot,
        body,
        *per_iteration,
        iteration_slots,
        keys,
        *dst,
        value,
    ))
}

fn for_in_keys(value: &crate::value::Value) -> Vec<String> {
    if matches!(
        value,
        crate::value::Value::Null | crate::value::Value::Undefined
    ) {
        return Vec::new();
    }
    crate::own_keys::enumerate_object_properties(value)
}

fn iterate_loop_keys(
    registers: &mut crate::register_file::RegisterFile,
    data: ForInLoopData<'_>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let (label, slot, body, per_iteration, iteration_slots, keys, dst, object) = data;
    let Some(body_code) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    for key in keys {
        let resolved = crate::locals::resolved_replacement(object.clone());
        if !crate::own_keys::is_enumerable_property(&resolved, &key) {
            continue;
        }
        let value = crate::value::Value::String(key);
        let _binding = bind_iteration(registers, slot, value, per_iteration, iteration_slots);
        let _ = body.enter_invocation();
        match execute_loop_body_with_owner(registers, label, body_code, body)? {
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
        iteration_slots,
        r#await,
        dst,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let iterable = crate::execute::read_register(registers, *iterable)?;
    Ok((
        label,
        *slot,
        body,
        *per_iteration,
        iteration_slots,
        *r#await,
        iterable,
        *dst,
    ))
}

fn iterate_loop_values(
    registers: &mut crate::register_file::RegisterFile,
    label: &Option<String>,
    slot: u16,
    body: &crate::machine::FunctionCode,
    per_iteration: bool,
    await_values: bool,
    iterator: crate::value::Value,
    iteration_slots: &[u16],
    dst: u16,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    registers.resize_undefined(
        registers.len().max(
            usize::from(dst)
                .saturating_add(body.len())
                .saturating_add(1),
        ),
    );
    let body_owner = body.clone();
    let Some(body_code) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let pending = crate::value::AsyncForOfState {
        label: label.clone(),
        slot,
        body: body_owner,
        per_iteration,
        iteration_slots: iteration_slots.to_vec(),
        iterator: iterator.clone(),
        dst,
        await_dst: 0,
    };
    loop {
        let value = match if await_values {
            crate::collections::iterator::step_value_await(&iterator)
        } else {
            crate::collections::iterator::step_value(&iterator)
        } {
            Ok(value) => value,
            Err(crate::execute::VmError::Thrown(reason)) => {
                return Err(crate::execute::VmError::Thrown(reason));
            }
            Err(crate::execute::VmError::Suspended(promise)) if await_values => {
                remember_pending_async_for_of(pending);
                return Err(crate::execute::VmError::Suspended(promise));
            }
            Err(error) => return Err(error),
        };
        let Some(value) = value else {
            return Ok(crate::completion::Completion::Normal);
        };
        let _binding = bind_iteration(registers, slot, value, per_iteration, iteration_slots);
        let _ = body.enter_invocation();
        match execute_loop_body_with_owner(registers, label, body_code, body)? {
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

pub(crate) fn resume_async_for_of(
    registers: &mut crate::register_file::RegisterFile,
    spec: &crate::value::AsyncForOfState,
    input: crate::value::Value,
) -> Result<
    (
        crate::completion::Completion,
        Option<crate::value::AsyncForOfState>,
    ),
    crate::execute::VmError,
> {
    let Some(body_code) = spec.body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let mut next_value = crate::collections::iterator::resume_async_result(&spec.iterator, input)?;
    loop {
        let Some(value) = next_value else {
            return Ok((crate::completion::Completion::Normal, None));
        };
        let _binding = bind_iteration(
            registers,
            spec.slot,
            value,
            spec.per_iteration,
            &spec.iteration_slots,
        );
        let _ = spec.body.enter_invocation();
        match execute_loop_body_with_owner(registers, &spec.label, body_code, &spec.body)? {
            crate::completion::LoopTransition::Continue(_) => {}
            crate::completion::LoopTransition::Break(_) => {
                return crate::collections::iterator::close(
                    spec.iterator.clone(),
                    crate::completion::Completion::Normal,
                )
                .map(|completion| (completion, None));
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                if completion.is_suspension() {
                    remember_for_of(spec.iterator.clone());
                    return Ok((completion, None));
                }
                let completion = attach_loop_completion(registers, spec.dst, completion)?;
                return crate::collections::iterator::close(spec.iterator.clone(), completion)
                    .map(|completion| (completion, None));
            }
        }
        next_value = match crate::collections::iterator::step_value_await(&spec.iterator) {
            Ok(value) => value,
            Err(crate::execute::VmError::Suspended(promise)) => {
                remember_pending_async_for_of(spec.clone());
                return Ok((
                    crate::completion::Completion::Suspend(promise),
                    Some(spec.clone()),
                ));
            }
            Err(error) => return Err(error),
        };
    }
}

fn bind_iteration(
    registers: &mut crate::register_file::RegisterFile,
    slot: u16,
    value: crate::value::Value,
    per_iteration: bool,
    iteration_slots: &[u16],
) -> Option<crate::locals::IterationBinding> {
    registers.write(usize::from(slot), value.clone());
    if per_iteration {
        for candidate in iteration_slots {
            if *candidate != slot {
                registers.write(usize::from(*candidate), crate::value::Value::Undefined);
            }
        }
        Some(crate::locals::IterationBinding::install_many(
            std::iter::once((slot, value)).chain(
                iteration_slots
                    .iter()
                    .copied()
                    .filter(|candidate| *candidate != slot)
                    .map(|candidate| (candidate, crate::value::Value::Undefined)),
            ),
        ))
    } else {
        crate::locals::write(slot, value);
        None
    }
}

fn execute_loop_body_with_owner(
    registers: &mut crate::register_file::RegisterFile,
    label: &Option<String>,
    body: crate::machine::CodeView<'_>,
    owner: &crate::machine::FunctionCode,
) -> Result<crate::completion::LoopTransition, crate::execute::VmError> {
    Ok(crate::completion::Completion::into_loop_transition(
        crate::vm::execute_code_completion_with_owner(body, owner, registers)?,
        label,
    ))
}

include!("loops_run.rs");
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

    #[test]
    fn counted_loop_body_uses_its_owner_for_tier_admission() {
        let init = crate::machine::FunctionCode::from_ops(vec![
            crate::ops::Op::Const {
                dst: 0,
                value: crate::ops::Constant::Number(0.0),
            },
            crate::ops::Op::Const {
                dst: 1,
                value: crate::ops::Constant::Number(3.0),
            },
            crate::ops::Op::Const {
                dst: 2,
                value: crate::ops::Constant::Number(1.0),
            },
        ]);
        let test = crate::machine::FunctionCode::from_ops(vec![
            crate::ops::Op::Binary {
                dst: 3,
                operator: crate::ops::BinaryOp::LessThan,
                lhs: 0,
                rhs: 1,
            },
            crate::ops::Op::Return { src: 3 },
        ]);
        let body = crate::machine::FunctionCode::from_ops(vec![crate::ops::Op::Binary {
            dst: 0,
            operator: crate::ops::BinaryOp::NumericAdd,
            lhs: 0,
            rhs: 2,
        }]);
        body.set_tier_threshold_for_test(1);
        let update = crate::machine::FunctionCode::from_ops(Vec::new());
        let loop_op = crate::ops::Op::Loop {
            label: None,
            init,
            test,
            body: body.clone(),
            update,
            post_test: false,
            dst: 4,
            per_iteration: Vec::new(),
        };
        let mut registers = crate::register_file::RegisterFile::new();
        assert_eq!(
            super::execute(&mut registers, &loop_op),
            Ok(crate::completion::Completion::Normal)
        );
        assert_eq!(registers.read_number(0), Some(3.0));
        assert!(matches!(
            body.tier(),
            crate::machine::ExecutionTier::Baseline | crate::machine::ExecutionTier::Optimizing
        ));
        assert_eq!(body.tier_profile().baseline_instructions, 1);
        if body.tier() == crate::machine::ExecutionTier::Optimizing {
            assert_eq!(body.tier_profile().optimizing_instructions, 1);
        }
        assert!(body.tier_profile().invocations >= 2);
        assert!(body.tier_profile().retired >= 1);
    }
}
