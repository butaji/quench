use std::collections::{HashMap, HashSet};

use oxc::{allocator::Allocator, ast::visit::Visit, parser::Parser, span::SourceType};

use crate::{execute::VmError, facts::ProgramDb, ops::FunctionKind, value::Value};

pub(crate) fn construct(
    arguments: &[Value],
    kind: FunctionKind,
    is_async: bool,
) -> Result<Value, VmError> {
    let source = function_source(arguments, kind, is_async)?;
    reduce_dynamic(&source, kind, is_async)
}

fn invalid_async_generator_parameters(arguments: &[Value]) -> bool {
    arguments
        .get(..arguments.len().saturating_sub(1))
        .is_some_and(|parameters| {
            parameters.iter().any(|value| {
                let source = to_string(value);
                source
                    .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                    .any(|token| matches!(token, "await" | "yield"))
            })
        })
}

pub(crate) fn construct_builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    use crate::ops::Builtin;
    let (kind, is_async) = match builtin {
        Builtin::Function => (FunctionKind::Ordinary, false),
        Builtin::AsyncFunction => (FunctionKind::Ordinary, true),
        Builtin::GeneratorFunction => (FunctionKind::Generator, false),
        Builtin::AsyncGeneratorFunction => (FunctionKind::Generator, true),
        _ => return None,
    };
    Some(construct(arguments, kind, is_async))
}

fn reduce_dynamic(source: &str, kind: FunctionKind, is_async: bool) -> Result<Value, VmError> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(syntax_error("Invalid function source"));
    }
    let Some(oxc::ast::ast::Statement::FunctionDeclaration(function)) = parsed.program.body.first()
    else {
        return Err(syntax_error("Invalid function source"));
    };
    validate_dynamic_parameters(&function.params, kind, is_async)?;
    let body = function
        .body
        .as_ref()
        .ok_or_else(|| syntax_error("Invalid function source"))?;
    let (parameters, count) = crate::functions::function_parameters(function)
        .map_err(|_| syntax_error("Invalid function parameters"))?;
    let strictness = crate::reduce_support::function_strictness(body, false);
    validate_strict_parameters(&function.params, strictness)?;
    validate_strict_body(body, strictness)?;
    let mut facts = ProgramDb {
        strict: matches!(strictness, crate::ops::FunctionStrictness::Strict),
        ..ProgramDb::default()
    };
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
    set_dynamic_name(&value);
    mark_dynamic(&value);
    Ok(value)
}

fn validate_dynamic_parameters(
    parameters: &oxc::ast::ast::FormalParameters<'_>,
    kind: FunctionKind,
    is_async: bool,
) -> Result<(), VmError> {
    struct Validator {
        await_expression: bool,
        yield_expression: bool,
    }
    impl<'a> oxc::ast::visit::Visit<'a> for Validator {
        fn visit_await_expression(&mut self, _: &oxc::ast::ast::AwaitExpression<'a>) {
            self.await_expression = true;
        }
        fn visit_yield_expression(&mut self, _: &oxc::ast::ast::YieldExpression<'a>) {
            self.yield_expression = true;
        }
    }
    let mut validator = Validator {
        await_expression: false,
        yield_expression: false,
    };
    oxc::ast::visit::Visit::visit_formal_parameters(&mut validator, parameters);
    if is_async && validator.await_expression
        || matches!(kind, FunctionKind::Generator) && validator.yield_expression
    {
        return Err(syntax_error("Invalid dynamic function parameters"));
    }
    Ok(())
}

fn validate_strict_parameters(
    parameters: &oxc::ast::ast::FormalParameters<'_>,
    strictness: crate::ops::FunctionStrictness,
) -> Result<(), VmError> {
    if !matches!(strictness, crate::ops::FunctionStrictness::Strict) {
        return Ok(());
    }
    let mut names = std::collections::HashSet::new();
    for parameter in &parameters.items {
        for name in crate::binding_patterns::names(&parameter.pattern) {
            if matches!(name.as_str(), "eval" | "arguments") || !names.insert(name) {
                return Err(syntax_error("Invalid strict function parameters"));
            }
        }
    }
    if let Some(rest) = &parameters.rest {
        for name in crate::binding_patterns::names(&rest.argument) {
            if matches!(name.as_str(), "eval" | "arguments") || !names.insert(name) {
                return Err(syntax_error("Invalid strict function parameters"));
            }
        }
    }
    Ok(())
}

fn validate_strict_body(
    body: &oxc::ast::ast::FunctionBody<'_>,
    strictness: crate::ops::FunctionStrictness,
) -> Result<(), VmError> {
    if !matches!(strictness, crate::ops::FunctionStrictness::Strict) {
        return Ok(());
    }
    struct Validator {
        has_with: bool,
    }
    impl<'a> oxc::ast::visit::Visit<'a> for Validator {
        fn visit_with_statement(&mut self, _: &oxc::ast::ast::WithStatement<'a>) {
            self.has_with = true;
        }
    }
    let mut validator = Validator { has_with: false };
    oxc::ast::visit::Visit::visit_function_body(&mut validator, body);
    if validator.has_with {
        return Err(syntax_error("With statement in strict function"));
    }
    Ok(())
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
    captures.set(0, crate::vm::current_global_object());
    crate::functions::make(
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
        },
    )
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
        .map(crate::conversion::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join(",");
    let body = arguments
        .last()
        .map(crate::conversion::to_string)
        .transpose()?
        .unwrap_or_default();
    let prefix = match (kind, is_async) {
        (FunctionKind::Generator, true) => "async function*",
        (FunctionKind::Generator, false) => "function*",
        (_, true) => "async function",
        (_, false) => "function",
    };
    Ok(format!("{prefix} anonymous({parameters}){{{body}}}"))
}

fn mark_dynamic(value: &Value) {
    if let Value::Function(function) = value {
        function.properties.borrow_mut().extend([
            ("\0dynamic_function".to_string(), Value::Boolean(true)),
            ("name".to_string(), Value::String("anonymous".to_string())),
        ]);
    }
}

fn set_dynamic_name(value: &Value) {
    let Value::Function(function) = value else {
        return;
    };
    let name = Value::String("anonymous".into());
    let descriptor = Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), name.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    let mut properties = function.properties.borrow_mut();
    properties.push(("name".to_string(), name));
    properties.push((crate::builtins::descriptor_key("name"), descriptor));
}

fn invalid(message: &str) -> VmError {
    VmError::EvalError(message.to_string())
}

fn syntax_error(message: &str) -> VmError {
    VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::SyntaxError,
        &[Value::String(message.to_string())],
    ))
}
