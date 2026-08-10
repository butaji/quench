use std::collections::HashMap;

use oxc::ast::ast::{ArrayExpression, ArrayExpressionElement};

use crate::{facts::ProgramDb, ops::Op, value::Value};

pub(crate) fn execute_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    use crate::ops::Builtin::*;
    let result = match builtin {
        Array => return Some(Ok(crate::builtins::array(arguments))),
        ArrayIsArray => return Some(Ok(crate::builtins::is_array(arguments.first()))),
        ArrayFrom => return Some(Ok(from(arguments.first()))),
        ArrayMap => return Some(crate::builtins::array_map(receiver, arguments)),
        ArrayFilter => return Some(crate::builtins::array_filter(receiver, arguments)),
        ArraySome => return Some(some(receiver, arguments)),
        ArrayEvery => return Some(every(receiver, arguments)),
        ArrayFind => return Some(find(receiver, arguments)),
        ArrayIncludes => includes(receiver, arguments),
        ArrayIndexOf => index_of(receiver, arguments),
        ArrayLastIndexOf => last_index_of(receiver, arguments),
        ArraySlice => slice(receiver, arguments),
        ArrayConcat => concat(receiver, arguments),
        ArrayFlat => flat(receiver, arguments),
        ArrayFlatMap => return Some(flat_map(receiver, arguments)),
        ArrayAt => return Some(Ok(at(receiver, arguments))),
        ArraySort => return Some(Ok(sort(receiver))),
        ArrayToReversed => return Some(Ok(to_reversed(receiver))),
        ArraySplice => return Some(Ok(splice(receiver, arguments))),
        ArrayReduce => return Some(reduce_values(receiver, arguments, false)),
        ArrayReduceRight => return Some(reduce_values(receiver, arguments, true)),
        ArrayForEach => return Some(crate::builtins::array_for_each(receiver, arguments)),
        ArrayToLocaleString => {
            return Some(crate::intl::tolocale::array_to_locale_string(
                receiver, arguments,
            ))
        }
        _ => return None,
    };
    Some(Ok(result))
}

fn from(value: Option<&Value>) -> Value {
    let values = match value {
        Some(Value::Array(values)) => values.to_vec(),
        Some(Value::Set(data)) => data.values.iter().cloned().collect(),
        Some(Value::Map(data)) => data
            .keys
            .iter()
            .zip(&data.values)
            .map(|(key, value)| Value::array(vec![key.clone(), value.clone()]))
            .collect(),
        Some(Value::Iterator(data)) => data.values.clone(),
        _ => Vec::new(),
    };
    Value::array(values)
}

fn splice(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::array(Vec::new());
    };
    let length = values.len();
    let start = relative_index(arguments.first(), length as isize) as usize;
    let delete_count = arguments.get(1).map_or(length - start, |value| {
        crate::intl::tolocale::value::to_number(Some(value))
            .max(0.0)
            .min((length - start) as f64) as usize
    });
    let mut updated = values.to_vec();
    let removed = updated
        .splice(
            start..start + delete_count,
            arguments.iter().skip(2).cloned(),
        )
        .collect();
    crate::locals::replace_value(receiver, &Value::array(updated));
    Value::array(removed)
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
            ArrayExpressionElement::Elision(_) => {
                crate::reduce_support::emit_undefined(ops, next_register)
            }
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

pub(crate) fn property(values: &crate::value::ArrayData, key: &str) -> Value {
    if let Some(value) = values.property(key) {
        return value;
    }
    if values.is_arguments() && key == "length" {
        return Value::Undefined;
    }
    if key == "length" {
        return Value::Number(values.logical_len() as f64);
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
        "flat" => crate::ops::Builtin::ArrayFlat,
        "flatMap" => crate::ops::Builtin::ArrayFlatMap,
        "at" => crate::ops::Builtin::ArrayAt,
        "sort" => crate::ops::Builtin::ArraySort,
        "toReversed" => crate::ops::Builtin::ArrayToReversed,
        "join" => crate::ops::Builtin::ArrayJoin,
        "push" => crate::ops::Builtin::ArrayPush,
        "splice" => crate::ops::Builtin::ArraySplice,
        "reduce" => crate::ops::Builtin::ArrayReduce,
        "reduceRight" => crate::ops::Builtin::ArrayReduceRight,
        "toLocaleString" => crate::ops::Builtin::ArrayToLocaleString,
        "hasOwnProperty" => crate::ops::Builtin::ObjectHasOwnProperty,
        "propertyIsEnumerable" => crate::ops::Builtin::ObjectPropertyIsEnumerable,
        _ => return index(values, key),
    };
    Value::Builtin(method)
}

fn sort(receiver: Option<&Value>) -> Value {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    let mut sorted = values.to_vec();
    sorted.sort_by_key(|value| crate::intl::tolocale::value::to_string(Some(value)));
    let result = Value::array(sorted);
    crate::locals::replace_value(receiver, &result);
    result
}

fn index(values: &crate::value::ArrayData, key: &str) -> Value {
    key.parse::<usize>()
        .ok()
        .and_then(|index| values.get_index(index))
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
        return Value::array(Vec::new());
    };
    let length = values.len() as isize;
    let start = relative_index(arguments.first(), length);
    let end = arguments
        .get(1)
        .map_or(length, |value| end_index(value, length));
    if end <= start {
        return Value::array(Vec::new());
    }
    Value::array(values[start as usize..end as usize].to_vec())
}

pub(crate) fn concat(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::array(Vec::new());
    };
    let mut result = values.to_vec();
    for argument in arguments {
        match argument {
            Value::Array(values) => result.extend(values.iter().cloned()),
            value => result.push(value.clone()),
        }
    }
    Value::array(result)
}

pub(crate) fn flat(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::array(Vec::new());
    };
    let depth = arguments
        .first()
        .and_then(|value| match value {
            Value::Number(number) => Some(number.max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(1);
    Value::array(flatten(values, depth))
}

fn flatten(values: &[Value], depth: usize) -> Vec<Value> {
    let mut result = Vec::new();
    for value in values {
        if depth > 0 {
            if let Value::Array(nested) = value {
                result.extend(flatten(nested, depth - 1));
                continue;
            }
        }
        result.push(value.clone());
    }
    result
}

pub(crate) fn flat_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::array(Vec::new()));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Array(values.clone()));
    };
    let mut mapped = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.cloned().unwrap_or(Value::Undefined),
        ];
        let result = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
        match result {
            Value::Array(nested) => mapped.extend(nested.iter().cloned()),
            value => mapped.push(value),
        }
    }
    Ok(Value::array(mapped))
}

pub(crate) fn at(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    let Some(Value::Number(number)) = arguments.first() else {
        return Value::Undefined;
    };
    let index = number.trunc() as isize;
    let index = if index < 0 {
        values.len() as isize + index
    } else {
        index
    };
    if index < 0 {
        return Value::Undefined;
    }
    values
        .get(index as usize)
        .cloned()
        .unwrap_or(Value::Undefined)
}

pub(crate) fn to_reversed(receiver: Option<&Value>) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::array(Vec::new());
    };
    Value::array(values.iter().rev().cloned().collect())
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
