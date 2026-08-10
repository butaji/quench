use std::collections::HashMap;

use crate::{facts::ProgramDb, ops::Op, value::Value};

pub(crate) fn reduce(
    expression: &oxc::ast::ast::NewExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let callee =
        crate::reduce::reduce_expression(&expression.callee, ops, facts, next_register, locals)?;
    let args = expression
        .arguments
        .iter()
        .map(|argument| {
            crate::reduce::reduce_expression(
                argument.as_expression()?,
                ops,
                facts,
                next_register,
                locals,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Construct { dst, callee, args });
    Some(dst)
}

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), crate::execute::VmError> {
    let Op::Construct { dst, callee, args } = op else {
        return Err(crate::execute::VmError::NotCallable);
    };
    let arguments = args
        .iter()
        .map(|index| crate::execute::read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let target = crate::execute::read_register(registers, *callee)?;
    let value = construct_value(&target, &arguments)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn construct_value(
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match target {
        Value::Builtin(crate::ops::Builtin::Array) => Ok(crate::builtins::array(arguments)),
        Value::Builtin(crate::ops::Builtin::Object) => Ok(crate::builtins::object(arguments)),
        Value::Builtin(crate::ops::Builtin::Number) => construct_number(arguments),
        Value::Builtin(crate::ops::Builtin::Boolean) => construct_boolean(arguments),
        Value::Builtin(crate::ops::Builtin::Promise) => {
            let executor = arguments
                .first()
                .ok_or(crate::execute::VmError::NotCallable)?;
            crate::promise::construct_promise(executor)
        }
        Value::Builtin(crate::ops::Builtin::TypeError) => {
            construct_error(&crate::ops::Builtin::TypeError, arguments)
        }
        Value::Builtin(
            crate::ops::Builtin::Error
            | crate::ops::Builtin::RangeError
            | crate::ops::Builtin::ReferenceError
            | crate::ops::Builtin::SyntaxError
            | crate::ops::Builtin::EvalError
            | crate::ops::Builtin::URIError
            | crate::ops::Builtin::AggregateError,
        ) => construct_error(target_builtin(target), arguments),
        Value::Builtin(crate::ops::Builtin::Date) => {
            crate::date::execute(crate::ops::Builtin::Date, None, arguments)
                .unwrap_or_else(|| Ok(crate::builtins::object(arguments)))
        }
        Value::Builtin(crate::ops::Builtin::RegExp) => Ok(crate::builtins::object(arguments)),
        Value::Builtin(
            crate::ops::Builtin::IntlNumberFormat
            | crate::ops::Builtin::IntlDateTimeFormat
            | crate::ops::Builtin::IntlCollator
            | crate::ops::Builtin::IntlPluralRules
            | crate::ops::Builtin::IntlListFormat
            | crate::ops::Builtin::IntlRelativeTimeFormat
            | crate::ops::Builtin::IntlSegmenter
            | crate::ops::Builtin::IntlDisplayNames
            | crate::ops::Builtin::IntlLocale,
        ) => crate::intl::execute(*target_builtin(target), arguments, None)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        Value::Function(function) => construct_function(function, target, arguments),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn construct_number(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = arguments.first().map_or(0.0, |argument| {
        crate::intl::tolocale::value::to_number(Some(argument))
    });
    Ok(Value::Object(std::rc::Rc::new(vec![(
        "_value".to_string(),
        Value::Number(value),
    )])))
}

fn construct_boolean(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = crate::execute::execute_builtin_with_receiver(
        crate::ops::Builtin::Boolean,
        arguments,
        None,
    )?;
    Ok(Value::Object(std::rc::Rc::new(vec![(
        "_value".to_string(),
        value,
    )])))
}

fn construct_error(
    builtin: &crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    Ok(crate::builtins::error(*builtin, arguments))
}

fn construct_function(
    function: &crate::value::FunctionValue,
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let object = Value::Object(std::rc::Rc::new(vec![(
        "constructor".to_string(),
        target.clone(),
    )]));
    let (result, final_this) = crate::functions::execute_construct(function, &object, arguments)?;
    if matches!(result, Value::Object(_)) {
        Ok(result)
    } else if matches!(final_this, Value::Object(_)) {
        Ok(final_this)
    } else {
        Ok(object)
    }
}

fn target_builtin(target: &Value) -> &crate::ops::Builtin {
    match target {
        Value::Builtin(builtin) => builtin,
        _ => &crate::ops::Builtin::Array,
    }
}
