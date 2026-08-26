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
    execute_builtin_match(builtin, receiver, arguments)
}

fn execute_builtin_match(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> BuiltinResult {
    use crate::ops::Builtin::*;
    let result = match builtin {
        Array => return Some(crate::builtins::array(arguments)),
        ArrayIsArray => return Some(crate::builtins::is_array(arguments.first())),
        ArrayFrom => return Some(from(receiver, arguments)),
        ArrayOf => {
            return Some(create_result(receiver, arguments.to_vec(), false));
        }
        ArrayMap => return Some(crate::builtins::array_map(receiver, arguments)),
        ArrayFilter => return Some(crate::builtins::array_filter(receiver, arguments)),
        ArraySome => return Some(some(receiver, arguments)),
        ArrayEvery => return Some(every(receiver, arguments)),
        ArrayFind => return Some(find(receiver, arguments)),
        ArrayIncludes => return Some(includes(receiver, arguments)),
        ArrayIndexOf => return Some(index_of(receiver, arguments)),
        ArrayLastIndexOf => return Some(last_index_of(receiver, arguments)),
        ArraySlice => return Some(slice(receiver, arguments)),
        ArrayConcat => return Some(concat(receiver, arguments)),
        ArrayFlat => return Some(flat(receiver, arguments)),
        ArrayFlatMap => return Some(flat_map(receiver, arguments)),
        ArrayAt => return Some(at(receiver, arguments)),
        ArraySort => return Some(sort(receiver, arguments)),
        ArrayToReversed => return Some(to_reversed(receiver)),
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
    if receiver.map_or(true, |value| {
        matches!(value, Value::Null | Value::Undefined)
    }) {
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
        crate::ops::Builtin::TypedArrayIterator => Some(typed_array_iterator(receiver)),
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
    let start = start.min(updated.len());
    let end = start.saturating_add(delete_count).min(updated.len());
    let removed = updated
        .splice(
            start..end,
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
        _ => return array_method_tail(key),
    })
}

fn array_method_tail(key: &str) -> Option<crate::ops::Builtin> {
    Some(match key {
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
    let index = array_index(key)? as usize;
    // Keep the dense backing-store read independent from generic properties.
    // `direct_property` has already handled indexed property overrides; this
    // path only reads an actual dense slot.
    values.dense_value_at(index)
}

fn array_prototype_override(key: &str) -> Option<Value> {
    let descriptor =
        crate::builtins::read_intrinsic_override(crate::ops::Builtin::ArrayPrototype, key)?;
    let Value::Object(properties) = descriptor else {
        return None;
    };
    let result = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "value").then(|| value.clone()));
    result
}

/// The `get` accessor of a user-defined `Array.prototype` override, e.g.
/// `Object.defineProperty(Array.prototype, "2", { get: ... })`.
pub(crate) fn prototype_override_getter(key: &str) -> Option<Value> {
    let descriptor =
        crate::builtins::read_intrinsic_override(crate::ops::Builtin::ArrayPrototype, key)?;
    let Value::Object(properties) = descriptor else {
        return None;
    };
    let result = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "get").then(|| value.clone()));
    result
}

pub(crate) fn prototype_override_setter(key: &str) -> Option<Value> {
    let descriptor =
        crate::builtins::read_intrinsic_override(crate::ops::Builtin::ArrayPrototype, key)?;
    let Value::Object(properties) = descriptor else {
        return None;
    };
    let result = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "set").then(|| value.clone()));
    result
}

pub(crate) fn prototype_override_present(key: &str) -> bool {
    crate::builtins::read_intrinsic_override(crate::ops::Builtin::ArrayPrototype, key).is_some()
}

pub(crate) fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.as_bytes()[0] == b'0') {
        return None;
    }
    let mut index = 0u32;
    for byte in key.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        index = index.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
    }
    (index != u32::MAX).then_some(index)
}

fn direct_property(values: &crate::value::ArrayData, key: &str) -> Option<Value> {
    // arguments.length may carry a plain value override stored on the
    // live argument data; consult it before falling back to the
    // ordinary own-property / array-length paths.
    if values.is_arguments() && key == "length" {
        return Some(values.arguments_length_value());
    }
    if let Some(value) = values.property(key) {
        return Some(value);
    }
    if values.is_arguments() && key == "length" {
        return Some(values.arguments_length_value());
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

fn sort(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver @ Value::Array(values)) = receiver else {
        return Ok(Value::Undefined);
    };
    let mut sorted = values.to_vec();
    if let Some(compare) = arguments.first().filter(|value| !matches!(value, Value::Undefined)) {
        if !crate::conversion::is_callable(compare) {
            return Err(crate::value::error::throw_type_error(
                "The comparison function must be either a function or undefined",
            ));
        }
        sorted.sort_by(|left, right| {
            let result = crate::execute::call(compare, &Value::Undefined, &[left.clone(), right.clone()])
                .ok()
                .and_then(|value| crate::conversion::to_number(&value).ok())
                .unwrap_or(0.0);
            result.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        sorted.sort_by_key(|value| crate::intl::tolocale::value::to_string(Some(value)));
    }
    let result = Value::array(sorted);
    crate::locals::replace_value(receiver, &result);
    Ok(result)
}

fn index(values: &crate::value::ArrayData, key: &str) -> Value {
    array_index(key)
        .and_then(|index| values.get_index(index as usize))
        .unwrap_or(Value::Undefined)
}

include!("arrays_iteration.rs");
pub(crate) fn flat(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined))
    else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.flat called on null or undefined",
        ));
    };
    let receiver = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&receiver)?;
    let depth = flat_depth(arguments.first())?;
    let mut values = Vec::with_capacity(length);
    for index in 0..length {
        if let Some(value) = crate::builtins::map_value(&receiver, index)? {
            values.push(value);
        }
    }
    Ok(Value::array(flatten(&values, depth)))
}

