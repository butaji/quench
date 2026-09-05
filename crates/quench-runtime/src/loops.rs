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
    let mut body_slots = Vec::new();
    let (mut body, _) = crate::switch::with_completion(dst, || {
        let mut body = Vec::new();
        let last = reduce_loop_body_with_slots(
            &statement.body,
            &mut body,
            facts,
            next_register,
            next_slot,
            &mut body_locals,
            dst,
            &mut body_slots,
        )?;
        Ok::<_, Vec<String>>((body, last))
    })?;
    prepend_iteration_immutability(&statement.left, slot, &body_locals, &mut body);
    if let Some(pattern) = pattern {
        prepend_for_of_binding(pattern, slot, &mut body, facts, next_register, &body_locals)?;
    }
    // A lexical declaration in the loop body gets a fresh binding for every
    // iteration even when the `for-in` head assigns to an existing `let`
    // identifier (`for (key in object)`). Keep those body cells distinct so
    // callbacks created during one iteration do not observe a later value.
    let per_iteration = per_iteration || !body_slots.is_empty();
    *locals = outer_locals;
    ops.push(Op::ForIn {
        label: None,
        object,
        slot,
        body: crate::machine::FunctionCode::pending(body),
        per_iteration,
        iteration_slots: iteration_slots(&statement.left, &body_locals, &body_slots),
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

fn iteration_slots(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    locals: &HashMap<String, u16>,
    body_slots: &[u16],
) -> Vec<u16> {
    let mut slots = match left {
        oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) => declaration
            .declarations
            .first()
            .map(|declarator| {
                crate::binding_patterns::names(&declarator.id)
                    .into_iter()
                    .filter_map(|name| locals.get(&name).copied())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    for slot in body_slots {
        if !slots.contains(slot) {
            slots.push(*slot);
        }
    }
    slots
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
    let mut body_slots = Vec::new();
    let (mut body, _) = crate::switch::with_completion(dst, || {
        let mut body = Vec::new();
        let last = reduce_loop_body_with_slots(
            &statement.body,
            &mut body,
            facts,
            next_register,
            next_slot,
            &mut body_locals,
            dst,
            &mut body_slots,
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
        iteration_slots: iteration_slots(&statement.left, &body_locals, &body_slots),
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

pub(crate) fn take_pending_async_for_of(owner: usize) -> Option<crate::value::AsyncForOfState> {
    PENDING_ASYNC_FOR_OF.with(|pending| unsafe {
        let pending = &mut *pending.get();
        let index = pending.iter().rposition(|state| state.owner == owner)?;
        Some(pending.remove(index))
    })
}

pub(crate) fn reset_fixture_state() {
    LIVE_FOR_OF.with(|live| unsafe { drop(std::mem::take(&mut *live.0.get())) });
    PENDING_ASYNC_FOR_OF.with(|pending| unsafe { drop(std::mem::take(&mut *pending.get())) });
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
    let Some(body) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    for key in keys {
        let resolved = crate::locals::resolved_replacement(object.clone());
        if !crate::own_keys::is_enumerable_property(&resolved, &key) {
            continue;
        }
        let value = crate::value::Value::String(key);
        let _binding = bind_iteration(registers, slot, value, per_iteration, iteration_slots);
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
    if matches!(
        completion,
        crate::completion::Completion::Break { value: None, .. }
            | crate::completion::Completion::Continue { value: None, .. }
    ) {
        let value = crate::execute::read_register(registers, dst)?;
        return Ok(completion.update_empty(value));
    }
    Ok(completion)
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
    let body_code = body.clone();
    let Some(body) = body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let pending = crate::value::AsyncForOfState {
        owner: crate::generator::current_generator_id(),
        label: label.clone(),
        slot,
        body: body_code,
        per_iteration,
        iteration_slots: iteration_slots.to_vec(),
        iterator: iterator.clone(),
        dst,
        await_dst: 0,
        await_values,
        bindings: Vec::new(),
        body_pc: 0,
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
        let (body_result, body_pc) = if await_values {
            execute_async_loop_body(registers, body, label, 0)?
        } else {
            execute_async_loop_body(registers, body, label, 0)?
        };
        match body_result {
            crate::completion::LoopTransition::Continue(_) => {}
            crate::completion::LoopTransition::Break(_) => {
                return crate::collections::iterator::close(
                    iterator,
                    crate::completion::Completion::Normal,
                );
            }
            crate::completion::LoopTransition::Propagate(completion) => {
                if completion.is_suspension() {
                    if matches!(completion, crate::completion::Completion::Suspend(_)) {
                        let mut pending = pending.clone();
                        pending.body_pc = body_pc;
                        pending.await_dst = body_await_destination(body, body_pc);
                        pending.bindings = capture_iteration_bindings(slot, iteration_slots);
                        remember_pending_async_for_of(pending);
                    } else {
                        remember_for_of(iterator);
                    }
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
    let Some(body) = spec.body.code() else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let mut body_pc = spec.body_pc;
    let mut next_value = if body_pc == 0 {
        crate::collections::iterator::resume_async_result(&spec.iterator, input)?
    } else {
        None
    };
    loop {
        if body_pc != 0 {
            let _binding = crate::locals::IterationBinding::install_cells(spec.bindings.clone());
            let (completion, next_pc) =
                execute_async_loop_body(registers, body, &spec.label, body_pc)?;
            match completion {
                crate::completion::LoopTransition::Continue(_) => {
                    body_pc = 0;
                }
                crate::completion::LoopTransition::Break(_) => {
                    return crate::collections::iterator::close(
                        spec.iterator.clone(),
                        crate::completion::Completion::Normal,
                    )
                    .map(|completion| (completion, None));
                }
                crate::completion::LoopTransition::Propagate(completion) => {
                    if completion.is_suspension() {
                        if matches!(completion, crate::completion::Completion::Suspend(_)) {
                            let mut pending = spec.clone();
                            pending.body_pc = next_pc;
                            pending.await_dst = body_await_destination(body, next_pc);
                            pending.bindings =
                                capture_iteration_bindings(spec.slot, &spec.iteration_slots);
                            return Ok((completion, Some(pending)));
                        }
                        remember_for_of(spec.iterator.clone());
                        return Ok((completion, None));
                    }
                    let completion = attach_loop_completion(registers, spec.dst, completion)?;
                    return crate::collections::iterator::close(spec.iterator.clone(), completion)
                        .map(|completion| (completion, None));
                }
            }
            next_value = match step_iterator_value(&spec.iterator, spec.await_values) {
                Ok(value) => value,
                Err(crate::execute::VmError::Suspended(promise)) => {
                    let mut pending = spec.clone();
                    pending.body_pc = 0;
                    return Ok((
                        crate::completion::Completion::Suspend(promise),
                        Some(pending),
                    ));
                }
                Err(error) => return Err(error),
            };
        }
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
        let (body_completion, body_next_pc) =
            execute_async_loop_body(registers, body, &spec.label, 0)?;
        match body_completion {
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
                    if matches!(completion, crate::completion::Completion::Suspend(_)) {
                        let mut pending = spec.clone();
                        pending.body_pc = body_next_pc;
                        pending.await_dst = body_await_destination(body, body_next_pc);
                        pending.bindings =
                            capture_iteration_bindings(spec.slot, &spec.iteration_slots);
                        return Ok((completion, Some(pending)));
                    }
                    remember_for_of(spec.iterator.clone());
                    return Ok((completion, None));
                }
                let completion = attach_loop_completion(registers, spec.dst, completion)?;
                return crate::collections::iterator::close(spec.iterator.clone(), completion)
                    .map(|completion| (completion, None));
            }
        }
        next_value = match step_iterator_value(&spec.iterator, spec.await_values) {
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

fn step_iterator_value(
    iterator: &crate::value::Value,
    await_values: bool,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    if await_values {
        crate::collections::iterator::step_value_await(iterator)
    } else {
        crate::collections::iterator::step_value(iterator)
    }
}

fn body_await_destination(body: crate::machine::CodeView<'_>, body_pc: usize) -> u16 {
    body_pc
        .checked_sub(1)
        .and_then(|index| body.cold_at(index))
        .and_then(|op| match op {
            crate::ops::Op::Await { dst, .. } => Some(*dst),
            _ => None,
        })
        .unwrap_or(0)
}

fn capture_iteration_bindings(
    slot: u16,
    iteration_slots: &[u16],
) -> Vec<(u16, std::rc::Rc<crate::value::BindingCell>)> {
    let slots = std::iter::once(slot)
        .chain(iteration_slots.iter().copied())
        .collect::<Vec<_>>();
    slots.into_iter()
        .filter_map({
            let mut last = None;
            move |slot| {
                if last == Some(slot) {
                    return None;
                }
                last = Some(slot);
                Some((slot, crate::locals::current().capture_slot_cell(slot)))
            }
        })
        .collect()
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

/// Execute an async-for-of body while retaining the exact body offset when an
/// await suspends. The ordinary loop helper intentionally discards that
/// offset because synchronous loops resume through iterator frames; async
/// loops own a compact state record instead.
fn execute_async_loop_body(
    registers: &mut crate::register_file::RegisterFile,
    body: crate::machine::CodeView<'_>,
    label: &Option<String>,
    start: usize,
) -> Result<(crate::completion::LoopTransition, usize), crate::execute::VmError> {
    let mut pc = start;
    loop {
        let step = {
            let defer = crate::module_bindings::fulfilled_await_defers();
            crate::module_bindings::defer_fulfilled_await(false);
            let result = crate::vm::execute_code_completion_step_from_in_place(body, pc, registers);
            crate::module_bindings::defer_fulfilled_await(defer);
            result?
        };
        pc = step.next;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                if let Err(crate::execute::VmError::Thrown(value)) =
                    crate::vm::vm_ops::execute_call_continuation(registers, continuation)
                {
                    return Ok((
                        crate::completion::LoopTransition::Propagate(
                            crate::completion::Completion::Throw(value),
                        ),
                        pc,
                    ));
                }
            }
            completion => {
                return Ok((
                    crate::completion::Completion::into_loop_transition(completion, label),
                    pc,
                ));
            }
        }
    }
}

fn execute_loop_body_with_context(
    registers: &mut crate::register_file::RegisterFile,
    label: &Option<String>,
    body: crate::machine::CodeView<'_>,
    context: &crate::vm::VmContext,
) -> Result<crate::completion::LoopTransition, crate::execute::VmError> {
    Ok(crate::completion::Completion::into_loop_transition(
        crate::vm::execute_code_completion_with_context(body, registers, context)?,
        label,
    ))
}

include!("loops_run.rs");
include!("loops_pair_walk.rs");
include!("loops_regexp_exec.rs");
include!("loops_body.rs");
include!("loops_while.rs");

#[cfg(test)]
mod tests {
    use super::{live_for_of, reset_fixture_state, take_live_for_of, LIVE_FOR_OF};
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
    fn fixture_reset_clears_live_for_of_stack() {
        LIVE_FOR_OF.with(|live| live.push(Value::Number(1.0)));
        reset_fixture_state();
        assert_eq!(live_for_of(), None);
    }
}
