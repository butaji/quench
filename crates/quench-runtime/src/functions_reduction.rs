pub(crate) struct ReducedFunctionOps {
    pub(crate) body: Vec<Op>,
    pub(crate) captures: u16,
    pub(crate) frame_register_count: u16,
}

struct ReducedFunctionBody {
    body: Vec<Op>,
    frame_register_count: u16,
}

pub(super) fn reduce_function_ops(
    statements: &[oxc::ast::ast::Statement<'_>],
    formal: &oxc::ast::ast::FormalParameters<'_>,
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    parameter_count: u16,
    locals: &HashMap<String, u16>,
    arrow_expression: Option<bool>,
) -> Option<ReducedFunctionOps> {
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
    self_name: Option<(&str, bool)>,
) -> Option<ReducedFunctionOps> {
    let (parameters, parameter_count) = parameters;
    let lexical_receiver = arrow_expression.is_some();
    let (parameters, body_locals, captures, rest) = prepare_function_scope(
        statements,
        locals,
        (parameters, parameter_count),
        lexical_receiver,
        self_name.map(|(name, _)| name),
    );
    let self_name_slot = self_name
        .and_then(|(name, immutable)| immutable.then(|| body_locals.get(name)).flatten().copied());
    let reduced = reduce_function_body(
        (statements, formal),
        facts,
        (&parameters, body_locals),
        (parameter_count, captures),
        arrow_expression,
        rest,
        self_name_slot,
    )?;
    Some(ReducedFunctionOps {
        body: bind_rest(reduced.body, rest, parameter_count, captures),
        captures,
        frame_register_count: reduced.frame_register_count,
    })
}

pub(crate) fn reduce_named_declaration(
    body: &oxc::ast::ast::FunctionBody<'_>,
    formal: &oxc::ast::ast::FormalParameters<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
    name: &str,
    kind: FunctionKind,
    is_async: bool,
) -> Result<ReducedFunctionOps, Vec<String>> {
    let (parameters, parameter_count) = crate::function_parameters::bindings(formal)?;
    let strictness = crate::reduce_support::function_strictness(body, facts.strict);
    let tail_calls = tail_calls_enabled(strictness, kind, is_async);
    let inherited = enter_function(facts, strictness, tail_calls);
    let self_name = (!locals.contains_key(name)).then_some((name, false));
    let reduced = reduce_function_ops_named(
        &body.statements,
        formal,
        facts,
        (parameters, parameter_count),
        locals,
        None,
        self_name,
    );
    (
        facts.strict,
        facts.in_function,
        facts.tail_calls,
        facts.function_dynamic_scope_floor,
    ) = inherited;
    reduced.ok_or_else(|| vec!["Unsupported function declaration body".to_string()])
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
    self_name_slot: Option<u16>,
) -> Option<ReducedFunctionBody> {
    let captured_name_slot = locals
        .1
        .values()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let inherited = (
        facts.eval_var_scope_start,
        facts.eval_arrow_scope,
        facts.function_has_direct_eval,
    );
    facts.eval_var_scope_start = layout.1;
    facts.eval_arrow_scope = arrow_expression.is_some();
    // A direct eval in a default/rest initializer runs while the parameter
    // environment is active.  Treat that initializer as part of the
    // function's dynamic scope so closures created there cannot resolve
    // unresolved names through the global fast path.
    facts.function_has_direct_eval = inherited.2
        || function_has_direct_eval(syntax.0)
        || function_has_direct_eval_formal(syntax.1);
    let previous_name_slot = facts.function_name_slot;
    facts.function_name_slot = self_name_slot
        .or_else(|| previous_name_slot.filter(|slot| captured_name_slot.contains(slot)));
    let result = reduce_function_body_inner(syntax, facts, locals, layout, arrow_expression, rest);
    (
        facts.eval_var_scope_start,
        facts.eval_arrow_scope,
        facts.function_has_direct_eval,
    ) = inherited;
    facts.function_name_slot = previous_name_slot;
    result
}

fn function_has_direct_eval(statements: &[oxc::ast::ast::Statement<'_>]) -> bool {
    let mut finder = DirectEvalFinder(false);
    for statement in statements {
        oxc::ast::visit::walk::walk_statement(&mut finder, statement);
    }
    finder.0
}

fn function_has_direct_eval_formal(formal: &oxc::ast::ast::FormalParameters<'_>) -> bool {
    let mut finder = DirectEvalFinder(false);
    oxc::ast::visit::walk::walk_formal_parameters(&mut finder, formal);
    finder.0
}

struct DirectEvalFinder(bool);

impl<'a> oxc::ast::visit::Visit<'a> for DirectEvalFinder {
    fn visit_call_expression(&mut self, call: &oxc::ast::ast::CallExpression<'a>) {
        if matches!(&call.callee, oxc::ast::ast::Expression::Identifier(id) if id.name == "eval") {
            self.0 = true;
        } else {
            oxc::ast::visit::walk::walk_call_expression(self, call);
        }
    }

    fn visit_function(
        &mut self,
        _: &oxc::ast::ast::Function<'a>,
        _: oxc::syntax::scope::ScopeFlags,
    ) {
    }

    fn visit_arrow_function_expression(
        &mut self,
        _: &oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) {
    }
}

fn reduce_function_body_inner(
    syntax: (
        &[oxc::ast::ast::Statement<'_>],
        &oxc::ast::ast::FormalParameters<'_>,
    ),
    facts: &mut ProgramDb,
    locals: (&HashMap<String, u16>, HashMap<String, u16>),
    layout: (u16, u16),
    arrow_expression: Option<bool>,
    rest: Option<u16>,
) -> Option<ReducedFunctionBody> {
    let (statements, formal) = syntax;
    let (parameters, body_locals) = locals;
    let captures = layout.1;
    let ordered = ordered_function_declarations_first(statements);
    let (mut prefix, prefix_register_count) = crate::function_parameters::prefix(
        formal,
        facts,
        parameters,
        captures,
        arrow_expression.is_none(),
    )?;
    let local_count = function_local_count(&body_locals, layout, rest, arrow_expression.is_some());
    let inherited_barrier = facts.eval_var_barrier.clone();
    let inherited_formals = facts.eval_formals.clone();
    facts.eval_formals.extend(crate::function_parameters::eval_var_barrier(
        formal,
        arrow_expression.is_none(),
    ));
    let body_ops = crate::switch::with_unobservable_completion(|| {
        reduce_selected_body(
            statements,
            &ordered,
            facts,
            body_locals,
            local_count,
            arrow_expression.unwrap_or(false),
        )
    });
    facts.eval_var_barrier = inherited_barrier;
    facts.eval_formals = inherited_formals;
    prefix.extend((!prefix.is_empty()).then_some(Op::ParameterEnd));
    let body = body_ops?;
    prefix.extend(body.body);
    Some(ReducedFunctionBody {
        body: prefix,
        frame_register_count: prefix_register_count.max(body.frame_register_count),
    })
}

fn prepare_function_scope(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &HashMap<String, u16>,
    parameters: (HashMap<String, u16>, u16),
    lexical_receiver: bool,
    self_name: Option<&str>,
) -> (HashMap<String, u16>, HashMap<String, u16>, u16, Option<u16>) {
    let (parameters, count) = parameters;
    let has_rest = parameters.contains_key("\0rest");
    let captures = capture_count(locals);
    let (mut parameters, mut body) = split_function_locals(
        statements,
        parameters,
        count,
        captures,
        locals,
        lexical_receiver,
        self_name.is_some(),
    );
    if let Some(name) = self_name {
        let slot = captures.saturating_add(count.saturating_add(3));
        parameters.insert(name.to_string(), slot);
        body.insert(name.to_string(), slot);
    }
    let rest = has_rest.then(|| rest_slot(&parameters)).flatten();
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
    let mut shadow = formal;
    if !lexical_receiver {
        // `arguments` is an implicit parameter binding for ordinary
        // functions, but inherited outer names must remain hidden while
        // parameter initializers are evaluated.
        shadow.insert("arguments".to_string(), 0);
    }
    crate::reduce_support::shadow_function_bindings(statements, &mut body, &shadow);
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
    order: &[usize],
    facts: &mut ProgramDb,
    parameters: HashMap<String, u16>,
    local_count: u16,
    expression_body: bool,
) -> Option<ReducedFunctionBody> {
    let ordered = ordered_body_statements(statements, order)?;
    let mut locals = parameters;
    let mut ops = Vec::new();
    let mut last = None;
    let mut next_register = 0;
    let mut next_slot = local_count;
    crate::reduce_support::predeclare_functions(statements, &mut locals, &mut next_slot, facts.strict);
    crate::reduce_support::predeclare_lexicals(statements, &mut locals, &mut next_slot);
    let stack = crate::using_scope::reserve(statements, &mut locals, &mut next_slot);
    crate::using_scope::emit_tdz(statements, &mut ops, &locals);
    let await_using = crate::using_scope::has_await_using(statements);
    let barrier_len = facts.eval_var_barrier.len();
    facts
        .eval_var_barrier
        .extend(crate::semantic_early::lexically_declared_names_in(
            statements,
        ));
    if let Some(stack) = stack {
        crate::using_scope::emit_create(&mut ops, stack, await_using, &mut next_register);
    }
    for statement in ordered {
        let Some(value) = evaluate_statement(
            statement,
            facts,
            &mut ops,
            &mut next_register,
            &mut next_slot,
            &mut locals,
        ) else {
            facts.eval_var_barrier.truncate(barrier_len);
            return None;
        };
        last = value.or(last);
    }
    facts.eval_var_barrier.truncate(barrier_len);
    let ops = match stack {
        Some(stack) => crate::using_scope::wrap(ops, stack, await_using, &mut next_register).ok()?,
        None => ops,
    };
    Some(ReducedFunctionBody {
        body: finalize_function_body(ops, last, expression_body)?,
        frame_register_count: next_register,
    })
}

fn ordered_body_statements<'a>(
    statements: &'a [oxc::ast::ast::Statement<'a>],
    order: &[usize],
) -> Option<Vec<&'a oxc::ast::ast::Statement<'a>>> {
    let mut ordered = Vec::with_capacity(order.len());
    for index in order {
        ordered.push(statements.get(*index)?);
    }
    Some(ordered)
}

fn evaluate_statement(
    statement: &oxc::ast::ast::Statement<'_>,
    facts: &mut ProgramDb,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Option<Option<u16>> {
    crate::reduce::reduce_statement(statement, ops, facts, next_register, next_slot, locals).ok()
}

fn finalize_function_body(
    mut ops: Vec<Op>,
    last: Option<u16>,
    expression_body: bool,
) -> Option<Vec<Op>> {
    if expression_body {
        crate::reduce_support::finish_program(ops, last).ok()
    } else {
        ops.push(Op::Const {
            dst: 0,
            value: crate::ops::Constant::Undefined,
        });
        ops.push(Op::Return { src: 0 });
        Some(ops)
    }
}

fn emit_function_op(
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    body: Vec<Op>,
    frame_register_count: u16,
    params: u16,
    captures: u16,
    metadata: FunctionMetadata,
    source: Option<String>,
) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeFunctionWithKind {
        dst: register,
        body: crate::machine::FunctionCode::pending_with_declared_frame(
            body,
            frame_register_count,
        ).with_facts(crate::facts::FunctionFacts {
            direct_constructor: metadata.direct_constructor.clone(),
            composed_constructor: metadata.composed_constructor.clone(),
        }),
        params,
        captures,
        length: metadata.length,
        kind: metadata.kind,
        strictness: metadata.strictness,
        is_async: metadata.is_async,
        mapped_arguments: metadata.mapped_arguments,
        source,
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
    reduce_expression_kind(function, ops, facts, next_register, locals, None)
}

pub(crate) fn reduce_expression_kind(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
    kind: Option<FunctionKind>,
) -> Option<u16> {
    reduce_expression_kind_source(function, ops, facts, next_register, locals, kind, None)
}

pub(crate) fn reduce_expression_kind_source(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
    kind: Option<FunctionKind>,
    source_override: Option<String>,
) -> Option<u16> {
    let body = function.body.as_ref()?;
    let strictness = crate::reduce_support::function_strictness(body, facts.strict);
    let (parameters, parameter_count) =
        crate::function_parameters::bindings(&function.params).ok()?;
    let emitted_kind = kind.unwrap_or_else(|| function_kind(function));
    let tail_calls = tail_calls_enabled(strictness, emitted_kind, function.r#async);
    let inherited = enter_function(facts, strictness, tail_calls);
    let reduced = reduce_function_ops_named(
        &body.statements,
        &function.params,
        facts,
        (parameters, parameter_count),
        locals,
        None,
        function.id.as_ref().map(|id| (id.name.as_str(), true)),
    );
    (
        facts.strict,
        facts.in_function,
        facts.tail_calls,
        facts.function_dynamic_scope_floor,
    ) = inherited;
    let reduced = reduced?;
    let source = source_override.or_else(|| {
        facts
            .reduction_source
            .get(function.span.start as usize..function.span.end as usize)
            .map(str::to_string)
    });
    Some(emit_function_expression(
        ops,
        next_register,
        reduced.body,
        reduced.frame_register_count,
        parameter_count,
        reduced.captures,
        FunctionMetadata {
            kind: emitted_kind,
            length: crate::function_parameters::expected_argument_count(&function.params),
            strictness,
            is_async: function.r#async,
            mapped_arguments: crate::function_parameters::is_simple(&function.params),
            direct_constructor: direct_constructor_fact(function, locals),
            composed_constructor: composed_constructor_fact(function, locals),
        },
        function.id.as_ref().map(|id| id.name.as_str()),
        source,
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
    let (reduced, strictness) = reduce_arrow_body(function, facts, locals)?;
    Some(emit_function_op(
        ops,
        next_register,
        reduced.body,
        reduced.frame_register_count,
        crate::function_parameters::bindings(&function.params)
            .map(|(_, count)| count)
            .ok()?,
        reduced.captures,
        FunctionMetadata {
            kind: FunctionKind::Arrow,
            length: crate::function_parameters::expected_argument_count(&function.params),
            strictness,
            is_async: function.r#async,
            mapped_arguments: false,
            direct_constructor: std::rc::Rc::default(),
            composed_constructor: std::rc::Rc::default(),
        },
        facts
            .reduction_source
            .get(function.span.start as usize..function.span.end as usize)
            .map(str::to_string),
    ))
}

fn reduce_arrow_body(
    function: &oxc::ast::ast::ArrowFunctionExpression<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
) -> Option<(ReducedFunctionOps, crate::ops_meta::FunctionStrictness)> {
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
    (
        facts.strict,
        facts.in_function,
        facts.tail_calls,
        facts.function_dynamic_scope_floor,
    ) = inherited;
    Some((reduced?, strictness))
}

pub(super) fn make(
    code: crate::machine::FunctionCode,
    params: u16,
    length: u16,
    captures: std::rc::Rc<crate::environment::Environment>,
    metadata: FunctionMetadata,
) -> crate::value::Value {
    let has_prototype = matches!(metadata.kind, FunctionKind::Generator)
        || matches!(metadata.kind, FunctionKind::Ordinary) && !metadata.is_async;
    let kind = metadata.kind;
    let value = make_function_value(code, params, captures, length, metadata);
    attach_lexical_super(&value, kind);
    if has_prototype {
        attach_prototype(&value);
    }
    if let crate::value::Value::Function(function) = &value {
        crate::cycle_collector::track_function(function);
        for captured in function.cycle_values() {
            crate::cycle_collector::track_value(&captured);
        }
    }
    value
}

fn make_function_value(
    code: crate::machine::FunctionCode,
    params: u16,
    captures: std::rc::Rc<crate::environment::Environment>,
    length: u16,
    metadata: FunctionMetadata,
) -> crate::value::Value {
    crate::execution_trace::function_lifecycle(true);
    crate::execution_trace::function_shape(code.capture_slots().len(), code.len(), true);
    let function_realm = crate::vm::realm_id_for_global_value(&captures.get(0))
        .or_else(|| crate::vm::realm_id_for_global_value(&crate::locals::current().get(0)))
        .unwrap_or_else(|| crate::vm::current_context_or_default().realm());
    let value = crate::value::Value::Function(std::rc::Rc::new(crate::value::FunctionValue {
        code,
        params,
        captures,
        with_captures: crate::with_scope::capture(),
        properties: function_properties(length),
        private_slots: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        private_environment: crate::private_environment::current(),
        instance_fields: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        kind: metadata.kind,
        strictness: metadata.strictness,
        is_async: metadata.is_async,
        mapped_arguments: metadata.mapped_arguments,
    }));
    if let crate::value::Value::Function(function) = &value {
        crate::cycle_collector::track_function(function);
        if let Some(token) = crate::vm::realm_token(function_realm) {
            function
                .properties
                .borrow_mut()
                .push(("\0realm".to_string(), token));
        }
    }
    value
}

include!("functions_reduction_tail.rs");
