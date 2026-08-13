use crate::{
    facts::ProgramDb,
    ops::{ArrayElement, Op},
    value::Value,
};
use oxc::ast::ast::{ArrayExpression, ArrayExpressionElement};
use std::collections::HashMap;
type BuiltinResult = Option<Result<Value, crate::execute::VmError>>;

pub(crate) fn execute_builtin(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> BuiltinResult {
    if let Some(result) = map_argument_error(builtin, receiver, arguments) {
        return Some(result);
    }
    if let Some(result) = revoked_receiver_error(builtin, receiver) {
        return Some(result);
    }
    if let Some(result) = array_iterator_builtin(builtin, receiver) {
        return Some(result);
    }
    if let Some(result) = array_mutation_builtin(builtin, receiver, arguments) {
        return Some(result);
    }
    use crate::ops::Builtin::*;
    let result = match builtin {
        Array => return Some(Ok(crate::builtins::array(arguments))),
        ArrayIsArray => return Some(Ok(crate::builtins::is_array(arguments.first()))),
        ArrayFrom => return Some(from(receiver, arguments)),
        ArrayMap => return Some(crate::builtins::array_map(receiver, arguments)),
        ArrayFilter => return Some(crate::builtins::array_filter(receiver, arguments)),
        ArraySome => return Some(some(receiver, arguments)),
        ArrayEvery => return Some(every(receiver, arguments)),
        ArrayFind => return Some(find(receiver, arguments)),
        ArrayIncludes => includes(receiver, arguments),
        ArrayIndexOf => index_of(receiver, arguments),
        ArrayLastIndexOf => last_index_of(receiver, arguments),
        ArraySlice => slice(receiver, arguments),
        ArrayConcat => return Some(concat(receiver, arguments)),
        ArrayFlat => flat(receiver, arguments),
        ArrayFlatMap => return Some(flat_map(receiver, arguments)),
        ArrayAt => return Some(Ok(at(receiver, arguments))),
        ArraySort => return Some(Ok(sort(receiver))),
        ArrayToReversed => return Some(Ok(to_reversed(receiver))),
        ArraySplice => return Some(Ok(splice(receiver, arguments))),
        ArrayReduce => return Some(reduce_values(receiver, arguments, false)),
        ArrayReduceRight => return Some(reduce_values(receiver, arguments, true)),
        ArrayForEach => return Some(crate::builtins::array_for_each(receiver, arguments)),
        ArrayToLocaleString => return Some(array_to_locale_string(receiver, arguments)),
        _ => return None,
    };
    Some(Ok(result))
}

fn map_argument_error(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, crate::execute::VmError>> {
    if builtin != crate::ops::Builtin::ArrayMap {
        return None;
    }
    if receiver.is_none_or(|value| matches!(value, Value::Null | Value::Undefined)) {
        return Some(Err(crate::value::error::throw_type_error(
            "Array.prototype.map called on null or undefined",
        )));
    }
    if !arguments
        .first()
        .is_some_and(crate::conversion::is_callable)
    {
        return Some(Err(crate::vm::not_callable()));
    }
    None
}

fn revoked_receiver_error(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
) -> Option<Result<Value, crate::execute::VmError>> {
    let Some(Value::Proxy(proxy)) = receiver else {
        return None;
    };
    let array_method = matches!(
        builtin,
        crate::ops::Builtin::ArrayConcat
            | crate::ops::Builtin::ArrayFilter
            | crate::ops::Builtin::ArrayMap
            | crate::ops::Builtin::ArraySlice
            | crate::ops::Builtin::ArraySplice
    );
    (array_method && crate::proxy::is_revoked(proxy)).then(|| {
        Err(crate::value::error::throw_type_error(
            "Cannot perform operation on a revoked proxy",
        ))
    })
}

fn array_to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    crate::intl::tolocale::array_to_locale_string(receiver, arguments)
}

fn array_iterator_builtin(builtin: crate::ops::Builtin, receiver: Option<&Value>) -> BuiltinResult {
    match builtin {
        crate::ops::Builtin::ArrayIterator => Some(array_iterator(receiver)),
        crate::ops::Builtin::ArrayKeys => Some(array_keys(receiver)),
        crate::ops::Builtin::ArrayEntries => Some(array_entries(receiver)),
        _ => None,
    }
}

include!("arrays_mutation.rs");
include!("arrays_typed_static.rs");
include!("arrays_from.rs");

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
        elements.push(reduce_element(element, ops, facts, next_register, locals)?);
    }
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    emit_array(ops, register, elements);
    Some(register)
}
fn reduce_element(
    element: &ArrayExpressionElement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<ArrayElement> {
    match element {
        ArrayExpressionElement::Elision(_) => Some(ArrayElement::Elision),
        ArrayExpressionElement::SpreadElement(spread) => {
            crate::reduce::reduce_expression(&spread.argument, ops, facts, next, locals)
                .map(ArrayElement::Spread)
        }
        _ => crate::reduce::reduce_expression(element.as_expression()?, ops, facts, next, locals)
            .map(ArrayElement::Value),
    }
}
fn emit_array(ops: &mut Vec<Op>, dst: u16, elements: Vec<ArrayElement>) {
    let dense = elements
        .iter()
        .map(|element| match element {
            ArrayElement::Value(register) => Some(*register),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    match dense {
        Some(elements) => ops.push(Op::MakeArray { dst, elements }),
        None => ops.push(Op::BuildArray { dst, elements }),
    }
}

pub(crate) fn property(values: &crate::value::ArrayData, key: &str) -> Value {
    if let Some(value) = direct_property(values, key) {
        return value;
    }
    if let Some(value) = own_index(values, key) {
        return value;
    }
    if let Some(value) = array_prototype_override(key) {
        return value;
    }
    if iterator_symbol_removed(key) {
        return Value::Undefined;
    }
    let Some(method) = array_method(key) else {
        return index(values, key);
    };
    Value::Builtin(method)
}

fn array_method(key: &str) -> Option<crate::ops::Builtin> {
    array_search_method(key).or_else(|| array_method_core(key))
}

fn array_method_core(key: &str) -> Option<crate::ops::Builtin> {
    Some(match key {
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
        "join" | "toString" => crate::ops::Builtin::ArrayJoin,
        "push" => crate::ops::Builtin::ArrayPush,
        "shift" => crate::ops::Builtin::ArrayShift,
        "reverse" => crate::ops::Builtin::ArrayReverse,
        "pop" => crate::ops::Builtin::ArrayPop,
        "unshift" => crate::ops::Builtin::ArrayUnshift,
        "fill" => crate::ops::Builtin::ArrayFill,
        "copyWithin" => crate::ops::Builtin::ArrayCopyWithin,
        "toSorted" => crate::ops::Builtin::ArrayToSorted,
        "splice" => crate::ops::Builtin::ArraySplice,
        "reduce" => crate::ops::Builtin::ArrayReduce,
        "reduceRight" => crate::ops::Builtin::ArrayReduceRight,
        "toLocaleString" => crate::ops::Builtin::ArrayToLocaleString,
        "values" => crate::ops::Builtin::ArrayIterator,
        "keys" => crate::ops::Builtin::ArrayKeys,
        "entries" => crate::ops::Builtin::ArrayEntries,
        "Symbol.iterator" => crate::ops::Builtin::ArrayIterator,
        "hasOwnProperty" => crate::ops::Builtin::ObjectHasOwnProperty,
        "propertyIsEnumerable" => crate::ops::Builtin::ObjectPropertyIsEnumerable,
        _ => return None,
    })
}

include!("arrays_search_methods.rs");

fn own_index(values: &crate::value::ArrayData, key: &str) -> Option<Value> {
    array_index(key).and_then(|index| values.get_index(index as usize))
}

fn array_prototype_override(key: &str) -> Option<Value> {
    let descriptor =
        crate::builtins::read_intrinsic_override(crate::ops::Builtin::ArrayPrototype, key)?;
    let Value::Object(properties) = descriptor else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "value").then(|| value.clone()))
}

pub(crate) fn array_index(key: &str) -> Option<u32> {
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

fn direct_property(values: &crate::value::ArrayData, key: &str) -> Option<Value> {
    if let Some(value) = values.property(key) {
        return Some(value);
    }
    if values.is_arguments() && key == "length" {
        return Some(Value::Undefined);
    }
    if values.is_arguments() && key == "constructor" {
        return Some(Value::Builtin(crate::ops::Builtin::Object));
    }
    if key == "constructor" {
        return Some(Value::Builtin(crate::ops::Builtin::Array));
    }
    (key == "length").then(|| Value::Number(values.logical_len() as f64))
}

fn iterator_symbol_removed(key: &str) -> bool {
    key == "Symbol.iterator"
        && crate::builtins::builtin_prototype_property_is_removed(
            crate::ops::Builtin::ArrayPrototype,
            "Symbol.iterator",
        )
}

include!("arrays_iterator.rs");

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
    array_index(key)
        .and_then(|index| values.get_index(index as usize))
        .unwrap_or(Value::Undefined)
}

include!("arrays_iteration.rs");
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
    let length = values.logical_len() as isize;
    let start = array_search_start(arguments.get(1), length);
    if start >= length {
        return Value::Number(-1.0);
    }
    let index = (start..length).find(|index| {
        values
            .get_index(*index as usize)
            .is_some_and(|value| strict_equal(&value, search))
    });
    Value::Number(index.map_or(-1.0, |value| value as f64))
}

fn array_search_start(value: Option<&Value>, length: isize) -> isize {
    let Some(value) = value else { return 0 };
    let number = crate::conversion::to_number(value).unwrap_or(0.0);
    if number.is_nan() {
        return 0;
    }
    if number.is_infinite() {
        return if number.is_sign_negative() { 0 } else { length };
    }
    let integer = number.trunc() as isize;
    if integer < 0 {
        (length + integer).max(0)
    } else {
        integer
    }
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
    crate::equality::strict_equal(left, right)
}
fn same_value_zero(left: &Value, right: &Value) -> bool {
    crate::builtins::same_value_zero(left, right)
}

#[cfg(test)]
mod tests {
    use super::index_of;
    use crate::value::{ArrayData, ObjectData, Value};
    use std::rc::Rc;

    #[test]
    fn index_of_does_not_use_structural_object_equality() {
        let left = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let right = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let array = Value::Array(Rc::new(ArrayData::new(vec![left.clone()])));
        let result = index_of(Some(&array), &[right]);
        assert_eq!(result, Value::Number(-1.0));
    }
}

include!("arrays_concat.rs");
