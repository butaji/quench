use crate::{
    completion::Completion,
    execute::{execute_completion_in_place, VmError},
    ops::Op,
    value::Value,
};

/// Execute a `try`/`catch`/`finally` container.
///
pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<Completion, VmError> {
    let Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let body_completion = execute_completion_in_place(body, registers)?;
    let completion = match body_completion {
        Completion::Throw(value) => match handler {
            Some(ops) => {
                bind_caught(value, *catch_slot, registers);
                execute_completion_in_place(ops, registers)?
            }
            None => Completion::Throw(value),
        },
        completion => completion,
    };
    if let Some(finalizer) = finalizer {
        match execute_completion_in_place(finalizer, registers)? {
            Completion::Normal => {}
            abrupt => return Ok(abrupt),
        }
    }
    Ok(completion)
}

fn bind_caught(value: Value, catch_slot: Option<u16>, registers: &mut Vec<Value>) {
    if let Some(slot) = catch_slot {
        let _ = registers;
        crate::locals::write(slot, value);
    }
}
