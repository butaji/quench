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
    match builtin {
        Array => Some(crate::builtins::array(arguments)),
        ArrayIsArray => Some(crate::builtins::is_array(arguments.first())),
        ArrayFrom => Some(from(receiver, arguments)),
        ArrayFromAsync => Some(from_async(receiver, arguments)),
        ArrayOf => Some(of(receiver, arguments)),
        ArrayMap => Some(crate::builtins::array_map(receiver, arguments)),
        ArrayFilter => Some(crate::builtins::array_filter(receiver, arguments)),
        ArraySome => Some(some(receiver, arguments)),
        ArrayEvery => Some(every(receiver, arguments)),
        TypedArrayEvery => Some(typed_array_every(receiver, arguments)),
        TypedArraySome => Some(typed_array_some(receiver, arguments)),
        TypedArrayMap => Some(typed_array_map(receiver, arguments)),
        TypedArrayFilter => Some(typed_array_filter(receiver, arguments)),
        TypedArraySlice => Some(receiver.map_or_else(
                || {
                    Err(crate::value::error::throw_type_error(
                        "TypedArray method called on incompatible receiver",
                    ))
                },
                |value| typed_array_slice(value, arguments),
            )),
        ArrayFind => Some(find(receiver, arguments)),
        TypedArrayFind => Some(typed_array_find(receiver, arguments)),
        TypedArrayFindIndex => Some(typed_array_find_index(receiver, arguments)),
        ArrayIncludes => Some(includes(receiver, arguments)),
        TypedArrayIncludes => Some(typed_array_includes(receiver, arguments)),
        ArrayIndexOf => Some(index_of(receiver, arguments)),
        TypedArrayIndexOf => Some(typed_array_index_of(receiver, arguments)),
        ArrayLastIndexOf => Some(last_index_of(receiver, arguments)),
        TypedArrayLastIndexOf => Some(typed_array_last_index_of(receiver, arguments)),
        ArraySlice => Some(slice(receiver, arguments)),
        ArrayConcat => Some(concat(receiver, arguments)),
        ArrayFlat => Some(flat(receiver, arguments)),
        ArrayFlatMap => Some(flat_map(receiver, arguments)),
        ArrayAt => Some(at(receiver, arguments)),
        TypedArrayAt => Some(typed_array_at(receiver, arguments)),
        ArraySort => Some(sort(receiver, arguments)),
        ArrayToReversed => Some(to_reversed(receiver)),
        TypedArrayToReversed => Some(typed_array_to_reversed(receiver)),
        ArraySplice => Some(splice(receiver, arguments)),
        ArrayReduce => Some(reduce_values(receiver, arguments, false, false)),
        ArrayReduceRight => Some(reduce_values(receiver, arguments, true, false)),
        TypedArrayReduce => Some(typed_array_reduce(receiver, arguments, false)),
        TypedArrayReduceRight => Some(typed_array_reduce(receiver, arguments, true)),
        ArrayForEach => Some(crate::builtins::array_for_each(receiver, arguments)),
        TypedArrayForEach => Some(typed_array_for_each(receiver, arguments)),
        ArrayToLocaleString => Some(array_to_locale_string(receiver, arguments)),
        TypedArrayToLocaleString => Some(typed_array_to_locale_string(receiver, arguments)),
        _ => None,
    }
}

fn typed_array_for_each(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "forEach")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.forEach called on out-of-bounds view",
        ));
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |item| item);
    for index in 0..length {
        let item = crate::builtins::map_value(&value, index)?.unwrap_or(Value::Undefined);
        crate::functions::execute_target(
            callback,
            this_arg,
            &[item, Value::Number(index as f64), value.clone()],
        )?;
    }
    Ok(Value::Undefined)
}

