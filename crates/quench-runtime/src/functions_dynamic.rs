use std::collections::{HashMap, HashSet};

use oxc::{
    allocator::Allocator,
    ast::{
        ast::{Class, PrivateFieldExpression, PrivateInExpression},
        visit::{walk, Visit},
    },
    parser::Parser,
    span::SourceType,
};

use crate::{
    conversion::to_string,
    execute::VmError,
    facts::ProgramDb,
    ops::{FunctionKind, FunctionStrictness},
    value::Value,
};

fn construct(
    arguments: &[Value],
    kind: FunctionKind,
    is_async: bool,
    realm: Option<crate::ops::RealmId>,
) -> Result<Value, VmError> {
    let source = function_source(arguments, kind, is_async)?;
    let realm = realm.unwrap_or_else(|| crate::vm::current_context_or_default().realm());
    crate::vm::with_realm(realm, || reduce_dynamic(&source, kind, is_async))
        .unwrap_or_else(|| reduce_dynamic(&source, kind, is_async))
}

pub(crate) fn construct_builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    construct_builtin_in_realm(builtin, arguments, None)
}

pub(crate) fn construct_builtin_in_realm(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    realm: Option<crate::ops::RealmId>,
) -> Option<Result<Value, VmError>> {
    use crate::ops::Builtin;
    let (kind, is_async) = match builtin {
        Builtin::Function => (FunctionKind::Ordinary, false),
        Builtin::AsyncFunction => (FunctionKind::Ordinary, true),
        Builtin::GeneratorFunction => (FunctionKind::Generator, false),
        Builtin::AsyncGeneratorFunction => (FunctionKind::Generator, true),
        _ => return None,
    };
    Some(construct(arguments, kind, is_async, realm))
}

fn reduce_dynamic(source: &str, kind: FunctionKind, is_async: bool) -> Result<Value, VmError> {
    if source.contains("import.meta") {
        return Err(syntax_error("import.meta is only valid in modules"));
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(syntax_error("Invalid function source"));
    }
    let Some(oxc::ast::ast::Statement::FunctionDeclaration(function)) = parsed.program.body.first()
    else {
        return Err(syntax_error("Invalid function source"));
    };
    let body = function
        .body
        .as_ref()
        .ok_or_else(|| syntax_error("Invalid function source"))?;
    if is_async
        && matches!(kind, FunctionKind::Generator)
        && forbidden_parameter_expression(&function.params)
    {
        return Err(syntax_error("Invalid async generator parameters"));
    }
    if matches!(kind, FunctionKind::Generator) && has_yield_expression(&function.params) {
        return Err(syntax_error("Invalid generator parameters"));
    }
    let strictness = crate::reduce_support::function_strictness(body, false);
    if matches!(strictness, crate::ops::FunctionStrictness::Strict)
        && has_duplicate_parameter_names(&function.params)
    {
        return Err(syntax_error("Duplicate function parameter name"));
    }
    if matches!(strictness, crate::ops::FunctionStrictness::Strict)
        && has_strict_parameter_name(&function.params)
    {
        return Err(syntax_error("Invalid strict function parameter"));
    }
    let (parameters, count) = crate::functions::function_parameters(function)
        .map_err(|_| syntax_error("Invalid function parameters"))?;
    if matches!(strictness, crate::ops::FunctionStrictness::Strict) && contains_with_statement(body)
    {
        return Err(syntax_error("Invalid strict function body"));
    }
    if has_invalid_private_identifier(body) {
        return Err(syntax_error("Invalid private identifier"));
    }
    let analysis = match crate::semantic::analyze(&parsed.program) {
        Ok(analysis) => analysis,
        Err(errors) => {
            // Non-strict dynamic Function constructor: ES5 allows duplicate
            // parameter names and the `eval`/`arguments` parameter names.
            // The OXC binder always raises a SyntaxError for them, so skip
            // the analyzer and use an empty Analysis when the only failures
            // are these historically-permitted parameters.
            if matches!(strictness, FunctionStrictness::Sloppy)
                && errors
                    .iter()
                    .all(|e| is_duplicate_param_error_str(e) || is_strict_only_param_error_str(e))
            {
                crate::semantic::Analysis {
                    scope_count: 0,
                    symbol_count: 0,
                    private_names: Vec::new(),
                    fact_sites: std::collections::HashMap::new(),
                }
            } else {
                return Err(syntax_error("Invalid function source"));
            }
        }
    };
    let mut facts = ProgramDb {
        strict: matches!(strictness, crate::ops::FunctionStrictness::Strict),
        private_names: analysis.private_names.into_iter().collect(),
        ..ProgramDb::default()
    };
    facts.install_fact_sites(analysis.fact_sites);
    let global = HashMap::from([("globalThis".to_string(), 0)]);
    let inherited = facts.in_function;
    facts.in_function = true;
    let reduced = crate::functions::reduce_function_ops(
        &body.statements,
        &function.params,
        &mut facts,
        parameters,
        count,
        &global,
        None,
    );
    facts.in_function = inherited;
    let (ops, _) = reduced.ok_or_else(|| invalid("Unsupported function source"))?;
    let length = crate::function_parameters::expected_argument_count(&function.params);
    let value = dynamic_value(ops, count, length, strictness, kind, is_async);
    crate::builtins::set_function_name(&value, "anonymous")?;
    mark_dynamic(&value, source);
    Ok(value)
}

