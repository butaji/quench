use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};

/// Execute a `try`/`catch`/`finally` container.
pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<Completion, VmError> {
    let Op::Try { body, .. } = op else {
        return Err(VmError::MissingReturn);
    };
    let body_step = execute_try_body(body, registers)?;
    if body_step.completion.is_suspension() {
        return wrap_try_suspension(op, crate::machine::TryPhase::Body, body.range, body_step);
    }
    finish_try_completion(registers, op, body_step.completion)
}

pub(crate) fn finish_try_completion(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
    body_completion: Completion,
) -> Result<Completion, VmError> {
    let Op::Try {
        handler,
        finalizer,
        catch_slot,
        dst,
        finally_dst,
        ..
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let completion = match body_completion {
        Completion::Throw(value) => match handler {
            Some(ops) => {
                let previous = bind_caught(value, *catch_slot, registers);
                let step = execute_try_body(ops, registers)?;
                if let Some((slot, cell)) = previous {
                    crate::locals::current().restore_slot(slot, cell);
                }
                if step.completion.is_suspension() {
                    return wrap_try_suspension(
                        op,
                        crate::machine::TryPhase::Catch,
                        ops.range,
                        step,
                    );
                }
                step.completion
            }
            None => Completion::Throw(value),
        },
        completion => completion,
    };
    if let Some(abrupt) = run_finalizer(finalizer, registers)? {
        return finish_abrupt_finally(registers, *dst, *finally_dst, abrupt);
    }
    Ok(completion.update_empty(Value::Undefined))
}

fn finish_abrupt_finally(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    finally_dst: Option<u16>,
    abrupt: Completion,
) -> Result<Completion, VmError> {
    let value = match finally_dst {
        Some(slot) => crate::execute::read_register(registers, slot)?,
        None => Value::Undefined,
    };
    crate::execute::write_value(registers, dst, value.clone());
    Ok(abrupt.update_empty(value))
}

fn run_finalizer(
    finalizer: &Option<crate::machine::FunctionCode>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Option<Completion>, VmError> {
    let Some(finalizer) = finalizer else {
        return Ok(None);
    };
    match crate::vm::execute_function_code_completion_in_current_frame(finalizer, registers)? {
        Completion::Normal => Ok(None),
        abrupt => Ok(Some(abrupt)),
    }
}

pub(crate) fn execute_ops(
    ops: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Completion, VmError> {
    crate::vm::execute_code_completion_in_current_frame(ops, registers)
}

fn execute_try_body(
    ops: &crate::machine::FunctionCode,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<crate::vm::CompletionStep, VmError> {
    let code = ops.code().ok_or(VmError::MissingReturn)?;
    let step = crate::vm::execute_function_code_completion_step_in_current_frame(ops, registers)?;
    crate::continuation::attach_executed_suspension(code, step)
}

fn wrap_try_suspension(
    op: &Op,
    phase: crate::machine::TryPhase,
    range: crate::machine::CodeRange,
    step: crate::vm::CompletionStep,
) -> Result<Completion, VmError> {
    let Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
        ..
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let inner = step
        .completion
        .suspension_point()
        .cloned()
        .ok_or(VmError::MissingReturn)?;
    let yield_dst = inner.destination();
    let next = u32::try_from(step.next).map_err(|_| VmError::MissingReturn)?;
    let body_resume = range
        .start
        .checked_add(next)
        .ok_or(VmError::MissingReturn)?;
    if body_resume > range.end {
        return Err(VmError::MissingReturn);
    }
    let outer = crate::continuation::SuspensionPoint::Try {
        phase,
        body: body.range,
        handler: handler.as_ref().map(|code| code.range),
        finalizer: finalizer.as_ref().map(|code| code.range),
        body_resume: crate::machine::CodeRange {
            start: body_resume,
            ..range
        },
        yield_dst,
        catch_slot: *catch_slot,
    };
    Ok(step.completion.nest_suspension(outer))
}

fn bind_caught(
    value: Value,
    catch_slot: Option<u16>,
    registers: &mut crate::register_file::RegisterFile,
) -> Option<(u16, std::rc::Rc<crate::value::BindingCell>)> {
    if let Some(slot) = catch_slot {
        crate::execute::write_value(registers, slot, value.clone());
        let previous = crate::locals::current().replace_slot(slot, value);
        return Some((slot, previous));
    }
    None
}

#[cfg(test)]
#[path = "stencil_exception_boundary_tests.rs"]
mod boundary_tests;
