use crate::{
    facts::ProgramDb,
    ops::{FunctionKind, FunctionStrictness, Op},
};
use std::collections::HashMap;
include!("functions_properties.rs");
const NEW_TARGET: &str = "\0new_target";
pub(crate) const FUNCTION_SELF: &str = "\0function_self";
#[derive(Clone, Copy)]
pub(crate) struct FunctionMetadata {
    pub(crate) kind: FunctionKind,
    pub(crate) length: u16,
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
    let lexical_receiver = matches!(metadata.kind, FunctionKind::Arrow);
    let rest = rest_slot(
        &parameters,
        parameter_count,
        captures,
        lexical_receiver,
        false,
    );
    let minimum = captures
        .saturating_add(parameter_count)
        .saturating_add(reserved_slots(lexical_receiver, rest));
    let next_slot = crate::reduce_support::register_base(&parameters).max(minimum);
    let tail_calls = tail_calls_enabled(metadata.strictness, metadata.kind, metadata.is_async);
    let inherited = enter_function(facts, metadata.strictness, tail_calls);
    let prefix = crate::function_parameters::prefix(formal, facts, &parameters, captures, true);
    let reduced = reduce_body_statements(body, formal, facts, &parameters, next_slot)?;
    (facts.strict, facts.in_function, facts.tail_calls) = inherited;
    let mut prefix =
        prefix.ok_or_else(|| vec!["Unsupported function parameter initialization".to_string()])?;
    prefix.extend((!prefix.is_empty()).then_some(Op::ParameterEnd));
    prefix.extend(reduced);
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
fn rest_slot(
    parameters: &HashMap<String, u16>,
    params: u16,
    captures: u16,
    lexical_receiver: bool,
    reserve_self: bool,
) -> Option<u16> {
    let slot = captures
        .saturating_add(params)
        .saturating_add(if lexical_receiver {
            2
        } else {
            3 + u16::from(reserve_self)
        });
    parameters.values().copied().find(|value| *value == slot)
}

fn reserved_slots(lexical_receiver: bool, rest: Option<u16>) -> u16 {
    (if lexical_receiver { 2_u16 } else { 3_u16 }).saturating_add(u16::from(rest.is_some()))
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
    reduce_function_ops_named(
        statements,
        formal,
        facts,
        (parameters, parameter_count),
        locals,
        arrow_expression,
        None,
    )
}

fn reduce_function_ops_named(
    statements: &[oxc::ast::ast::Statement<'_>],
    formal: &oxc::ast::ast::FormalParameters<'_>,
    facts: &mut ProgramDb,
    parameters: (HashMap<String, u16>, u16),
    locals: &HashMap<String, u16>,
    arrow_expression: Option<bool>,
    self_name: Option<&str>,
) -> Option<(Vec<Op>, u16)> {
    let (parameters, parameter_count) = parameters;
    let lexical_receiver = arrow_expression.is_some();
    let (parameters, body_locals, captures, rest) = prepare_function_scope(
        statements,
        locals,
        (parameters, parameter_count),
        lexical_receiver,
        self_name,
    );
    let prefix = reduce_function_body(
        (statements, formal),
        facts,
        (&parameters, body_locals),
        (parameter_count, captures),
        arrow_expression,
        rest,
    )?;
    Some((bind_rest(prefix, rest, parameter_count, captures), captures))
}

fn reduce_function_body(
    syntax: (
        &[oxc::ast::ast::Statement<'_>],
        &oxc::ast::ast::FormalParameters<'_>,
    ),
    facts: &mut ProgramDb,
    locals: (&HashMap<String, u16>, HashMap<String, u16>),
    layout: (u16, u16),
    arrow_expression: Option<bool>,
    rest: Option<u16>,
) -> Option<Vec<Op>> {
    let (statements, formal) = syntax;
    let (parameters, body_locals) = locals;
    let captures = layout.1;
    let mut prefix = crate::function_parameters::prefix(
        formal,
        facts,
        parameters,
        captures,
        arrow_expression.is_none(),
    )?;
    let local_count = function_local_count(&body_locals, layout, rest, arrow_expression.is_some());
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
    Some(prefix)
}

fn prepare_function_scope(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &HashMap<String, u16>,
    parameters: (HashMap<String, u16>, u16),
    lexical_receiver: bool,
    self_name: Option<&str>,
) -> (HashMap<String, u16>, HashMap<String, u16>, u16, Option<u16>) {
    let (parameters, count) = parameters;
    let captures = capture_count(locals);
    let (parameters, mut body) = split_function_locals(
        statements,
        parameters,
        count,
        captures,
        locals,
        lexical_receiver,
        self_name.is_some(),
    );
    if let Some(name) = self_name {
        body.insert(
            name.to_string(),
            captures.saturating_add(count.saturating_add(3)),
        );
    }
    let rest = rest_slot(
        &parameters,
        count,
        captures,
        lexical_receiver,
        self_name.is_some(),
    );
    (parameters, body, captures, rest)
}
fn split_function_locals(
    statements: &[oxc::ast::ast::Statement<'_>],
    parameters: HashMap<String, u16>,
    count: u16,
    captures: u16,
    locals: &HashMap<String, u16>,
    lexical_receiver: bool,
    reserve_self: bool,
) -> (HashMap<String, u16>, HashMap<String, u16>) {
    let formal = parameters.clone();
    let mut parameters = function_bindings(parameters, count, captures, locals, lexical_receiver);
    if reserve_self {
        let first_auxiliary = captures.saturating_add(count.saturating_add(3));
        parameters
            .values_mut()
            .filter(|slot| **slot >= first_auxiliary)
            .for_each(|slot| *slot = slot.saturating_add(1));
    }
    let mut body = parameters.clone();
    crate::reduce_support::shadow_function_bindings(statements, &mut body, &formal);
    (parameters, body)
}
fn function_local_count(
    locals: &HashMap<String, u16>,
    layout: (u16, u16),
    rest: Option<u16>,
    lexical_receiver: bool,
) -> u16 {
    let (count, captures) = layout;
    let minimum = captures
        .saturating_add(count)
        .saturating_add(reserved_slots(lexical_receiver, rest));
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
        length: metadata.length,
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
    let tail_calls = tail_calls_enabled(strictness, function_kind(function), function.r#async);
    let inherited = enter_function(facts, strictness, tail_calls);
    let reduced = reduce_function_ops_named(
        &body.statements,
        &function.params,
        facts,
        (parameters, parameter_count),
        locals,
        None,
        function.id.as_ref().map(|id| id.name.as_str()),
    );
    (facts.strict, facts.in_function, facts.tail_calls) = inherited;
    let (body_ops, captures) = reduced?;
    Some(emit_function_expression(
        ops,
        next_register,
        body_ops,
        parameter_count,
        captures,
        FunctionMetadata {
            kind: function_kind(function),
            length: crate::function_parameters::expected_argument_count(&function.params),
            strictness,
            is_async: function.r#async,
            mapped_arguments: crate::function_parameters::is_simple(&function.params),
        },
        function.id.as_ref().map(|id| id.name.as_str()),
    ))
}

pub(crate) fn function_kind(function: &oxc::ast::ast::Function<'_>) -> FunctionKind {
    if function.generator {
        FunctionKind::Generator
    } else {
        FunctionKind::Ordinary
    }
}

include!("functions_tail.rs");

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
            length: crate::function_parameters::expected_argument_count(&function.params),
            strictness,
            is_async: function.r#async,
            mapped_arguments: false,
        },
    ))
}

pub(super) fn make(
    body: &[Op],
    params: u16,
    length: u16,
    captures: std::rc::Rc<crate::environment::Environment>,
    metadata: FunctionMetadata,
) -> crate::value::Value {
    let value = crate::value::Value::Function(std::rc::Rc::new(crate::value::FunctionValue {
        body: body.to_vec(),
        params,
        captures,
        properties: function_properties(length),
        instance_fields: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        kind: metadata.kind,
        strictness: metadata.strictness,
        is_async: metadata.is_async,
        mapped_arguments: metadata.mapped_arguments,
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
        metadata.length,
        crate::locals::capture(captures),
        metadata,
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