fn typed_array_every(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "every")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.every called on out-of-bounds view",
        ));
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |item| item);
    for index in 0..length {
        let item = crate::builtins::map_value(&value, index)?.unwrap_or(Value::Undefined);
        let result = crate::functions::execute_target(
            callback,
            this_arg,
            &[item, Value::Number(index as f64), value.clone()],
        )?;
        if !crate::execute::is_truthy(&result) {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

fn typed_array_some(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "some")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.some called on out-of-bounds view",
        ));
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |item| item);
    for index in 0..length {
        let item = crate::builtins::map_value(&value, index)?.unwrap_or(Value::Undefined);
        let result = crate::functions::execute_target(
            callback,
            this_arg,
            &[item, Value::Number(index as f64), value.clone()],
        )?;
        if crate::execute::is_truthy(&result) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

fn typed_array_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "map")?;
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |item| item);
    let mapped_target = typed_array_species_create(&value, length)?;
    let mut mapped = Vec::with_capacity(length);
    for index in 0..length {
        let item = typed_array_element_for_map(&value, index)?;
        mapped.push(crate::functions::execute_target(
            callback,
            this_arg,
            &[item, Value::Number(index as f64), value.clone()],
        )?);
    }
    for (index, item) in mapped.into_iter().enumerate() {
        crate::properties::assign_set_property(&mapped_target, &index.to_string(), item)?;
    }
    Ok(mapped_target)
}

fn typed_array_filter(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "filter")?;
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |item| item);
    let mut filtered = Vec::new();
    for index in 0..length {
        let item = typed_array_element_for_map(&value, index)?;
        let keep = crate::functions::execute_target(
            callback,
            this_arg,
            &[item.clone(), Value::Number(index as f64), value.clone()],
        )?;
        if crate::execute::is_truthy(&keep) {
            filtered.push(item);
        }
    }
    let result = typed_array_species_create(&value, filtered.len())?;
    for (index, item) in filtered.into_iter().enumerate() {
        crate::properties::assign_set_property(&result, &index.to_string(), item)?;
    }
    Ok(result)
}

fn typed_array_element_for_map(
    value: &Value,
    index: usize,
) -> Result<Value, crate::execute::VmError> {
    if crate::typed_array_prototype::is_out_of_bounds(value)
        || crate::typed_array_ops::logical_len(value).unwrap_or(0) <= index
    {
        return Ok(Value::Undefined);
    }
    crate::execute::get_property_result(value, &index.to_string())
}

pub(crate) fn typed_array_receiver(
    receiver: Option<&Value>,
    _method: &str,
) -> Result<Value, crate::execute::VmError> {
    let value = receiver.map(unwrap_binding_cells);
    let Some(value) = value.as_ref().filter(|value| is_typed_array(value)) else {
        return Err(crate::value::error::throw_type_error(
            "TypedArray method called on incompatible receiver",
        ));
    };
    if typed_array_is_detached(value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray method called on detached TypedArray",
        ));
    }
    if crate::typed_array_prototype::is_out_of_bounds(value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray method called on out-of-bounds view",
        ));
    }
    Ok(value.clone())
}

fn typed_array_find(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "find")?;
    find(Some(&value), arguments)
}

fn typed_array_find_index(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "findIndex")?;
    find_index(Some(&value), arguments)
}

fn typed_array_find_last(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "findLast")?;
    crate::builtins::array_find_last(Some(&value), arguments)
}

fn typed_array_find_last_index(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "findLastIndex")?;
    crate::builtins::array_find_last_index(Some(&value), arguments)
}

fn typed_array_includes(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "includes")?;
    typed_includes(Some(&value), arguments)
}

fn typed_array_index_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "indexOf")?;
    typed_index_of(Some(&value), arguments)
}

fn typed_array_last_index_of(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "lastIndexOf")?;
    typed_last_index_of(Some(&value), arguments)
}

fn typed_array_at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "at")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.at called on out-of-bounds view",
        ));
    }
    at(Some(&value), arguments)
}

pub(crate) fn typed_array_join(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "join")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.join called on out-of-bounds view",
        ));
    }
    crate::builtins::array_join(Some(&value), arguments)
}

fn typed_array_to_locale_string(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "toLocaleString")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.toLocaleString called on out-of-bounds view",
        ));
    }
    array_to_locale_string(Some(&value), arguments)
}

fn typed_array_reverse(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "reverse")?;
    let immutable = match &value {
        Value::Float64Array(view) => view.buffer.immutable,
        Value::Float32Array(view) => view.buffer.immutable,
        Value::Int8Array(view) => view.buffer.immutable,
        Value::Int16Array(view) => view.buffer.immutable,
        Value::Int32Array(view) => view.buffer.immutable,
        Value::Uint8Array(view) => view.buffer.immutable,
        Value::Uint8ClampedArray(view) => view.buffer.immutable,
        Value::Uint16Array(view) => view.buffer.immutable,
        Value::Uint32Array(view) => view.buffer.immutable,
        Value::BigInt64Array(view) => view.buffer.immutable,
        Value::BigUint64Array(view) => view.buffer.immutable,
        _ => false,
    };
    if immutable || crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.reverse called on invalid view",
        ));
    }
    crate::builtins::array_reverse(Some(&value))
}

