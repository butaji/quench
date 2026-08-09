use crate::{
    execute::{execute_with_registers, VmError},
    ops::Op,
    value::Value,
};

pub(crate) fn execute(registers: &[Value], op: &Op) -> Result<(), VmError> {
    let Op::Try {
        body,
        handler,
        finalizer,
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let result = execute_with_registers(body, registers.to_vec());
    let result = match result {
        Ok(value) => Ok(value),
        Err(error) => handler
            .as_ref()
            .map(|ops| execute_with_registers(ops.as_slice(), registers.to_vec()))
            .unwrap_or(Err(error)),
    };
    if let Some(finalizer) = finalizer {
        execute_with_registers(finalizer, registers.to_vec())?;
    }
    result.map(|_| ())
}
