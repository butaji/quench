use crate::execute::VmError;
use crate::value::Value;

pub(crate) fn builtin(
    _builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    evaluate(value, false)
}

pub(crate) fn execute_eval(
    registers: &mut Vec<Value>,
    dst: u16,
    source: u16,
    strict: bool,
    bindings: &[(String, u16)],
) -> Result<(), VmError> {
    let source = crate::execute::read_register(registers, source)?;
    let value = evaluate_direct(&source, strict, bindings, registers)?;
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

fn evaluate(value: &Value, strict: bool) -> Result<Value, VmError> {
    let Value::String(source) = value else {
        return Ok(value.clone());
    };
    let script = crate::reduce::ScriptSource { source, strict };
    let program = crate::reduce::reduce_script_sources(&[script]).map_err(syntax_error)?;
    crate::execute::run_vm(&program.ops)
}

fn evaluate_direct(
    value: &Value,
    strict: bool,
    bindings: &[(String, u16)],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let Value::String(source) = value else {
        return Ok(value.clone());
    };
    let program =
        crate::reduce::reduce_eval_source(source, strict, bindings).map_err(syntax_error)?;
    crate::execute::execute_in_place(&program.ops, registers)
}

fn syntax_error(errors: Vec<String>) -> VmError {
    VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::SyntaxError,
        &[Value::String(errors.join("; "))],
    ))
}