fn typed_array_reduce(
    receiver: Option<&Value>,
    arguments: &[Value],
    reverse: bool,
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, if reverse { "reduceRight" } else { "reduce" })?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray reduce called on out-of-bounds view",
        ));
    }
    reduce_values(Some(&value), arguments, reverse, true)
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
    let _ = arguments;
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
        crate::ops::Builtin::TypedArrayKeys => Some(typed_array_keys(receiver)),
        crate::ops::Builtin::ArrayEntries => Some(array_entries(receiver)),
        crate::ops::Builtin::TypedArrayEntries => Some(typed_array_entries(receiver)),
        _ => None,
    }
}

include!("arrays_mutation.rs");
include!("arrays_typed_static.rs");
include!("arrays_from.rs");

fn splice(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined))
    else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.splice called on null or undefined",
        ));
    };
    let mut target = crate::construct::to_object(receiver)?;
    let length = array_like_length(&target)?;
    let start = splice_index(arguments.first(), length)?;
    let available = length - start;
    let delete_count = match arguments.get(1) {
        None if arguments.len() == 1 => available,
        None => 0,
        Some(value) => splice_count(value)?.min(available),
    };
    let mut removed = crate::builtins::array_species_create(&target, delete_count)?;
    for offset in 0..delete_count {
        if let Some(value) = splice_value(&target, start + offset)? {
            removed = crate::builtins::create_data_property_or_throw(
                removed,
                &offset.to_string(),
                value,
            )?;
        }
    }
    removed = crate::properties::assign_set_property(
        &removed,
        "length",
        Value::Number(delete_count as f64),
    )?;
    let items: Vec<Value> = arguments.iter().skip(2).cloned().collect();
    let new_length = length - delete_count + items.len();
    if (new_length as u64) > 9_007_199_254_740_991u64 {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.splice result exceeds integer limit",
        ));
    }
    if items.len() < delete_count {
        for index in start..(length - delete_count) {
            let from = index + delete_count;
            target = splice_move(&target, index + items.len(), &target, from)?;
        }
        for index in (new_length..length).rev() {
            target = splice_delete(target, index)?;
        }
    } else if items.len() > delete_count {
        for index in (start..(length - delete_count)).rev() {
            let from = index + delete_count;
            target = splice_move(&target, index + items.len(), &target, from)?;
        }
    }
    for (offset, value) in items.into_iter().enumerate() {
        target =
            crate::properties::assign_set_property(&target, &(start + offset).to_string(), value)?;
    }
    target = crate::properties::assign_set_property(
        &target,
        "length",
        Value::Number(new_length as f64),
    )?;
    crate::locals::replace_value(receiver, &target);
    Ok(removed)
}

fn splice_index(value: Option<&Value>, length: usize) -> Result<usize, crate::execute::VmError> {
    let number = value
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    if number.is_nan() || number == 0.0 {
        return Ok(0);
    }
    if number.is_sign_negative() {
        return Ok((length as f64 + number.trunc()).max(0.0) as usize);
    }
    Ok(number.min(length as f64).trunc() as usize)
}

fn splice_count(value: &Value) -> Result<usize, crate::execute::VmError> {
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.min(9_007_199_254_740_991.0).trunc() as usize)
}

fn splice_move(
    target: &Value,
    to: usize,
    source: &Value,
    from: usize,
) -> Result<Value, crate::execute::VmError> {
    match splice_value(source, from)? {
        Some(value) => crate::properties::assign_set_property(target, &to.to_string(), value),
        None => splice_delete(target.clone(), to),
    }
}

fn splice_value(source: &Value, index: usize) -> Result<Option<Value>, crate::execute::VmError> {
    let key = index.to_string();
    if !crate::with_scope::has_property(source, &key)? {
        return Ok(None);
    }
    crate::execute::get_property_result(source, &key).map(Some)
}