fn has_duplicate_parameter_names(parameters: &oxc::ast::ast::FormalParameters<'_>) -> bool {
    let mut seen = HashSet::new();
    let names = parameters
        .items
        .iter()
        .flat_map(|item| crate::binding_patterns::names(&item.pattern))
        .chain(
            parameters
                .rest
                .iter()
                .flat_map(|rest| crate::binding_patterns::names(&rest.argument)),
        );
    names.into_iter().any(|name| !seen.insert(name))
}

fn has_strict_parameter_name(parameters: &oxc::ast::ast::FormalParameters<'_>) -> bool {
    parameters
        .items
        .iter()
        .flat_map(|item| crate::binding_patterns::names(&item.pattern))
        .chain(
            parameters
                .rest
                .iter()
                .flat_map(|rest| crate::binding_patterns::names(&rest.argument)),
        )
        .any(|name| matches!(name.as_str(), "eval" | "arguments"))
}

fn contains_with_statement(body: &oxc::ast::ast::FunctionBody<'_>) -> bool {
    struct Validator {
        found: bool,
    }

    impl<'a> Visit<'a> for Validator {
        fn visit_with_statement(&mut self, _: &oxc::ast::ast::WithStatement<'a>) {
            self.found = true;
        }
    }

    let mut validator = Validator { found: false };
    validator.visit_function_body(body);
    validator.found
}

fn has_invalid_private_identifier(body: &oxc::ast::ast::FunctionBody<'_>) -> bool {
    struct Validator {
        class_depth: u16,
        invalid: bool,
    }

    impl<'a> Visit<'a> for Validator {
        fn visit_class(&mut self, class: &Class<'a>) {
            self.class_depth = self.class_depth.saturating_add(1);
            walk::walk_class(self, class);
            self.class_depth = self.class_depth.saturating_sub(1);
        }

        fn visit_private_field_expression(&mut self, expression: &PrivateFieldExpression<'a>) {
            self.invalid |= self.class_depth == 0;
            walk::walk_private_field_expression(self, expression);
        }

        fn visit_private_in_expression(&mut self, expression: &PrivateInExpression<'a>) {
            self.invalid |= self.class_depth == 0;
            walk::walk_private_in_expression(self, expression);
        }
    }

    let mut validator = Validator {
        class_depth: 0,
        invalid: false,
    };
    validator.visit_function_body(body);
    validator.invalid
}

fn forbidden_parameter_expression(parameters: &oxc::ast::ast::FormalParameters<'_>) -> bool {
    struct Validator {
        forbidden: bool,
    }

    impl<'a> Visit<'a> for Validator {
        fn visit_await_expression(&mut self, _: &oxc::ast::ast::AwaitExpression<'a>) {
            self.forbidden = true;
        }

        fn visit_yield_expression(&mut self, _: &oxc::ast::ast::YieldExpression<'a>) {
            self.forbidden = true;
        }
    }

    let mut validator = Validator { forbidden: false };
    validator.visit_formal_parameters(parameters);
    validator.forbidden
}

