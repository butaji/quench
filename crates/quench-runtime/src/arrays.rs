use std::collections::HashMap;
use std::rc::Rc;

use oxc::ast::ast::{ArrayExpression, ArrayExpressionElement};

use crate::{facts::ProgramDb, ops::Op, value::Value};

pub(crate) fn execute_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    let result = match builtin {
        crate::ops::Builtin::Array => return Some(Ok(crate::builtins::array(arguments))),
        crate::ops::Builtin::ArrayIsArray => {
            return Some(Ok(crate::builtins::is_array(arguments.first())))
        }
        crate::ops::Builtin::ArrayMap => {
            return Some(crate::builtins::array_map(receiver, arguments))
        }
        crate::ops::Builtin::ArrayFilter => {
            return Some(crate::builtins::array_filter(receiver, arguments));
        }
        crate::ops::Builtin::ArraySome => return Some(some(receiver, arguments)),
        crate::ops::Builtin::ArrayEvery => return Some(every(receiver, arguments)),
        crate::ops::Builtin::ArrayFind => return Some(find(receiver, arguments)),
        crate::ops::Builtin::ArrayIncludes => includes(receiver, arguments),
        crate::ops::Builtin::ArrayIndexOf => index_of(receiver, arguments),
        crate::ops::Builtin::ArrayLastIndexOf => last_index_of(receiver, arguments),
        crate::ops::Builtin::ArraySlice => slice(receiver, arguments),
        crate::ops::Builtin::ArrayConcat => concat(receiver, arguments),
        crate::ops::Builtin::ArrayReduce => return Some(reduce_values(receiver, arguments, false)),
        crate::ops::Builtin::ArrayReduceRight => {
            return Some(reduce_values(receiver, arguments, true));
        }
        crate::ops::Builtin::ArrayForEach => {
            return Some(crate::builtins::array_for_each(receiver, arguments));
        }
        _ => return None,
    };
    Some(Ok(result))
}

pub(crate) fn reduce(
    array: &ArrayExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let mut elements = Vec::new();
    for element in &array.elements {
        let register = match element {
            ArrayExpressionElement::Elision(_) => crate::reduce::emit_undefined(ops, next_register),
            ArrayExpressionElement::SpreadElement(_) => return None,
            _ => crate::reduce::reduce_expression(
                element.as_expression()?,
                ops,
                facts,
                next_register,
                locals,
            )?,
        };
        elements.push(register);
    }
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeArray {
        dst: register,
        elements,
    });
    Some(register)
}

pub(crate) fn property(values: &[Value], key: &str) -> Value {
    if key == "length" {
        return Value::Number(values.len() as f64);
    }
    let method = match key {
        "forEach" => crate::ops::Builtin::ArrayForEach,
        "map" => crate::ops::Builtin::ArrayMap,
        "filter" => crate::ops::Builtin::ArrayFilter,
        "some" => crate::ops::Builtin::ArraySome,
        "every" => crate::ops::Builtin::ArrayEvery,
        "find" => crate::ops::Builtin::ArrayFind,
        "includes" => crate::ops::Builtin::ArrayIncludes,
        "indexOf" => crate::ops::Builtin::ArrayIndexOf,
        "lastIndexOf" => crate::ops::Builtin::ArrayLastIndexOf,
        "slice" => crate::ops::Builtin::ArraySlice,
        "concat" => crate::ops::Builtin::ArrayConcat,
        "reduce" => crate::ops::Builtin::ArrayReduce,
        "reduceRight" => crate::ops::Builtin::ArrayReduceRight,
        _ => return index(values, key),
    };
    Value::Builtin(method)
}

fn index(values: &[Value], key: &str) -> Value {
    key.parse::<usize>()
        .ok()
        .and_then(|index| values.get(index).cloned())
        .unwrap_or(Value::Undefined)
}

