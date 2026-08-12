use crate::execute::VmError;
use crate::value::Value;

pub(crate) fn builtin(
    _builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let realm = crate::vm::realm_id_for_intrinsic_receiver(receiver);
    evaluate(value, false, realm)
}

pub(crate) fn execute_eval(
    registers: &mut Vec<Value>,
    input: EvalExecution<'_>,
) -> Result<(), VmError> {
    let source = crate::execute::read_register(registers, input.source)?;
    let value = if input.direct {
        evaluate_direct(
            &source,
            input.strict,
            input.global,
            input.bindings,
            input.forbidden_var_names,
        )?
    } else {
        evaluate(&source, input.strict, None)?
    };
    crate::execute::write_value(registers, input.dst, value);
    Ok(())
}

pub(crate) struct EvalExecution<'a> {
    pub(crate) dst: u16,
    pub(crate) source: u16,
    pub(crate) strict: bool,
    pub(crate) global: bool,
    pub(crate) direct: bool,
    pub(crate) bindings: &'a [(String, u16)],
    pub(crate) forbidden_var_names: &'a [String],
}

fn evaluate(
    value: &Value,
    strict: bool,
    realm: Option<crate::ops::RealmId>,
) -> Result<Value, VmError> {
    let Value::String(source) = value else {
        return Ok(value.clone());
    };
    let bindings = vec![
        ("globalThis".to_string(), 0),
        ("\0script_this".to_string(), 0),
    ];
    let program = crate::reduce::reduce_eval_source(source, strict, true, false, &bindings, &[])
        .map_err(syntax_error)?;
    match realm {
        Some(realm) => crate::vm::execute_indirect_eval_in_realm(realm, &program.ops),
        None => crate::vm::execute_indirect_eval(&program.ops),
    }
}

fn evaluate_direct(
    value: &Value,
    strict: bool,
    global: bool,
    bindings: &[(String, u16)],
    forbidden_var_names: &[String],
) -> Result<Value, VmError> {
    let Value::String(source) = value else {
        return Ok(value.clone());
    };
    let grammar = crate::semantic::EvalGrammarContext {
        new_target: bindings.iter().any(|(name, _)| name == "\0new_target"),
        super_property: crate::super_scope::is_active(),
    };
    let program = crate::reduce::reduce_statements::reduce_eval_source_in_context(
        source,
        strict,
        global,
        true,
        bindings,
        forbidden_var_names,
        grammar,
    )
    .map_err(syntax_error)?;
    execute_direct_eval(&program.ops, program.facts.strict)
}

fn execute_direct_eval(ops: &[crate::ops::Op], strict: bool) -> Result<Value, VmError> {
    let count = u16::try_from(crate::locals::current().len())
        .map_err(|_| VmError::EvalError("Too many eval bindings".to_string()))?;
    let captures = crate::locals::capture(count);
    let environment = crate::environment::Environment::child(&captures, Vec::new());
    let _guard = crate::locals::EnvironmentGuard::install(environment);
    let _strict_eval = crate::locals::StrictEvalGuard::install(strict);
    crate::execute::execute_in_place(ops, &mut Vec::new())
}

fn syntax_error(errors: Vec<String>) -> VmError {
    VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::SyntaxError,
        &[Value::String(errors.join("; "))],
    ))
}
