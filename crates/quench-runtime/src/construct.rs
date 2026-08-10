use std::{collections::HashMap, rc::Rc};

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
        Value::Builtin(builtin) => construct_builtin(*builtin, arguments),
        Value::Function(function) => construct_function(function, target, arguments),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn construct_builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if is_error_builtin(builtin) {
        return construct_error(&builtin, arguments);
    }
    match builtin {
        crate::ops::Builtin::Array => Ok(crate::builtins::array(arguments)),
        crate::ops::Builtin::ArrayBuffer => construct_array_buffer(arguments),
        crate::ops::Builtin::Float64Array => construct_float64_array(arguments),
        crate::ops::Builtin::Float32Array => construct_float32_array(arguments),
        crate::ops::Builtin::Int8Array => construct_int8_array(arguments),
        crate::ops::Builtin::Int16Array => construct_int16_array(arguments),
        crate::ops::Builtin::Int32Array => construct_int32_array(arguments),
        crate::ops::Builtin::Uint8Array => construct_uint8_array(arguments),
        crate::ops::Builtin::Uint16Array => construct_uint16_array(arguments),
        crate::ops::Builtin::Uint32Array => construct_uint32_array(arguments),
        crate::ops::Builtin::Uint8ClampedArray => construct_uint8_clamped_array(arguments),
        crate::ops::Builtin::DataView => construct_data_view(arguments),
        crate::ops::Builtin::Object => Ok(crate::builtins::object(arguments)),
        crate::ops::Builtin::Number => construct_number(arguments),
        crate::ops::Builtin::Boolean => construct_boolean(arguments),
        crate::ops::Builtin::Promise => construct_promise(arguments),
        crate::ops::Builtin::Date => crate::date::execute(builtin, None, arguments)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        crate::ops::Builtin::RegExp => Ok(crate::builtins::object(arguments)),
        crate::ops::Builtin::IntlNumberFormat
        | crate::ops::Builtin::IntlDateTimeFormat
        | crate::ops::Builtin::IntlCollator
        | crate::ops::Builtin::IntlPluralRules
        | crate::ops::Builtin::IntlListFormat
        | crate::ops::Builtin::IntlRelativeTimeFormat
        | crate::ops::Builtin::IntlSegmenter
        | crate::ops::Builtin::IntlDisplayNames
        | crate::ops::Builtin::IntlLocale => crate::intl::execute(builtin, arguments, None)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn is_error_builtin(builtin: crate::ops::Builtin) -> bool {
    matches!(
        builtin,
        crate::ops::Builtin::TypeError
            | crate::ops::Builtin::Error
            | crate::ops::Builtin::RangeError
            | crate::ops::Builtin::ReferenceError
            | crate::ops::Builtin::SyntaxError
            | crate::ops::Builtin::EvalError
            | crate::ops::Builtin::URIError
            | crate::ops::Builtin::AggregateError
    )
}

fn construct_promise(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let executor = arguments
        .first()
        .ok_or(crate::execute::VmError::NotCallable)?;
    crate::promise::construct_promise(executor)
}

fn construct_array_buffer(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let length = arguments.first().map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let length = to_index(length)?;
    Ok(Value::ArrayBuffer(Rc::new(
        crate::value::ArrayBufferData::new(length),
    )))
}

fn construct_data_view(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(Value::ArrayBuffer(buffer)) = arguments.first() else {
        return Err(type_error("DataView buffer must be an ArrayBuffer"));
    };
    if *buffer.detached.borrow() {
        return Err(type_error("Cannot use a detached ArrayBuffer"));
    }
    let offset = arguments.get(1).map_or(0.0, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
    });
    let offset = to_index(offset)?;
    let buffer_length = buffer.byte_length();
    if offset > buffer_length {
        return Err(range_error("Invalid DataView byte offset"));
    }
    let available = buffer_length - offset;
    let length = match arguments.get(2) {
        Some(value) => to_index(crate::intl::tolocale::value::to_number(Some(value)))?,
        None => available,
    };
    if length > available {
        return Err(range_error("Invalid DataView byte length"));
    }
    Ok(Value::DataView(Rc::new(crate::value::DataViewData::new(
        buffer.clone(),
        offset,
        length,
    ))))
}

include!("construct_typed_low.rs");
include!("construct_typed_high.rs");

fn range_error(message: &str) -> crate::execute::VmError {
    crate::execute::VmError::Thrown(crate::builtins::error(
        crate::ops::Builtin::RangeError,
        &[Value::String(message.to_string())],
    ))
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
    if !crate::functions::is_constructible(function) {
        return Err(crate::execute::VmError::NotCallable);
    }
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
