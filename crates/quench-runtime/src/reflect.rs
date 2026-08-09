use crate::execute::{run_vm, VmError};
use crate::value::Value;

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let values = match arguments.get(1) {
        Some(Value::Array(values)) => values.as_ref().clone(),
        _ => Vec::new(),
    };
    crate::construct::construct_value(target, &values)
}

pub(crate) fn builtin(builtin: crate::ops::Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    if builtin == crate::ops::Builtin::ReflectConstruct {
        return construct(arguments);
    }
    let Some(Value::String(source)) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let program = crate::reduce::reduce_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    run_vm(&program.ops).map_err(|error| VmError::EvalError(format!("{error:?}")))
}