fn has_yield_expression(parameters: &oxc::ast::ast::FormalParameters<'_>) -> bool {
    struct Validator {
        found: bool,
    }

    impl<'a> Visit<'a> for Validator {
        fn visit_identifier_reference(
            &mut self,
            identifier: &oxc::ast::ast::IdentifierReference<'a>,
        ) {
            if identifier.name == "yield" {
                self.found = true;
            }
        }

        fn visit_yield_expression(&mut self, _: &oxc::ast::ast::YieldExpression<'a>) {
            self.found = true;
        }
    }

    let mut validator = Validator { found: false };
    validator.visit_formal_parameters(parameters);
    validator.found
}

fn dynamic_value(
    ops: Vec<crate::ops::Op>,
    count: u16,
    length: u16,
    strictness: crate::ops::FunctionStrictness,
    kind: FunctionKind,
    is_async: bool,
) -> Value {
    let captures = crate::environment::Environment::new();
    let realm = crate::vm::current_context_or_default().realm();
    let global =
        crate::vm::realm_global_value(realm).unwrap_or_else(|| crate::locals::current().get(0));
    captures.set(0, global);
    let value = crate::functions::make(
        crate::machine::FunctionCode::from_ops(ops),
        count,
        length,
        captures,
        crate::functions::FunctionMetadata {
            kind,
            length,
            strictness,
            is_async,
            mapped_arguments: true,
            direct_constructor: std::rc::Rc::default(),
            composed_constructor: std::rc::Rc::default(),
        },
    );
    if let Value::Function(function) = &value {
        let mut properties = function.properties.borrow_mut();
        let prototype = match (kind, is_async) {
            (FunctionKind::Generator, true) => crate::ops::Builtin::AsyncGeneratorFunctionPrototype,
            (FunctionKind::Generator, false) => crate::ops::Builtin::GeneratorFunctionPrototype,
            (_, true) => crate::ops::Builtin::AsyncFunctionPrototype,
            (_, false) => crate::ops::Builtin::FunctionPrototype,
        };
        properties.push((
            "\0function_prototype".to_string(),
            crate::vm::realm_intrinsic(prototype),
        ));
    }
    value
}

fn function_source(
    arguments: &[Value],
    kind: FunctionKind,
    is_async: bool,
) -> Result<String, VmError> {
    let parameters = arguments
        .get(..arguments.len().saturating_sub(1))
        .unwrap_or_default()
        .iter()
        .map(to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join(",");
    let parameters = normalize_annex_b_comments(&parameters, false);
    // The Function constructor joins parameter strings with commas. A
    // trailing line comment would otherwise consume the generated closing
    // parenthesis; ECMAScript inserts a line terminator before it.
    let parameter_terminator = parameters
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.contains("//"));
    let parameters = if parameter_terminator {
        format!("{parameters}\n")
    } else {
        parameters
    };
    let body = arguments
        .last()
        .map_or_else(|| Ok(String::new()), to_string)?;
    let body = normalize_annex_b_comments(&body, true);
    let prefix = match (kind, is_async) {
        (FunctionKind::Generator, true) => "async function*",
        (FunctionKind::Generator, false) => "function*",
        (_, true) => "async function",
        (_, false) => "function",
    };
    Ok(format!("{prefix} anonymous({parameters}) {{\n{body}\n}}"))
}

fn normalize_annex_b_comments(source: &str, allow_leading_close: bool) -> String {
    let source = source.strip_prefix("<!--").unwrap_or(source);
    source
        .lines()
        .enumerate()
        .map(|(line_number, line)| {
            let trimmed = line.trim_start();
            if (allow_leading_close || line_number > 0) && trimmed.starts_with("-->") {
                &line[..line.len() - trimmed.len()]
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn mark_dynamic(value: &Value, source: &str) {
    if let Value::Function(function) = value {
        let mut properties = function.properties.borrow_mut();
        properties.push(("\0dynamic_function".to_string(), Value::Boolean(true)));
        properties.push((
            "\0dynamic_source".to_string(),
            Value::String(source.to_string()),
        ));
    }
}

fn invalid(message: &str) -> VmError {
    VmError::EvalError(message.to_string())
}

#[cold]
#[inline(never)]
fn syntax_error(message: &str) -> VmError {
    crate::value::error::throw_syntax_error(message)
}

fn is_duplicate_param_error_str(message: &str) -> bool {
    message.contains("has already been declared")
}

fn is_strict_only_param_error_str(message: &str) -> bool {
    // OXC's analyzer unconditionally rejects `eval`/`arguments` as parameter
    // names; filter those for non-strict dynamic functions.
    message.contains("'eval'") || message.contains("'arguments'")
}
