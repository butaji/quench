use crate::execute::{run_vm, VmError};
use crate::value::Value;

pub(crate) fn builtin(
    _builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let program = crate::reduce::reduce_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    run_vm(&program.ops).map_err(|error| VmError::EvalError(format!("{error:?}")))
}
