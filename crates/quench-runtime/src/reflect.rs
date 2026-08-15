use crate::execute::VmError;
use crate::value::Value;

pub(crate) fn builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let realm = crate::vm::realm_id_for_intrinsic_receiver(receiver);
    let result = evaluate(value, false, realm)?;
    if builtin == crate::ops::Builtin::ShadowRealmEvaluate
        && crate::conversion::is_callable(&result)
    {
        return wrap_shadow_function(&result);
    }
    Ok(result)
}

fn wrap_shadow_function(target: &Value) -> Result<Value, VmError> {
    let name = match crate::execute::get_property_result(target, "name")? {
        Value::String(value) if !crate::conversion::is_symbol_string(&value) => value,
        _ => String::new(),
    };
    let length = match crate::execute::get_property_result(target, "length")? {
        Value::Number(value) if value.is_finite() => value.max(0.0).trunc(),
        Value::Number(value) if value.is_infinite() && value.is_sign_positive() => value,
        _ => 0.0,
    };
    let properties = vec![
        ("name".to_string(), Value::String(name.clone())),
        (
            crate::builtins::descriptor_key("name"),
            name_descriptor(&name),
        ),
        ("length".to_string(), Value::Number(length)),
        (
            crate::builtins::descriptor_key("length"),
            length_descriptor(length),
        ),
    ];
    Ok(Value::BoundFunction(std::rc::Rc::new(
        crate::value::BoundFunctionValue {
            target: target.clone(),
            receiver: Value::Undefined,
            arguments: Vec::new(),
            properties: std::cell::RefCell::new(properties),
        },
    )))
}

fn name_descriptor(value: &str) -> Value {
    descriptor(Value::String(value.to_string()))
}

fn length_descriptor(value: f64) -> Value {
    descriptor(Value::Number(value))
}

fn descriptor(value: Value) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
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
            input.reusable_var_names,
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
    pub(crate) reusable_var_names: &'a [String],
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
        Some(realm) => crate::vm::execute_indirect_eval_in_realm(realm, program.ops()),
        None => crate::vm::execute_indirect_eval(program.ops()),
    }
}

fn evaluate_direct(
    value: &Value,
    strict: bool,
    global: bool,
    bindings: &[(String, u16)],
    reusable_var_names: &[String],
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
        bindings,
        reusable_var_names,
        forbidden_var_names,
        grammar,
    )
    .map_err(syntax_error)?;
    execute_direct_eval(program.ops(), program.facts.strict)
}

fn execute_direct_eval(ops: &[crate::ops::Op], strict: bool) -> Result<Value, VmError> {
    let environment = crate::environment::Environment::child(&crate::locals::current(), Vec::new());
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
