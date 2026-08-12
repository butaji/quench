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
    let body_completion = execute_try_body(body, registers)?;
    if body_completion.is_suspension() {
        return Ok(body_completion);
    }
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

fn execute_try_body(ops: &[Op], registers: &mut Vec<Value>) -> Result<Completion, VmError> {
    for op in ops {
        if matches!(op, Op::YieldStar { .. }) {
            match crate::generator::execute_yield_star(registers, op, Completion::Normal) {
                Ok(Some(completion)) => return Ok(completion),
                Ok(None) => {}
                Err(VmError::Thrown(value)) => return Ok(Completion::Throw(value)),
                Err(error) => return Err(error),
            }
            continue;
        }
        match execute_completion_in_place(std::slice::from_ref(op), registers)? {
            Completion::Normal => {}
            completion => return Ok(completion),
        }
    }
    Ok(Completion::Normal)
}

fn bind_caught(value: Value, catch_slot: Option<u16>, registers: &mut Vec<Value>) {
    if let Some(slot) = catch_slot {
        crate::execute::write_value(registers, slot, value.clone());
        crate::locals::write(slot, value);
    }
}
