use crate::{
    execute::{execute_in_place, VmError},
    ops::Op,
    value::Value,
};

/// Execute a `try`/`catch`/`finally` container.
///
/// Returns `Ok(Some(value))` when the body (or handler) performed an explicit
/// `return`, `Ok(None)` when it completed normally, and `Err` when an
/// unhandled exception escaped.
pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    let Op::Try {
        body,
        handler,
        finalizer,
        catch_slot,
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let body_result = execute_in_place(body, registers);
    let result = match body_result {
        Ok(value) => Some(value),
        Err(VmError::MissingReturn) => None,
        Err(VmError::Thrown(value)) => match handler {
            Some(ops) => {
                bind_caught(value, *catch_slot, registers);
                match execute_in_place(ops, registers) {
                    Ok(value) => Some(value),
                    Err(VmError::MissingReturn) => None,
                    Err(error) => return Err(error),
                }
            }
            None => return Err(VmError::Thrown(value)),
        },
        Err(error) => return Err(error),
    };
    if let Some(finalizer) = finalizer {
        match execute_in_place(finalizer, registers) {
            Ok(_) | Err(VmError::MissingReturn) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(result)
}

fn bind_caught(value: Value, catch_slot: Option<u16>, registers: &mut Vec<Value>) {
    if let Some(slot) = catch_slot {
        let _ = registers;
        crate::locals::write(slot, value);
    }
}
