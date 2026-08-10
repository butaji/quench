use std::{collections::HashMap, convert::TryFrom};

use oxc::ast::ast::BindingPatternKind;

use crate::{
    facts::ProgramDb,
    ops::{FunctionKind, FunctionStrictness, Op},
};

pub(super) fn strictness(body: &oxc::ast::ast::FunctionBody<'_>) -> FunctionStrictness {
    if body
        .directives
        .iter()
        .any(|directive| directive.directive.as_str() == "use strict")
    {
        FunctionStrictness::Strict
    } else {
        FunctionStrictness::Sloppy
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FunctionMetadata {
    kind: FunctionKind,
    strictness: FunctionStrictness,
    is_async: bool,
}

pub(super) fn function_parameters(
    function: &oxc::ast::ast::Function<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    parameters(&function.params)
}

fn parameters(
    formal: &oxc::ast::ast::FormalParameters<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    let mut parameters = HashMap::new();
    for (slot, parameter) in formal.items.iter().enumerate() {
        let BindingPatternKind::BindingIdentifier(identifier) = &parameter.pattern.kind else {
            return Err(vec!["Unsupported function parameter pattern".to_string()]);
        };
        let slot =
            u16::try_from(slot).map_err(|_| vec!["Too many function parameters".to_string()])?;
        parameters.insert(identifier.name.to_string(), slot);
    }
    let count = u16::try_from(formal.items.len())
        .map_err(|_| vec!["Too many function parameters".to_string()])?;
    Ok((parameters, count))
}

pub(crate) fn reduce_body(
    body: &oxc::ast::ast::FunctionBody<'_>,
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    parameter_count: u16,
    captures: u16,
) -> Result<Vec<Op>, Vec<String>> {
    crate::reduce::reduce_statements_with_locals(
        &body.statements,
        facts,
        parameters,
        captures.saturating_add(parameter_count).saturating_add(2),
    )
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
) -> HashMap<String, u16> {
    for slot in parameters.values_mut() {
        *slot = slot.saturating_add(captures);
    }
    parameters.insert(
        "arguments".to_string(),
        captures.saturating_add(parameter_count),
    );
    parameters.insert(
        "this".to_string(),
        captures.saturating_add(parameter_count).saturating_add(1),
    );
    parameters.extend(locals.iter().map(|(name, slot)| (name.clone(), *slot)));
    parameters
}

pub(super) fn reduce_function_ops(
    statements: &[oxc::ast::ast::Statement<'_>],
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    parameter_count: u16,
    locals: &HashMap<String, u16>,
) -> Option<(Vec<Op>, u16)> {
    let captures = capture_count(locals);
    let parameters = extend_function_parameters(parameters, parameter_count, captures, locals);
    let local_count = captures.saturating_add(parameter_count).saturating_add(2);
    let body_ops =
        crate::reduce::reduce_statements_with_locals(statements, facts, parameters, local_count)
            .ok()?;
    Some((body_ops, captures))
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
    let (parameters, parameter_count) = function_parameters(function).ok()?;
    let (body_ops, captures) =
        reduce_function_ops(&body.statements, facts, parameters, parameter_count, locals)?;
    Some(emit_function_op(
        ops,
        next_register,
        body_ops,
        parameter_count,
        captures,
        FunctionMetadata {
            kind: if function.generator {
                FunctionKind::Generator
            } else {
                FunctionKind::Ordinary
            },
            strictness: strictness(body),
            is_async: function.r#async,
        },
    ))
}

pub(crate) fn reduce_arrow(
    function: &oxc::ast::ast::ArrowFunctionExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (parameters, parameter_count) = parameters(&function.params).ok()?;
    let (body_ops, captures) = reduce_function_ops(
        &function.body.statements,
        facts,
        parameters,
        parameter_count,
        locals,
    )?;
    Some(emit_function_op(
        ops,
        next_register,
        body_ops,
        parameter_count,
        captures,
        FunctionMetadata {
            kind: FunctionKind::Arrow,
            strictness: FunctionStrictness::Sloppy,
            is_async: function.r#async,
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
) -> crate::value::Value {
    let value = crate::value::Value::Function(std::rc::Rc::new(crate::value::FunctionValue {
        body: body.to_vec(),
        params,
        captures,
        properties: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        kind,
        strictness,
        is_async,
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

pub(crate) fn dynamic_constructor(
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    crate::functions_dynamic::construct(arguments)
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
    match op {
        Op::MakeFunction {
            dst,
            body,
            params,
            captures,
            strictness,
            is_async,
        } => write_ordinary(
            registers,
            *dst,
            body,
            *params,
            *captures,
            *strictness,
            *is_async,
        ),
        Op::MakeFunctionWithKind {
            dst,
            body,
            params,
            captures,
            kind,
            strictness,
            is_async,
        } => write_kind(
            registers,
            (*dst, body, *params, *captures),
            FunctionMetadata {
                kind: *kind,
                strictness: *strictness,
                is_async: *is_async,
            },
        ),
        _ => {}
    }
}

fn write_kind(
    registers: &mut Vec<crate::value::Value>,
    function: (u16, &[Op], u16, u16),
    metadata: FunctionMetadata,
) {
    let (dst, body, params, captures) = function;
    write(registers, dst, body, params, captures, metadata);
}

fn write_ordinary(
    registers: &mut Vec<crate::value::Value>,
    dst: u16,
    body: &[Op],
    params: u16,
    captures: u16,
    strictness: FunctionStrictness,
    is_async: bool,
) {
    let metadata = FunctionMetadata {
        kind: FunctionKind::Ordinary,
        strictness,
        is_async,
    };
    write(registers, dst, body, params, captures, metadata);
}

fn build_registers(
    function: &crate::value::FunctionValue,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> (
    Vec<crate::value::Value>,
    std::rc::Rc<crate::environment::Environment>,
) {
    let original_arguments = arguments.to_vec();
    let mut parameters = arguments.to_vec();
    parameters.resize(usize::from(function.params), crate::value::Value::Undefined);
    parameters.truncate(usize::from(function.params));
    parameters.push(crate::value::Value::Array(std::rc::Rc::new(
        original_arguments,
    )));
    parameters.push(this_value.clone());
    let environment = crate::environment::Environment::child(&function.captures, parameters);
    (vec![crate::value::Value::Undefined; 32], environment)
}

pub(crate) fn execute(
    function: &crate::value::FunctionValue,
    this_value: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let this_value = crate::vm::bare_call_receiver(function, this_value);
    let (mut registers, environment) = build_registers(function, &this_value, arguments);
    let completion = crate::vm::execute_in_environment(
        &function.body,
        &mut registers,
        &crate::vm::VmContext::default(),
        environment,
    );
    if function.is_async {
        return Ok(crate::promise::from_async_completion(completion));
    }
    completion
}

/// Execute a constructor, returning both its result and the object bound to
/// `this` after it ran (so `this.message = ...` mutations are preserved).
pub(crate) fn execute_construct(
    function: &crate::value::FunctionValue,
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

pub(crate) fn is_constructible(function: &crate::value::FunctionValue) -> bool {
    match (function.kind, function.strictness, function.is_async) {
        (FunctionKind::Ordinary, FunctionStrictness::Sloppy, false)
        | (FunctionKind::Ordinary, FunctionStrictness::Strict, false) => true,
        (FunctionKind::Arrow, _, _)
        | (FunctionKind::Generator, _, _)
        | (FunctionKind::Ordinary, _, true) => false,
    }
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