fn splice_delete(target: Value, index: usize) -> Result<Value, crate::execute::VmError> {
    let key = index.to_string();
    let (updated, deleted) = crate::builtins::delete_property(target, &key);
    if deleted {
        Ok(updated)
    } else {
        Err(crate::value::error::throw_type_error(
            "Cannot delete array property",
        ))
    }
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

/// Resolve a method on an array whose ordinary packed proof and the global
/// Array.prototype cleanliness proof have already been established.  This
/// keeps the hot named-call/property paths from rebuilding the generic
/// descriptor lookup (including its override-map probe) on every iteration.
#[inline]
pub(crate) fn packed_method(key: &str) -> Option<crate::ops::Builtin> {
    array_method(key)
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
        "join" => crate::ops::Builtin::ArrayJoin,
        "toString" => crate::ops::Builtin::ArrayToString,
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

fn sort(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let compare = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined));
    if let Some(compare) = compare {
        if !crate::conversion::is_callable(compare) {
            return Err(crate::value::error::throw_type_error(
                "The comparison function must be either a function or undefined",
            ));
        }
    }
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined))
    else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.sort called on null or undefined",
        ));
    };
    let mut target = crate::construct::to_object(receiver)?;
    let length = array_like_length(&target)?;
    let mut elements = Vec::new();
    for index in 0..length {
        if let Some(value) = crate::builtins::map_value(&target, index)? {
            elements.push(value);
        }
        target = crate::locals::resolved_replacement(target);
    }
    insertion_sort(&mut elements, compare)?;
    for index in 0..length {
        let key = index.to_string();
        if let Some(value) = elements.get(index).cloned() {
            let previous = target.clone();
            let updated = crate::properties::assign_set_property(&target, &key, value)?;
            crate::locals::replace_value(&previous, &updated);
            target = updated;
        } else {
            let previous = target.clone();
            let (updated, _) = crate::builtins::delete_property(target, &key);
            crate::locals::replace_value(&previous, &updated);
            target = updated;
        }
    }
    crate::locals::replace_value(receiver, &target);
    Ok(target)
}

fn insertion_sort(
    elements: &mut [Value],
    compare: Option<&Value>,
) -> Result<(), crate::execute::VmError> {
    for index in 1..elements.len() {
        let value = elements[index].clone();
        let mut position = index;
        while position > 0 {
            let ordering = compare_values(&elements[position - 1], &value, compare)?;
            if ordering != std::cmp::Ordering::Greater {
                break;
            }
            elements[position] = elements[position - 1].clone();
            position -= 1;
        }
        elements[position] = value;
    }
    Ok(())
}

fn compare_values(
    left: &Value,
    right: &Value,
    compare: Option<&Value>,
) -> Result<std::cmp::Ordering, crate::execute::VmError> {
    let left_undefined = matches!(left, Value::Undefined);
    let right_undefined = matches!(right, Value::Undefined);
    if left_undefined || right_undefined {
        return Ok(match (left_undefined, right_undefined) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => std::cmp::Ordering::Equal,
        });
    }
    let number = if let Some(compare) = compare {
        let value =
            crate::execute::call(compare, &Value::Undefined, &[left.clone(), right.clone()])?;
        crate::conversion::to_number(&value)?
    } else {
        let left = crate::conversion::to_string(left)?;
        let right = crate::conversion::to_string(right)?;
        return Ok(left.cmp(&right));
    };
    Ok(number
        .partial_cmp(&0.0)
        .unwrap_or(std::cmp::Ordering::Equal))
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
    let mut target = crate::builtins::array_species_create(&receiver, 0)?;
    let mut values = Vec::with_capacity(length);
    for index in 0..length {
        if let Some(value) = crate::builtins::map_value(&receiver, index)? {
            flatten_value(&value, depth, &mut values)?;
        }
    }
    for (index, value) in values.into_iter().enumerate() {
        target = crate::builtins::create_data_property_or_throw(target, &index.to_string(), value)?;
    }
    Ok(target)
}

