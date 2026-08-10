use crate::{
    facts::ProgramDb,
    ops::{FunctionKind, FunctionStrictness, Op},
};
use std::collections::HashMap;
const NEW_TARGET: &str = "\0new_target";
#[derive(Clone, Copy)]
pub(crate) struct FunctionMetadata {
    pub(crate) kind: FunctionKind,
    pub(crate) strictness: FunctionStrictness,
    pub(crate) is_async: bool,
    pub(crate) mapped_arguments: bool,
}
pub(super) fn function_parameters(
    function: &oxc::ast::ast::Function<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    crate::function_parameters::bindings(&function.params)
}
pub(crate) fn reduce_body(
    body: &oxc::ast::ast::FunctionBody<'_>,
    formal: &oxc::ast::ast::FormalParameters<'_>,
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    parameter_count: u16,
    captures: u16,
    metadata: FunctionMetadata,
) -> Result<Vec<Op>, Vec<String>> {
    let formal_parameters = crate::function_parameters::bindings(formal)?.0;
    let rest = rest_slot(&parameters, parameter_count, captures);
    let minimum = captures
        .saturating_add(parameter_count)
        .saturating_add(if rest.is_some() { 4 } else { 3 });
    let next_slot = crate::reduce_support::register_base(&parameters).max(minimum);
    let tail_calls = tail_calls_enabled(metadata.strictness, metadata.kind, metadata.is_async);
    let inherited = enter_function(facts, metadata.strictness, tail_calls);
    let prefix = crate::function_parameters::prefix(formal, facts, &parameters, captures, true);
    let mut body_locals = parameters.clone();
    crate::reduce_support::shadow_function_bindings(
        &body.statements,
        &mut body_locals,
        &formal_parameters,
    );
    let inherited_barrier = std::mem::take(&mut facts.eval_var_barrier);
    let reduced = crate::reduce::reduce_statements_with_locals(
        &body.statements,
        facts,
        body_locals,
        next_slot,
    );
    facts.eval_var_barrier = inherited_barrier;
    (facts.strict, facts.in_function, facts.tail_calls) = inherited;
    let mut prefix =
        prefix.ok_or_else(|| vec!["Unsupported function parameter initialization".to_string()])?;
    prefix.extend((!prefix.is_empty()).then_some(Op::ParameterEnd));
    prefix.extend(reduced?);
    Ok(bind_rest(prefix, rest, parameter_count, captures))
}
fn enter_function(
    facts: &mut ProgramDb,
    strictness: FunctionStrictness,
    tail_calls: bool,
) -> (bool, bool, bool) {
    let inherited = (facts.strict, facts.in_function, facts.tail_calls);
    facts.strict = matches!(strictness, FunctionStrictness::Strict);
    facts.in_function = true;
    facts.tail_calls = tail_calls;
    inherited
}
fn rest_slot(parameters: &HashMap<String, u16>, params: u16, captures: u16) -> Option<u16> {
    let slot = captures
        .saturating_add(params)
        .saturating_add(2 + u16::from(parameters.contains_key(NEW_TARGET)));
    parameters.values().copied().find(|value| *value == slot)
}
fn bind_rest(mut body: Vec<Op>, rest: Option<u16>, params: u16, captures: u16) -> Vec<Op> {
    let Some(slot) = rest else { return body };
    let arguments = captures.saturating_add(params);
    let mut prefix = vec![
        Op::LoadLocal {
            dst: 0,
            slot: arguments,
        },
        Op::Const {
            dst: 1,
            value: crate::ops::Constant::Number(f64::from(params)),
        },
        Op::CallMethod {
            dst: 2,
            object: 0,
            key: "slice".to_string(),
            args: vec![1],
        },
        Op::StoreLocal { slot, src: 2 },
    ];
    prefix.append(&mut body);
    prefix
}
fn capture_count(locals: &HashMap<String, u16>) -> u16 {
    crate::reduce_support::register_base(locals)
}
pub(super) fn function_bindings(
    mut parameters: HashMap<String, u16>,
    parameter_count: u16,
    captures: u16,
    locals: &HashMap<String, u16>,
    lexical_receiver: bool,
) -> HashMap<String, u16> {
    if !lexical_receiver {
        let shifted = parameter_count.saturating_add(2);
        parameters
            .values_mut()
            .filter(|slot| **slot >= shifted)
            .for_each(|slot| *slot = slot.saturating_add(1));
    }
    for slot in parameters.values_mut() {
        *slot = slot.saturating_add(captures);
    }
    let mut bindings = locals.clone();
    if !lexical_receiver {
        let base = captures.saturating_add(parameter_count);
        for (name, offset) in [("arguments", 0), ("this", 1), (NEW_TARGET, 2)] {
            bindings.insert(name.to_string(), base.saturating_add(offset));
        }
    }
    bindings.extend(parameters);
    bindings
}
pub(super) fn reduce_function_ops(
    statements: &[oxc::ast::ast::Statement<'_>],
    formal: &oxc::ast::ast::FormalParameters<'_>,
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    parameter_count: u16,
    locals: &HashMap<String, u16>,
    arrow_expression: Option<bool>,
) -> Option<(Vec<Op>, u16)> {
    let captures = capture_count(locals);
    let (parameters, body_locals) = split_function_locals(
        statements,
        parameters,
        parameter_count,
        captures,
        locals,
        arrow_expression.is_some(),
    );
    let rest = rest_slot(&parameters, parameter_count, captures);
    let mut prefix = crate::function_parameters::prefix(
        formal,
        facts,
        &parameters,
        captures,
        arrow_expression.is_none(),
    )?;
    let local_count = function_local_count(&body_locals, parameter_count, captures, rest);
    let inherited_barrier = std::mem::take(&mut facts.eval_var_barrier);
    let body_ops = reduce_selected_body(
        statements,
        facts,
        body_locals,
        local_count,
        arrow_expression.unwrap_or(false),
    );
    facts.eval_var_barrier = inherited_barrier;
    prefix.extend((!prefix.is_empty()).then_some(Op::ParameterEnd));
    prefix.extend(body_ops?);
    Some((bind_rest(prefix, rest, parameter_count, captures), captures))
}
fn split_function_locals(
    statements: &[oxc::ast::ast::Statement<'_>],
    parameters: HashMap<String, u16>,
    count: u16,
    captures: u16,
    locals: &HashMap<String, u16>,
    lexical_receiver: bool,
) -> (HashMap<String, u16>, HashMap<String, u16>) {
    let formal = parameters.clone();
    let parameters = function_bindings(parameters, count, captures, locals, lexical_receiver);
    let mut body = parameters.clone();
    crate::reduce_support::shadow_function_bindings(statements, &mut body, &formal);
    (parameters, body)
}
fn function_local_count(
    locals: &HashMap<String, u16>,
    count: u16,
    captures: u16,
    rest: Option<u16>,
) -> u16 {
    let minimum = captures
        .saturating_add(count)
        .saturating_add(2 + u16::from(locals.contains_key(NEW_TARGET)))
        .saturating_add(u16::from(rest.is_some()));
    crate::reduce_support::register_base(locals).max(minimum)
}

fn reduce_selected_body(
    statements: &[oxc::ast::ast::Statement<'_>],
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    local_count: u16,
    expression_body: bool,
) -> Option<Vec<Op>> {
    if expression_body {
        crate::reduce::reduce_expression_statements_with_locals(
            statements,
            facts,
            parameters,
            local_count,
        )
        .ok()
    } else {
        crate::reduce::reduce_statements_with_locals(statements, facts, parameters, local_count)
            .ok()
    }
}

fn emit_function_op(
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    body: Vec<Op>,
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeFunctionWithKind {
        dst: register,
        body,
        params,
        captures,
        kind: metadata.kind,
        strictness: metadata.strictness,
        is_async: metadata.is_async,
        mapped_arguments: metadata.mapped_arguments,
    });
    register
}

pub(crate) fn reduce_expression(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let body = function.body.as_ref()?;
    let strictness = crate::reduce_support::function_strictness(body, facts.strict);
    let (parameters, parameter_count) =
        crate::function_parameters::bindings(&function.params).ok()?;
    let kind = function_kind(function);
    let tail_calls = tail_calls_enabled(strictness, kind, function.r#async);
    let inherited = enter_function(facts, strictness, tail_calls);
    let reduced = reduce_function_ops(
        &body.statements,
        &function.params,
        facts,
        parameters,
        parameter_count,
        locals,
        None,
    );
    (facts.strict, facts.in_function, facts.tail_calls) = inherited;
    let (body_ops, captures) = reduced?;
    Some(emit_function_op(
        ops,
        next_register,
        body_ops,
        parameter_count,
        captures,
        FunctionMetadata {
            kind,
            strictness,
            is_async: function.r#async,
            mapped_arguments: crate::function_parameters::is_simple(&function.params),
        },
    ))
}

pub(crate) fn function_kind(function: &oxc::ast::ast::Function<'_>) -> FunctionKind {
    if function.generator {
        FunctionKind::Generator
    } else {
        FunctionKind::Ordinary
    }
}

pub(crate) fn tail_calls_enabled(
    strictness: FunctionStrictness,
    kind: FunctionKind,
    is_async: bool,
) -> bool {
    matches!(strictness, FunctionStrictness::Strict)
        && matches!(kind, FunctionKind::Ordinary | FunctionKind::Arrow)
        && !is_async
}

pub(crate) fn reduce_arrow(
    function: &oxc::ast::ast::ArrowFunctionExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let strictness = crate::reduce_support::function_strictness(&function.body, facts.strict);
    let (parameters, parameter_count) =
        crate::function_parameters::bindings(&function.params).ok()?;
    let tail_calls = tail_calls_enabled(strictness, FunctionKind::Arrow, function.r#async);
    let inherited = enter_function(facts, strictness, tail_calls);
    let reduced = reduce_function_ops(
        &function.body.statements,
        &function.params,
        facts,
        parameters,
        parameter_count,
        locals,
        Some(function.expression),
    );
    (facts.strict, facts.in_function, facts.tail_calls) = inherited;
    let (body_ops, captures) = reduced?;
    Some(emit_function_op(
        ops,
        next_register,
        body_ops,
        parameter_count,
        captures,
        FunctionMetadata {
            kind: FunctionKind::Arrow,
            strictness,
            is_async: function.r#async,
            mapped_arguments: false,
        },
    ))
}

pub(super) fn make(
    body: &[Op],
    params: u16,
    captures: std::rc::Rc<crate::environment::Environment>,
    kind: FunctionKind,
    strictness: FunctionStrictness,
    is_async: bool,
    mapped_arguments: bool,
) -> crate::value::Value {
    let value = crate::value::Value::Function(std::rc::Rc::new(crate::value::FunctionValue {
        body: body.to_vec(),
        params,
        captures,
        properties: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        kind,
        strictness,
        is_async,
        mapped_arguments,
    }));
    let prototype = crate::value::Value::Object(std::rc::Rc::new(vec![(
        "constructor".to_string(),
        value.clone(),
    )]));
    if let crate::value::Value::Function(ref function) = value {
        function
            .properties
            .borrow_mut()
            .push(("prototype".to_string(), prototype));
    }
    value
}

pub(crate) fn write(
    registers: &mut Vec<crate::value::Value>,
    dst: u16,
    body: &[Op],
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
) {
    let value = make(
        body,
        params,
        crate::locals::capture(captures),
        metadata.kind,
        metadata.strictness,
        metadata.is_async,
        metadata.mapped_arguments,
    );
    if matches!(metadata.kind, FunctionKind::Ordinary) {
        if let crate::value::Value::Function(function) = &value {
            function.properties.borrow_mut().push((
                "\0ordinary_function".to_string(),
                crate::value::Value::Boolean(true),
            ));
        }
    }
    crate::execute::write_value(registers, dst, value);
}

pub(crate) fn write_op(registers: &mut Vec<crate::value::Value>, op: &Op) {
    crate::functions_write::write_op(registers, op);
}

include!("functions_arguments.rs");

pub(crate) fn execute_bound(
    bound: &crate::value::BoundFunctionValue,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    match &bound.target {
        crate::value::Value::Builtin(builtin) => {
            execute_builtin_target(*builtin, Some(&bound.receiver), &combined)
        }
        crate::value::Value::Function(function) => execute(function, &bound.receiver, &combined),
        crate::value::Value::BoundFunction(next) => execute_bound(next, &combined),
        crate::value::Value::Proxy(_) => {
            crate::proxy::proxy_apply(&bound.target, &bound.receiver, &combined)
        }
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

pub(crate) fn execute_target(
    target: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match target {
        crate::value::Value::Builtin(builtin) => {
            execute_builtin_target(*builtin, Some(receiver), arguments)
        }
        crate::value::Value::Function(function) => execute(function, receiver, arguments),
        crate::value::Value::BoundFunction(bound) => execute_bound(bound, arguments),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn execute_builtin_target(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if let crate::ops::Builtin::HostCapability(kind) = builtin {
        return crate::vm::execute_host_capability(kind, receiver, arguments);
    }
    crate::execute::execute_builtin_with_receiver(builtin, arguments, receiver)
}

fn execute_function_call(
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let receiver = receiver.ok_or(crate::execute::VmError::NotCallable)?;
    let this = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::Undefined);
    execute_target(receiver, &this, arguments.get(1..).unwrap_or_default())
}

fn bind_function_target(
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    if !matches!(
        receiver,
        Some(
            crate::value::Value::Builtin(_)
                | crate::value::Value::Function(_)
                | crate::value::Value::BoundFunction(_)
                | crate::value::Value::Proxy(_)
        )
    ) {
        return Err(crate::execute::VmError::NotCallable);
    }
    let target = arguments
        .first()
        .ok_or(crate::execute::VmError::NotCallable)?;
    Ok(crate::value::Value::BoundFunction(std::rc::Rc::new(
        crate::value::BoundFunctionValue {
            target: receiver.cloned().unwrap_or(crate::value::Value::Undefined),
            receiver: target.clone(),
            arguments: arguments[1..].to_vec(),
        },
    )))
}

pub(crate) fn function_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    match builtin {
        crate::ops::Builtin::FunctionCall => execute_function_call(receiver, arguments),
        crate::ops::Builtin::FunctionBind => bind_function_target(receiver, arguments),
        crate::ops::Builtin::ArrayJoin => Ok(crate::builtins::array_join(receiver, arguments)),
        crate::ops::Builtin::ArrayPush => Ok(crate::builtins::array_push(receiver, arguments)),
        crate::ops::Builtin::ObjectPropertyIsEnumerable => Ok(
            crate::builtins::object::object_property_is_enumerable(receiver, arguments),
        ),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}
