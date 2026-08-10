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
    global: bool,
    bindings: &[(String, u16)],
    forbidden_var_names: &[String],
) -> Result<(), VmError> {
    let source = crate::execute::read_register(registers, source)?;
    let value = evaluate_direct(
        &source,
        strict,
        global,
        bindings,
        forbidden_var_names,
        registers,
    )?;
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

fn evaluate(value: &Value, strict: bool) -> Result<Value, VmError> {
    let Value::String(source) = value else {
        return Ok(value.clone());
    };
    let bindings = vec![
        ("globalThis".to_string(), 0),
        ("\0script_this".to_string(), 0),
    ];
    let program = crate::reduce::reduce_eval_source(source, strict, true, &bindings, &[])
        .map_err(syntax_error)?;
    crate::vm::execute_indirect_eval(&program.ops)
}

fn evaluate_direct(
    value: &Value,
    strict: bool,
    global: bool,
    bindings: &[(String, u16)],
    forbidden_var_names: &[String],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let Value::String(source) = value else {
        return Ok(value.clone());
    };
    let program =
        crate::reduce::reduce_eval_source(source, strict, global, bindings, forbidden_var_names)
            .map_err(syntax_error)?;
    if program.facts.strict {
        return execute_strict_eval(&program.ops, registers);
    }
    crate::execute::execute_in_place(&program.ops, registers)
}

fn execute_strict_eval(
    ops: &[crate::ops::Op],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let count = u16::try_from(crate::locals::current().len())
        .map_err(|_| VmError::EvalError("Too many eval bindings".to_string()))?;
    let captures = crate::locals::capture(count);
    let environment = crate::environment::Environment::child(&captures, Vec::new());
    let _guard = crate::locals::EnvironmentGuard::install(environment);
    crate::execute::execute_in_place(ops, registers)
}

fn syntax_error(errors: Vec<String>) -> VmError {
    VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::SyntaxError,
        &[Value::String(errors.join("; "))],
    ))
}