fn flat_depth(value: Option<&Value>) -> Result<usize, crate::execute::VmError> {
    let Some(value) = value else {
        return Ok(1);
    };
    if matches!(value, Value::Undefined) {
        return Ok(1);
    }
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 {
        return Ok(0);
    }
    Ok(number.trunc().min(usize::MAX as f64) as usize)
}
fn flatten_value(
    value: &Value,
    depth: usize,
    result: &mut Vec<Value>,
) -> Result<(), crate::execute::VmError> {
    if depth > 0
        && matches!(
            crate::builtins::is_array(Some(value))?,
            Value::Boolean(true)
        )
    {
        let length = crate::builtins::map_length(value)?;
        for index in 0..length {
            if let Some(nested) = crate::builtins::map_value(value, index)? {
                flatten_value(&nested, depth - 1, result)?;
            }
        }
    } else {
        result.push(value.clone());
    }
    Ok(())
}
pub(crate) fn flat_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined))
    else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.flatMap called on null or undefined",
        ));
    };
    let receiver = crate::construct::to_object(receiver)?;
    let Some(callback) = arguments
        .first()
        .filter(|value| crate::conversion::is_callable(value))
    else {
        return Err(crate::vm::not_callable());
    };
    let length = crate::builtins::map_length(&receiver)?;
    let mut target = crate::builtins::array_species_create(&receiver, 0)?;
    let mut target_index = 0usize;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = crate::builtins::map_value(&receiver, index)? else {
            continue;
        };
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let result = crate::functions::execute_target(callback, this_arg, &args)?;
        let mut flattened = Vec::new();
        flatten_value(&result, 1, &mut flattened)?;
        for value in flattened {
            target = crate::builtins::create_data_property_or_throw(
                target,
                &target_index.to_string(),
                value,
            )?;
            target_index += 1;
        }
    }
    Ok(target)
}
pub(crate) fn at(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined))
    else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.at called on null or undefined",
        ));
    };
    let receiver = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&receiver)?;
    let number = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if number.is_nan() {
        return Ok(crate::execute::get_property_result(&receiver, "0")?);
    }
    let index = number.trunc();
    let length = length as f64;
    let position = if index < 0.0 { length + index } else { index };
    if position < 0.0 || position >= length {
        return Ok(Value::Undefined);
    }
    Ok(crate::execute::get_property_result(
        &receiver,
        &(position as usize).to_string(),
    )?)
}
pub(crate) fn to_reversed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let this = receiver.cloned().unwrap_or(Value::Undefined);
    if matches!(this, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.toReversed called on null or undefined",
        ));
    }
    let length = array_like_length(&this)?;
    if length > u32::MAX as usize {
        return Err(crate::value::error::throw_range_error(
            "Invalid array length",
        ));
    }
    let mut values = Vec::with_capacity(length);
    for index in (0..length).rev() {
        values.push(crate::execute::get_property_result(
            &this,
            &index.to_string(),
        )?);
    }
    Ok(Value::array(values))
}
fn typed_array_to_reversed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "toReversed")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.toReversed called on out-of-bounds view",
        ));
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let mut values = Vec::with_capacity(length);
    for index in (0..length).rev() {
        values.push(crate::execute::get_property_result(
            &value,
            &index.to_string(),
        )?);
    }
    construct_typed_array_result(&value, values)
}

fn typed_array_to_sorted(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "toSorted")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.toSorted called on out-of-bounds view",
        ));
    }
    let compare = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined));
    if let Some(compare) = compare {
        if !crate::conversion::is_callable(compare) {
            return Err(crate::value::error::throw_type_error(
                "TypedArray.prototype.toSorted comparator is not callable",
            ));
        }
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let mut values = Vec::with_capacity(length);
    for index in 0..length {
        values.push(crate::execute::get_property_result(
            &value,
            &index.to_string(),
        )?);
    }
    for index in 1..values.len() {
        let item = values[index].clone();
        let mut position = index;
        while position > 0
            && typed_array_compare_values(&values[position - 1], &item, compare)?
                == std::cmp::Ordering::Greater
        {
            values[position] = values[position - 1].clone();
            position -= 1;
        }
        values[position] = item;
    }
    construct_typed_array_result(&value, values)
}

