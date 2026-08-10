use std::collections::HashMap;

use crate::{
    facts::ProgramDb,
    ops::{FunctionKind, FunctionStrictness, Op},
};

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
    strictness: FunctionStrictness,
) -> Result<Vec<Op>, Vec<String>> {
    let rest = rest_slot(&parameters, parameter_count, captures);
    let minimum = captures
        .saturating_add(parameter_count)
        .saturating_add(if rest.is_some() { 3 } else { 2 });
    let next_slot = crate::reduce_support::register_base(&parameters).max(minimum);
    let inherited = facts.strict;
    facts.strict = matches!(strictness, FunctionStrictness::Strict);
    let mut prefix = crate::function_parameters::prefix(formal, facts, &parameters, captures)
        .ok_or_else(|| vec!["Unsupported function parameter initialization".to_string()])?;
    let reduced = crate::reduce::reduce_statements_with_locals(
        &body.statements,
        facts,
        parameters,
        next_slot,
    );
    facts.strict = inherited;
    prefix.extend(reduced?);
    Ok(bind_rest(prefix, rest, parameter_count, captures))
}

fn rest_slot(parameters: &HashMap<String, u16>, params: u16, captures: u16) -> Option<u16> {
    let slot = captures.saturating_add(params).saturating_add(2);
    parameters
        .values()
        .any(|value| *value == slot)
        .then_some(slot)
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
    locals
        .values()
        .copied()
        .max()
        .map_or(0, |slot| slot.saturating_add(1))
}

fn extend_function_parameters(
    mut parameters: HashMap<String, u16>,
    parameter_count: u16,
    captures: u16,
    locals: &HashMap<String, u16>,
    lexical_receiver: bool,
) -> HashMap<String, u16> {
    for slot in parameters.values_mut() {
        *slot = slot.saturating_add(captures);
    }
    let mut bindings = locals.clone();
    if !lexical_receiver {
        bindings.insert(
            "arguments".to_string(),
            captures.saturating_add(parameter_count),
        );
        bindings.insert(
            "this".to_string(),
            captures.saturating_add(parameter_count).saturating_add(1),
        );
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
    let formal_parameters = parameters.clone();
    let mut parameters = extend_function_parameters(
        parameters,
        parameter_count,
        captures,
        locals,
        arrow_expression.is_some(),
    );
    crate::reduce_support::shadow_function_bindings(
        statements,
        &mut parameters,
        &formal_parameters,
    );
    let rest = rest_slot(&parameters, parameter_count, captures);
    let mut prefix = crate::function_parameters::prefix(formal, facts, &parameters, captures)?;
    let minimum = captures
        .saturating_add(parameter_count)
        .saturating_add(2)
        .saturating_add(u16::from(rest.is_some()));
    let local_count = crate::reduce_support::register_base(&parameters).max(minimum);
    let body_ops = reduce_selected_body(
        statements,
        facts,
        parameters,
        local_count,
        arrow_expression.unwrap_or(false),
    )?;
    prefix.extend(body_ops);
    Some((bind_rest(prefix, rest, parameter_count, captures), captures))
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
    let (parameters, parameter_count) = function_parameters(function).ok()?;
    let inherited = facts.strict;
    facts.strict = matches!(strictness, FunctionStrictness::Strict);
    let reduced = reduce_function_ops(
        &body.statements,
        &function.params,
        facts,
        parameters,
        parameter_count,
        locals,
        None,
    );
    facts.strict = inherited;
    let (body_ops, captures) = reduced?;
    Some(emit_function_op(
        ops,
        next_register,
        body_ops,
        parameter_count,
        captures,
        FunctionMetadata {
            kind: function_kind(function),
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
    let inherited = facts.strict;
    facts.strict = matches!(strictness, FunctionStrictness::Strict);
    let reduced = reduce_function_ops(
        &function.body.statements,
        &function.params,
        facts,
        parameters,
        parameter_count,
        locals,
        Some(function.expression),
    );
    facts.strict = inherited;
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

/// Execute a constructor and return its result plus the final `this` value.
pub(crate) fn execute_construct(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<(crate::value::Value, crate::value::Value), crate::execute::VmError> {
    let this_slot = function.captures.len() as u16 + function.params + 1;
    let (mut registers, environment) = build_registers(function, this_value, arguments);
    let result = crate::vm::execute_in_environment(
        &function.body,
        &mut registers,
        &crate::vm::VmContext::default(),
        environment.clone(),
    )?;
    let final_this = environment.get(this_slot);
    Ok((result, final_this))
}

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
