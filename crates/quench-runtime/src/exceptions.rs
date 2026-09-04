use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};

/// Execute a `try`/`catch`/`finally` container.
pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<Completion, VmError> {
    let Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
        dst,
        finally_dst,
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let Some(body) = body.code() else {
        return Err(VmError::MissingReturn);
    };
    let body_completion = execute_try_body(body, registers)?;
    if body_completion.is_suspension() {
        return Ok(body_completion);
    }
    let completion = match body_completion {
        Completion::Throw(value) => match handler {
            Some(ops) => {
                let Some(ops) = ops.code() else {
                    return Err(VmError::MissingReturn);
                };
                let previous = bind_caught(value, *catch_slot, registers);
                let completion =
                    crate::vm::execute_code_completion_in_current_frame(ops, registers)?;
                if let Some((slot, cell)) = previous {
                    crate::locals::current().restore_slot(slot, cell);
                }
                completion
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
    let Some(finalizer) = finalizer.code() else {
        return Err(VmError::MissingReturn);
    };
    match crate::vm::execute_code_completion_in_current_frame(finalizer, registers)? {
        Completion::Normal => Ok(None),
        abrupt => Ok(Some(abrupt)),
    }
}

pub(crate) fn execute_ops(
    ops: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Completion, VmError> {
    execute_try_body(ops, registers)
}

fn execute_try_body(
    ops: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Completion, VmError> {
    match crate::vm::execute_code_completion_in_current_frame(ops, registers) {
        // Calls are driven through a separate continuation. Convert a host
        // exception back into the completion form consumed by this try
        // container, preserving JavaScript catch semantics across that edge.
        Err(VmError::Thrown(value)) => Ok(Completion::Throw(value)),
        result => result,
    }
}

fn bind_caught(
    value: Value,
    catch_slot: Option<u16>,
    registers: &mut crate::register_file::RegisterFile,
) -> Option<(u16, std::rc::Rc<crate::value::BindingCell>)> {
    if let Some(slot) = catch_slot {
        crate::execute::write_value(registers, slot, value.clone());
        let environment = crate::locals::current();
        let previous = environment.replace_slot(slot, value.clone());
        environment.set(slot, value);
        return Some((slot, previous));
    }
    None
}