fn typed_array_sort(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "sort")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) || typed_array_is_immutable(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.sort called on invalid view",
        ));
    }
    let compare = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined));
    if let Some(compare) = compare {
        if !crate::conversion::is_callable(compare) {
            return Err(crate::value::error::throw_type_error(
                "The comparison function must be either a function or undefined",
            ));
        }
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let mut values = (0..length)
        .map(|index| crate::execute::get_property_result(&value, &index.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    for index in 1..values.len() {
        let item = values[index].clone();
        let mut position = index;
        while position > 0
            && typed_array_compare_values(&values[position - 1], &item, compare)?
                == std::cmp::Ordering::Greater
        {
            values[position] = values[position - 1].clone();
            position -= 1;
        }
        values[position] = item;
    }
    for (index, item) in values.into_iter().enumerate() {
        crate::properties::assign_set_property(&value, &index.to_string(), item)?;
    }
    Ok(value)
}

pub(crate) fn typed_array_is_immutable(value: &Value) -> bool {
    match value {
        Value::Float64Array(view) => view.buffer.immutable,
        Value::Float32Array(view) => view.buffer.immutable,
        Value::Int8Array(view) => view.buffer.immutable,
        Value::Int16Array(view) => view.buffer.immutable,
        Value::Int32Array(view) => view.buffer.immutable,
        Value::Uint8Array(view) => view.buffer.immutable,
        Value::Uint16Array(view) => view.buffer.immutable,
        Value::Uint32Array(view) => view.buffer.immutable,
        Value::Uint8ClampedArray(view) => view.buffer.immutable,
        Value::BigInt64Array(view) => view.buffer.immutable,
        Value::BigUint64Array(view) => view.buffer.immutable,
        _ => false,
    }
}

fn typed_array_with(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = typed_array_receiver(receiver, "with")?;
    if crate::typed_array_prototype::is_out_of_bounds(&value) {
        return Err(crate::value::error::throw_type_error(
            "TypedArray.prototype.with called on out-of-bounds view",
        ));
    }
    let length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    let number = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let integer = if number.is_nan() { 0.0 } else { number.trunc() };
    let index = if integer < 0.0 {
        length as f64 + integer
    } else {
        integer
    };
    let replacement = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let replacement = if matches!(value, Value::BigInt64Array(_) | Value::BigUint64Array(_)) {
        match crate::conversion::to_primitive(&replacement, "number")? {
            Value::BigInt(value) => Value::BigInt(value),
            Value::String(value) => Value::BigInt(
                crate::bigint::parse_string(&value)
                    .ok_or_else(|| crate::value::error::throw_syntax_error("Invalid BigInt value"))?
                    .to_string(),
            ),
            Value::Boolean(value) => Value::BigInt(if value { "1" } else { "0" }.to_string()),
            _ => {
                return Err(crate::value::error::throw_type_error(
                    "Cannot convert value to BigInt",
                ))
            }
        }
    } else {
        Value::Number(crate::conversion::to_number(&replacement)?)
    };
    let current_length = crate::typed_array_ops::logical_len(&value).unwrap_or(0);
    if index < 0.0 || index as usize >= current_length {
        return Err(crate::value::error::throw_range_error("Invalid index"));
    }
    let mut values = Vec::with_capacity(length);
    for current in 0..length {
        values.push(if current == index as usize {
            replacement.clone()
        } else {
            crate::execute::get_property_result(&value, &current.to_string())?
        });
    }
    construct_typed_array_result(&value, values)
}

fn typed_array_compare_values(
    left: &Value,
    right: &Value,
    compare: Option<&Value>,
) -> Result<std::cmp::Ordering, crate::execute::VmError> {
    if let Some(compare) = compare {
        let result =
            crate::execute::call(compare, &Value::Undefined, &[left.clone(), right.clone()])?;
        return Ok(crate::conversion::to_number(&result)?
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal));
    }
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => match (left.is_nan(), right.is_nan()) {
            (true, true) => Ok(std::cmp::Ordering::Equal),
            (true, false) => Ok(std::cmp::Ordering::Greater),
            (false, true) => Ok(std::cmp::Ordering::Less),
            (false, false) if *left == 0.0 && *right == 0.0 => {
                Ok(match (left.is_sign_negative(), right.is_sign_negative()) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                })
            }
            (false, false) => Ok(left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)),
        },
        (Value::BigInt(left), Value::BigInt(right)) => Ok(left
            .parse::<num_bigint::BigInt>()
            .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt representation"))?
            .cmp(&right.parse::<num_bigint::BigInt>().map_err(|_| {
                crate::value::error::throw_type_error("Invalid BigInt representation")
            })?)),
        _ => Ok(std::cmp::Ordering::Equal),
    }
}

