use std::collections::HashMap;

use oxc::{allocator::Allocator, parser::Parser, span::SourceType};

use crate::{execute::VmError, facts::ProgramDb, ops::FunctionKind, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let source = function_source(arguments);
    reduce_dynamic(&source)
}

fn reduce_dynamic(source: &str) -> Result<Value, VmError> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(invalid("Invalid function source"));
    }
    let Some(oxc::ast::ast::Statement::FunctionDeclaration(function)) = parsed.program.body.first()
    else {
        return Err(invalid("Invalid function source"));
    };
    let body = function
        .body
        .as_ref()
        .ok_or_else(|| invalid("Invalid function source"))?;
    let (parameters, count) = crate::functions::function_parameters(function)
        .map_err(|_| invalid("Invalid function parameters"))?;
    let strictness = crate::reduce_support::function_strictness(body, false);
    let mut facts = ProgramDb {
        strict: matches!(strictness, crate::ops::FunctionStrictness::Strict),
        ..ProgramDb::default()
    };
    let (ops, _) = crate::functions::reduce_function_ops(
        &body.statements,
        &function.params,
        &mut facts,
        parameters,
        count,
        &HashMap::new(),
        None,
    )
    .ok_or_else(|| invalid("Unsupported function source"))?;
    let value = dynamic_value(&ops, count, strictness);
    mark_dynamic(&value);
    Ok(value)
}

fn dynamic_value(
    ops: &[crate::ops::Op],
    count: u16,
    strictness: crate::ops::FunctionStrictness,
) -> Value {
    crate::functions::make(
        ops,
        count,
        crate::environment::Environment::new(),
        FunctionKind::Ordinary,
        strictness,
        false,
        true,
    )
}

fn function_source(arguments: &[Value]) -> String {
    let body = arguments.last().map_or_else(String::new, to_string);
    let parameters = arguments
        .get(..arguments.len().saturating_sub(1))
        .unwrap_or_default()
        .iter()
        .map(to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("function anonymous({parameters}){{{body}}}")
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
