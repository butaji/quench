use std::collections::HashMap;

use oxc::{allocator::Allocator, parser::Parser, span::SourceType};

use crate::{execute::VmError, facts::ProgramDb, ops::FunctionKind, value::Value};

pub(crate) fn construct(
    arguments: &[Value],
    kind: FunctionKind,
    is_async: bool,
) -> Result<Value, VmError> {
    let source = function_source(arguments, kind, is_async);
    reduce_dynamic(&source, kind, is_async)
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
    let (parameters, count) = crate::functions::function_parameters(function)
        .map_err(|_| syntax_error("Invalid function parameters"))?;
    let strictness = crate::reduce_support::function_strictness(body, false);
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
    let value = dynamic_value(&ops, count, strictness, kind, is_async);
    mark_dynamic(&value);
    Ok(value)
}

fn dynamic_value(
    ops: &[crate::ops::Op],
    count: u16,
    strictness: crate::ops::FunctionStrictness,
    kind: FunctionKind,
    is_async: bool,
) -> Value {
    let captures = crate::environment::Environment::new();
    captures.set(0, crate::vm::current_global_object());
    crate::functions::make(ops, count, captures, kind, strictness, is_async, true)
}

fn function_source(arguments: &[Value], kind: FunctionKind, is_async: bool) -> String {
    let body = arguments.last().map_or_else(String::new, to_string);
    let parameters = arguments
        .get(..arguments.len().saturating_sub(1))
        .unwrap_or_default()
        .iter()
        .map(to_string)
        .collect::<Vec<_>>()
        .join(",");
    let prefix = match (kind, is_async) {
        (FunctionKind::Generator, true) => "async function*",
        (FunctionKind::Generator, false) => "function*",
        (_, true) => "async function",
        (_, false) => "function",
    };
    format!("{prefix} anonymous({parameters}){{{body}}}")
}

fn to_string(value: &Value) -> String {
    crate::intl::tolocale::value::to_string(Some(value))
}

fn mark_dynamic(value: &Value) {
    if let Value::Function(function) = value {
        function
            .properties
            .borrow_mut()
            .push(("\0dynamic_function".to_string(), Value::Boolean(true)));
    }
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
