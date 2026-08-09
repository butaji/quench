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
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let body_result = execute_in_place(body, registers);
    let result = match body_result {
        Ok(value) => Some(value),
        Err(VmError::MissingReturn) => None,
        Err(error) => match handler {
            Some(ops) => match execute_in_place(ops, registers) {
                Ok(value) => Some(value),
                Err(VmError::MissingReturn) => None,
                Err(error) => return Err(error),
            },
            None => return Err(error),
        },
    };
    if let Some(finalizer) = finalizer {
        let _ = execute_in_place(finalizer, registers);
    }
    Ok(result)
}