pub(crate) fn some(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Boolean(false));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    for (index, value) in values.iter().enumerate() {
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.cloned().unwrap_or(Value::Undefined),
        ];
        let result = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
        if crate::execute::is_truthy(&result) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

pub(crate) fn every(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Boolean(true));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Boolean(true));
    };
    for (index, value) in values.iter().enumerate() {
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.cloned().unwrap_or(Value::Undefined),
        ];
        let result = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
        if !crate::execute::is_truthy(&result) {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

pub(crate) fn find(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    for (index, value) in values.iter().enumerate() {
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.cloned().unwrap_or(Value::Undefined),
        ];
        let result = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
        if crate::execute::is_truthy(&result) {
            return Ok(value.clone());
        }
    }
    Ok(Value::Undefined)
}

pub(crate) fn includes(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::Boolean(false);
    };
    let Some(search) = arguments.first() else {
        return Value::Boolean(false);
    };
    Value::Boolean(values.iter().any(|value| same_value_zero(value, search)))
}

pub(crate) fn index_of(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::Number(-1.0);
    };
    let Some(search) = arguments.first() else {
        return Value::Number(-1.0);
    };
    let index = values.iter().position(|value| strict_equal(value, search));
    Value::Number(index.map_or(-1.0, |value| value as f64))
}

pub(crate) fn last_index_of(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::Number(-1.0);
    };
    let Some(search) = arguments.first() else {
        return Value::Number(-1.0);
    };
    let index = values.iter().rposition(|value| strict_equal(value, search));
    Value::Number(index.map_or(-1.0, |value| value as f64))
}

pub(crate) fn slice(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::Array(Rc::new(Vec::new()));
    };
    let length = values.len() as isize;
    let start = relative_index(arguments.first(), length);
    let end = arguments
        .get(1)
        .map_or(length, |value| end_index(value, length));
    if end <= start {
        return Value::Array(Rc::new(Vec::new()));
    }
    Value::Array(Rc::new(values[start as usize..end as usize].to_vec()))
}

pub(crate) fn concat(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::Array(Rc::new(Vec::new()));
    };
    let mut result = values.as_ref().clone();
    for argument in arguments {
        match argument {
            Value::Array(values) => result.extend(values.iter().cloned()),
            value => result.push(value.clone()),
        }
    }
    Value::Array(Rc::new(result))
}

pub(crate) fn reduce_values(
    receiver: Option<&Value>,
    arguments: &[Value],
    reverse: bool,
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Undefined);
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let indices: Vec<usize> = if reverse {
        (0..values.len()).rev().collect()
    } else {
        (0..values.len()).collect()
    };
    if indices.is_empty() && arguments.get(1).is_none() {
        return Ok(Value::Undefined);
    }
    let (mut accumulator, start) = match arguments.get(1) {
        Some(value) => (value.clone(), 0),
        None => (values[indices.first().copied().unwrap_or(0)].clone(), 1),
    };
    for index in indices.into_iter().skip(start) {
        let args = [
            accumulator,
            values[index].clone(),
            Value::Number(index as f64),
            receiver.cloned().unwrap_or(Value::Undefined),
        ];
        accumulator = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
    }
    Ok(accumulator)
}

fn relative_index(value: Option<&Value>, length: isize) -> isize {
    let number = match value {
        None | Some(Value::Undefined) => 0.0,
        Some(Value::Number(number)) => *number,
        _ => 0.0,
    };
    if number.is_nan() {
        return 0;
    }
    let integer = number.trunc() as isize;
    if integer < 0 {
        (length + integer).max(0)
    } else {
        integer.min(length)
    }
}

fn end_index(value: &Value, length: isize) -> isize {
    if matches!(value, Value::Undefined) {
        length
    } else {
        relative_index(Some(value), length)
    }
}

fn strict_equal(left: &Value, right: &Value) -> bool {
    left == right
}

fn same_value_zero(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => {
            (left.is_nan() && right.is_nan()) || left == right
        }
        _ => left == right,
    }
}