fn typed_array_species_create(
    exemplar: &Value,
    length: usize,
) -> Result<Value, crate::execute::VmError> {
    let default = typed_array_constructor(exemplar)?;
    let constructor = typed_array_species_constructor(exemplar, default)?;
    let species = if matches!(constructor, Value::Undefined) {
        Value::Builtin(default)
    } else {
        if !crate::value::is_object(&constructor) {
            return Err(crate::value::error::throw_type_error(
                "TypedArray constructor is not an object",
            ));
        }
        match crate::execute::get_property_result(&constructor, "Symbol.species")? {
            Value::Undefined | Value::Null => Value::Builtin(default),
            species => species,
        }
    };
    let result = crate::construct::construct_value(&species, &[Value::Number(length as f64)])?;
    if !result.is_typed_array() {
        return Err(crate::value::error::throw_type_error(
            "TypedArray species constructor returned a non-TypedArray",
        ));
    }
    validate_typed_array_species_target(&result, length, Some(exemplar))?;
    Ok(result)
}

fn validate_typed_array_species_target(
    target: &Value,
    length: usize,
    exemplar: Option<&Value>,
) -> Result<(), crate::execute::VmError> {
    if crate::typed_array_prototype::is_out_of_bounds(target)
        || (typed_array_is_immutable(target)
            && !exemplar.is_some_and(|value| typed_array_same_buffer(value, target)))
        || crate::typed_array_ops::logical_len(target).unwrap_or(0) < length
    {
        return Err(crate::value::error::throw_type_error(
            "TypedArray species constructor returned an invalid target",
        ));
    }
    Ok(())
}

fn typed_array_same_buffer(left: &Value, right: &Value) -> bool {
    fn buffer(value: &Value) -> Option<&std::rc::Rc<crate::value::ArrayBufferData>> {
        match value {
            Value::Float64Array(view) => Some(&view.buffer),
            Value::Float32Array(view) => Some(&view.buffer),
            Value::Int8Array(view) => Some(&view.buffer),
            Value::Int16Array(view) => Some(&view.buffer),
            Value::Int32Array(view) => Some(&view.buffer),
            Value::Uint8Array(view) => Some(&view.buffer),
            Value::Uint8ClampedArray(view) => Some(&view.buffer),
            Value::Uint16Array(view) => Some(&view.buffer),
            Value::Uint32Array(view) => Some(&view.buffer),
            Value::BigInt64Array(view) => Some(&view.buffer),
            Value::BigUint64Array(view) => Some(&view.buffer),
            _ => None,
        }
    }
    buffer(left)
        .zip(buffer(right))
        .is_some_and(|(left, right)| std::rc::Rc::ptr_eq(left, right))
}

pub(crate) fn typed_array_species_constructor(
    exemplar: &Value,
    default: crate::ops::Builtin,
) -> Result<Value, crate::execute::VmError> {
    let has_own = crate::typed_array_prototype::own_property(exemplar, "constructor").is_some()
        || crate::typed_array_prototype::descriptor(exemplar, "constructor").is_some();
    if !has_own {
        if let Some(prototype) = typed_array_prototype_builtin(default) {
            let prototype = crate::typed_array_prototype::get(exemplar)
                .unwrap_or_else(|| crate::vm::realm_intrinsic(prototype));
            if let Some(result) =
                typed_array_prototype_override(&prototype, "constructor", exemplar)
            {
                return result;
            }
        }
    }
    crate::execute::get_property_result(exemplar, "constructor")
}

fn typed_array_prototype_builtin(constructor: crate::ops::Builtin) -> Option<crate::ops::Builtin> {
    use crate::ops::Builtin;
    Some(match constructor {
        Builtin::Float64Array => Builtin::Float64ArrayPrototype,
        Builtin::Float32Array => Builtin::Float32ArrayPrototype,
        Builtin::Int8Array => Builtin::Int8ArrayPrototype,
        Builtin::Int16Array => Builtin::Int16ArrayPrototype,
        Builtin::Int32Array => Builtin::Int32ArrayPrototype,
        Builtin::Uint8Array => Builtin::Uint8ArrayPrototype,
        Builtin::Uint16Array => Builtin::Uint16ArrayPrototype,
        Builtin::Uint32Array => Builtin::Uint32ArrayPrototype,
        Builtin::Uint8ClampedArray => Builtin::Uint8ClampedArrayPrototype,
        Builtin::BigInt64Array => Builtin::BigInt64ArrayPrototype,
        Builtin::BigUint64Array => Builtin::BigUint64ArrayPrototype,
        _ => return None,
    })
}