fn flat_depth(value: Option<&Value>) -> Result<usize, crate::execute::VmError> {
    let Some(value) = value else { return Ok(1); };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 { return Ok(0); }
    Ok(number.trunc().min(usize::MAX as f64) as usize)
}
fn flatten(values: &[Value], depth: usize) -> Vec<Value> {
    // Allocate the result once and append through the whole traversal. The
    // previous recursive form allocated one temporary Vec per nested array,
    // then copied each temporary into its parent.
    let mut result = Vec::with_capacity(values.len());
    flatten_into(values, depth, &mut result);
    result
}

fn flatten_into(values: &[Value], depth: usize, result: &mut Vec<Value>) {
    for value in values {
        if depth > 0 {
            if let Value::Array(nested) = value {
                flatten_into(&nested.snapshot(), depth - 1, result);
                continue;
            }
        }
        result.push(value.clone());
    }
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
    // Each source element contributes at least one output slot in the common case.
    // Reserve that lower bound once; nested results can still grow the vector.
    let mut mapped = Vec::with_capacity(values.len());
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for (index, value) in values.snapshot().into_iter().enumerate() {
        let args = [
            value,
            Value::Number(index as f64),
            receiver.cloned().unwrap_or(Value::Undefined),
        ];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        match result {
            Value::Array(nested) => mapped.extend(nested.snapshot()),
            value => mapped.push(value),
        }
    }
    Ok(Value::array(mapped))
}
pub(crate) fn at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.at called on null or undefined",
        ));
    };
    let Value::Array(values) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.at called on non-array",
        ));
    };
    let number = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if number.is_nan() {
        return Ok(values.first().unwrap_or(Value::Undefined));
    }
    let index = number.trunc();
    let length = values.len() as f64;
    let position = if index < 0.0 { length + index } else { index };
    if position < 0.0 || position >= length {
        return Ok(Value::Undefined);
    }
    Ok(values
        .get_index(position as usize)
        .unwrap_or(Value::Undefined))
}
pub(crate) fn to_reversed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let this = receiver.cloned().unwrap_or(Value::Undefined);
    if matches!(this, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.toReversed called on null or undefined",
        ));
    }
    let length = array_like_length(&this)?;
    let mut values = Vec::with_capacity(length);
    for index in (0..length).rev() {
        values.push(crate::execute::get_property_result(
            &this,
            &index.to_string(),
        )?);
    }
    Ok(Value::array(values))
}
pub(crate) fn reduce_values(
    receiver: Option<&Value>,
    arguments: &[Value],
    reverse: bool,
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.reduce called on null or undefined",
        ));
    };
    if matches!(receiver, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.reduce called on null or undefined",
        ));
    }
    let Some(callback) = arguments.first() else {
        return Err(crate::vm::not_callable());
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let length = crate::builtins::map_length(receiver)?;
    let initial = arguments.get(1).cloned();
    let mut index = if reverse { length } else { 0 };
    let mut accumulator = initial;
    while accumulator.is_none() {
        if reverse {
            if index == 0 { break; }
            index -= 1;
        } else if index >= length { break; } else {
            index += 1;
        }
        let position = if reverse { index } else { index - 1 };
        if let Some(value) = crate::builtins::map_value(receiver, position)? {
            accumulator = Some(value);
        }
    }
    let Some(mut accumulator) = accumulator else {
        return Err(crate::value::error::throw_type_error("Reduce of empty array"));
    };
    let indices: Box<dyn Iterator<Item = usize>> = if reverse {
        Box::new((0..index).rev())
    } else {
        Box::new(index..length)
    };
    for position in indices {
        let Some(value) = crate::builtins::map_value(receiver, position)? else { continue; };
        let args = [accumulator, value, Value::Number(position as f64), receiver.clone()];
        accumulator = crate::functions::execute_target(callback, receiver, &args)?;
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
include!("arrays_concat.rs");
include!("arrays_slice.rs");
include!("arrays_index_of.rs");

include!("arrays_tests.rs");
