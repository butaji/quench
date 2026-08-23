use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};

/// Execute a `try`/`catch`/`finally` container.
pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<Completion, VmError> {
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
                bind_caught(value, *catch_slot, registers);
                crate::vm::execute_code_completion_in_current_frame(ops, registers)?
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
    registers: &mut Vec<Value>,
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
    registers: &mut Vec<Value>,
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
    registers: &mut Vec<Value>,
) -> Result<Completion, VmError> {
    execute_try_body(ops, registers)
}

fn execute_try_body(
    ops: crate::machine::CodeView<'_>,
    registers: &mut Vec<Value>,
) -> Result<Completion, VmError> {
    crate::vm::execute_code_completion_in_current_frame(ops, registers)
}

fn bind_caught(value: Value, catch_slot: Option<u16>, registers: &mut Vec<Value>) {
    if let Some(slot) = catch_slot {
        crate::execute::write_value(registers, slot, value.clone());
        crate::locals::write(slot, value);
    }
}