fn typed_array_prototype_override(
    prototype: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, crate::execute::VmError>> {
    if let Value::Builtin(builtin) = prototype {
        if crate::builtins::read_intrinsic_override(*builtin, key).is_some() {
            return Some(Ok(crate::vm::intrinsic_override_property(
                *builtin, key, receiver,
            )
            .unwrap_or(Value::Undefined)));
        }
        return None;
    }
    let Value::BoundFunction(bound) = prototype else {
        return Some(crate::execute::get_property_result(prototype, key));
    };
    let descriptor_key = crate::builtins::descriptor_key(key);
    let descriptor = bound
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == &descriptor_key).then_some(value.clone()))?;
    let Value::Object(fields) = descriptor else {
        return Some(Ok(Value::Undefined));
    };
    if let Some(getter) = fields
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "get").then_some(value.clone()))
    {
        return Some(match getter {
            Value::Undefined => Ok(Value::Undefined),
            getter => crate::functions::execute_target(&getter, receiver, &[]),
        });
    }
    let value = fields
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "value").then_some(value.clone()));
    value.map(Ok)
}

fn typed_array_constructor(
    exemplar: &Value,
) -> Result<crate::ops::Builtin, crate::execute::VmError> {
    let constructor = match exemplar {
        Value::Float64Array(_) => crate::ops::Builtin::Float64Array,
        Value::Float32Array(_) => crate::ops::Builtin::Float32Array,
        Value::Int8Array(_) => crate::ops::Builtin::Int8Array,
        Value::Int16Array(_) => crate::ops::Builtin::Int16Array,
        Value::Int32Array(_) => crate::ops::Builtin::Int32Array,
        Value::Uint8Array(_) => crate::ops::Builtin::Uint8Array,
        Value::Uint16Array(_) => crate::ops::Builtin::Uint16Array,
        Value::Uint32Array(_) => crate::ops::Builtin::Uint32Array,
        Value::Uint8ClampedArray(_) => crate::ops::Builtin::Uint8ClampedArray,
        Value::BigInt64Array(_) => crate::ops::Builtin::BigInt64Array,
        Value::BigUint64Array(_) => crate::ops::Builtin::BigUint64Array,
        _ => return Err(crate::value::error::throw_type_error("Not a TypedArray")),
    };
    Ok(constructor)
}

fn construct_typed_array_result(
    exemplar: &Value,
    values: Vec<Value>,
) -> Result<Value, crate::execute::VmError> {
    let constructor = typed_array_constructor(exemplar)?;
    let source = Value::Array(std::rc::Rc::new(crate::value::ArrayData::new(values)));
    crate::construct::construct_value(&Value::Builtin(constructor), &[source])
}

pub(crate) fn reduce_values(
    receiver: Option<&Value>,
    arguments: &[Value],
    reverse: bool,
    typed: bool,
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
    let receiver = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&receiver)?;
    let Some(callback) = arguments.first() else {
        return Err(crate::vm::not_callable());
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let initial = arguments.get(1).cloned();
    let mut index = if reverse { length } else { 0 };
    let mut accumulator = initial;
    while accumulator.is_none() {
        if reverse {
            if index == 0 {
                break;
            }
            index -= 1;
        } else if index >= length {
            break;
        } else {
            index += 1;
        }
        let position = if reverse { index } else { index - 1 };
        let value = crate::builtins::map_value(&receiver, position)?;
        let value = if typed && value.is_none() {
            Some(Value::Undefined)
        } else {
            value
        };
        if let Some(value) = value {
            accumulator = Some(value);
        }
    }
    let Some(mut accumulator) = accumulator else {
        return Err(crate::value::error::throw_type_error(
            "Reduce of empty array",
        ));
    };
    // Keep the traversal state in two machine words.  A boxed iterator here
    // adds a heap allocation and an indirect call to a hot, linear loop;
    // explicit bounds are just as clear and let LLVM inline the body.
    let mut position = index;
    loop {
        let current = if reverse {
            if position == 0 {
                break;
            }
            position -= 1;
            position
        } else if position >= length {
            break;
        } else {
            let current = position;
            position += 1;
            current
        };
        let value = crate::builtins::map_value(&receiver, current)?;
        let value = if typed && value.is_none() {
            Some(Value::Undefined)
        } else {
            value
        };
        let Some(value) = value else { continue };
        let args = [
            accumulator,
            value,
            Value::Number(current as f64),
            receiver.clone(),
        ];
        accumulator = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
    }
    Ok(accumulator)
}
include!("arrays_concat.rs");
include!("arrays_slice.rs");
include!("arrays_index_of.rs");

include!("arrays_tests.rs");
