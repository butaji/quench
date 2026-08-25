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
    if builtin == ArrayFlat {
        return Some(flat(receiver, arguments));
    }
    match builtin {
        Array => return Some(crate::builtins::array(arguments)),
        ArrayIsArray => return Some(crate::builtins::is_array(arguments.first())),
        ArrayFrom => return Some(from(receiver, arguments)),
        ArrayOf => {
            return Some(of(receiver, arguments));
        }
        ArrayMap => return Some(crate::builtins::array_map(receiver, arguments)),
        ArrayFilter => return Some(crate::builtins::array_filter(receiver, arguments)),
        ArraySome => return Some(some(receiver, arguments)),
        ArrayEvery => return Some(every(receiver, arguments)),
        ArrayFind => return Some(find(receiver, arguments)),
        ArrayFindIndex => return Some(find_index(receiver, arguments)),
        ArrayIncludes => return Some(includes(receiver, arguments)),
        ArrayIndexOf => return Some(index_of(receiver, arguments)),
        ArrayLastIndexOf => return Some(last_index_of(receiver, arguments)),
        ArraySlice => return Some(slice(receiver, arguments)),
        ArrayConcat => return Some(concat(receiver, arguments)),
        ArrayFlatMap => return Some(flat_map(receiver, arguments)),
        ArrayAt => return Some(at(receiver, arguments)),
        ArraySort => return Some(sort(receiver, arguments)),
        ArrayToReversed => return Some(to_reversed(receiver)),
        ArraySplice => return Some(splice(receiver, arguments)),
        ArrayReduce => return Some(reduce_values(receiver, arguments, false)),
        ArrayReduceRight => return Some(reduce_values(receiver, arguments, true)),
        ArrayForEach => return Some(crate::builtins::array_for_each(receiver, arguments)),
        ArrayToLocaleString => return Some(array_to_locale_string(receiver, arguments)),
        _ => return None,
    }
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

fn splice(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.splice called on null or undefined",
        ));
    };
    let mut target = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&target)?;
    let start = relative_index(arguments.first(), length as isize) as usize;
    let start = start.min(length);
    let item_count = arguments.len().saturating_sub(2);
    let delete_count = match arguments.len() {
        0 => 0,
        1 => length - start,
        _ => crate::conversion::to_number(arguments.get(1).unwrap_or(&Value::Undefined))?
            .max(0.0)
            .min((length - start) as f64) as usize,
    };
    const MAX_SAFE_LENGTH: usize = 9_007_199_254_740_991;
    let resulting_length = length
        .saturating_sub(delete_count)
        .checked_add(item_count)
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid array length"))?;
    if resulting_length > MAX_SAFE_LENGTH {
        return Err(crate::value::error::throw_type_error(
            "Invalid array length",
        ));
    }
    let removed_target = crate::builtins::array_species_create(&target, delete_count)?;
    let mut removed = removed_target;
    for offset in 0..delete_count {
        let key = (start + offset).to_string();
        if crate::with_scope::has_property(&target, &key)? {
            let value = crate::execute::get_property_result(&target, &key)?;
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
    if let Value::Array(values) = &target {
        if values.is_packed_ordinary() {
            let mut updated = values.to_vec();
            let removed_start = start;
            let removed_end = start + delete_count;
            updated.splice(
                removed_start..removed_end,
                arguments.iter().skip(2).cloned(),
            );
            let updated = Value::array(updated);
            crate::locals::replace_value(receiver, &updated);
            return Ok(removed);
        }
    }
    if item_count < delete_count {
        for index in start..(length - delete_count) {
            let from = (index + delete_count).to_string();
            let to = (index + item_count).to_string();
            if crate::with_scope::has_property(&target, &from)? {
                let value = crate::execute::get_property_result(&target, &from)?;
                target = crate::properties::assign_set_property(&target, &to, value)?;
            } else {
                let (updated, deleted) = crate::builtins::delete_property(target, &to);
                if !deleted {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot delete property during splice",
                    ));
                }
                target = updated;
            }
            target = crate::locals::resolved_replacement(target);
        }
        for index in ((length - delete_count + item_count)..length).rev() {
            let (updated, deleted) = crate::builtins::delete_property(target, &index.to_string());
            if !deleted {
                return Err(crate::value::error::throw_type_error(
                    "Cannot delete property during splice",
                ));
            }
            target = crate::locals::resolved_replacement(updated);
        }
    } else if item_count > delete_count {
        for index in (start + 1..=length - delete_count).rev() {
            let from = (index + delete_count - 1).to_string();
            let to = (index + item_count - 1).to_string();
            if crate::with_scope::has_property(&target, &from)? {
                let value = crate::execute::get_property_result(&target, &from)?;
                target = crate::properties::assign_set_property(&target, &to, value)?;
            } else {
                let (updated, deleted) = crate::builtins::delete_property(target, &to);
                if !deleted {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot delete property during splice",
                    ));
                }
                target = updated;
            }
            target = crate::locals::resolved_replacement(target);
        }
    }
    for (offset, value) in arguments.iter().skip(2).cloned().enumerate() {
        target =
            crate::properties::assign_set_property(&target, &(start + offset).to_string(), value)?;
        target = crate::locals::resolved_replacement(target);
    }
    let new_length = resulting_length;
    target = crate::properties::assign_set_property(
        &crate::locals::resolved_replacement(target),
        "length",
        Value::Number(new_length as f64),
    )?;
    crate::locals::replace_value(receiver, &target);
    Ok(removed)
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
        "findIndex" => crate::ops::Builtin::ArrayFindIndex,
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
        "toSpliced" => crate::ops::Builtin::ArrayToSpliced,
        "with" => crate::ops::Builtin::ArrayWith,
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
    if values.is_arguments() {
        return values.get_index(index);
    }
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
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "value").then(|| value.clone()))
}

/// The `get` accessor of a user-defined `Array.prototype` override, e.g.
/// `Object.defineProperty(Array.prototype, "2", { get: ... })`.
pub(crate) fn prototype_override_getter(key: &str) -> Option<Value> {
    let descriptor =
        crate::builtins::read_intrinsic_override(crate::ops::Builtin::ArrayPrototype, key)?;
    let Value::Object(properties) = descriptor else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "get").then(|| value.clone()))
}

pub(crate) fn prototype_override_setter(key: &str) -> Option<Value> {
    let descriptor =
        crate::builtins::read_intrinsic_override(crate::ops::Builtin::ArrayPrototype, key)?;
    let Value::Object(properties) = descriptor else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "set").then(|| value.clone()))
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
        if let Some(live) = values.argument_live_view() {
            if live.length_override.is_some() {
                return Some(values.arguments_length_value());
            }
        }
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
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.sort called on null or undefined",
        ));
    };
    let compare = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined));
    if let Some(compare) = compare {
        if !crate::conversion::is_callable(compare) {
            return Err(crate::value::error::throw_type_error(
                "Array.prototype.sort comparator is not callable",
            ));
        }
    }
    let mut target = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&target)?;
    let mut values = Vec::new();
    let mut undefined_count = 0usize;
    for index in 0..length {
        target = crate::locals::resolved_replacement(target);
        let key = index.to_string();
        if !crate::with_scope::has_property(&target, &key)? {
            continue;
        }
        let value = crate::execute::get_property_result(&target, &key)?;
        if matches!(value, Value::Undefined) {
            undefined_count += 1;
        } else {
            values.push(value);
        }
    }
    let value_count = values.len();
    if let Some(compare) = compare {
        sort_with_comparator(&mut values, compare)?;
    } else {
        sort_by_string(&mut values)?;
    }

    target = crate::locals::resolved_replacement(target);
    for (index, value) in values.into_iter().enumerate() {
        target = sort_write(target, &index.to_string(), value)?;
        crate::locals::replace_value(receiver, &target);
    }
    for index in value_count..value_count + undefined_count {
        target = sort_write(target, &index.to_string(), Value::Undefined)?;
        crate::locals::replace_value(receiver, &target);
    }
    let final_count = value_count + undefined_count;
    for index in final_count..length {
        let (updated, deleted) = crate::builtins::delete_property(target, &index.to_string());
        if !deleted {
            return Err(crate::value::error::throw_type_error(
                "Cannot delete property during sort",
            ));
        }
        target = updated;
        crate::locals::replace_value(receiver, &target);
    }
    crate::locals::replace_value(receiver, &target);
    Ok(target)
}

fn sort_with_comparator(
    values: &mut Vec<Value>,
    compare: &Value,
) -> Result<(), crate::execute::VmError> {
    let mut index = 1;
    while index < values.len() {
        let value = values.remove(index);
        let mut position = 0;
        while position < index {
            let result = crate::functions::execute_target(
                compare,
                &Value::Undefined,
                &[value.clone(), values[position].clone()],
            )?;
            let result = crate::conversion::to_number(&result)?;
            if !result.is_nan() && result < 0.0 {
                break;
            }
            position += 1;
        }
        values.insert(position, value);
        index += 1;
    }
    Ok(())
}

fn sort_by_string(values: &mut Vec<Value>) -> Result<(), crate::execute::VmError> {
    let mut index = 1;
    while index < values.len() {
        let value = values.remove(index);
        let value_key = crate::conversion::to_string(&value)?;
        let mut position = 0;
        while position < index {
            let current_key = crate::conversion::to_string(&values[position])?;
            if value_key < current_key {
                break;
            }
            position += 1;
        }
        values.insert(position, value);
        index += 1;
    }
    Ok(())
}

fn sort_write(target: Value, key: &str, value: Value) -> Result<Value, crate::execute::VmError> {
    if let Some(setter) = sort_setter(&target, key)? {
        let (_, updated) = crate::functions::execute_target_with_receiver(
            &setter,
            &target,
            std::slice::from_ref(&value),
        )?;
        return Ok(crate::locals::resolved_replacement(updated));
    }
    if let Value::Array(_) = &target {
        if crate::builtins::descriptor_flag(&target, key, "writable") == Some(false)
            || !crate::properties::object_is_extensible(&target)
                && !crate::with_scope::has_property(&target, key)?
        {
            return Err(crate::value::error::throw_type_error(
                "Cannot assign property during sort",
            ));
        }
        return Ok(crate::builtins::set_property(target, key, value));
    }
    crate::properties::assign_set_property(&target, key, value)
}

fn sort_setter(target: &Value, key: &str) -> Result<Option<Value>, crate::execute::VmError> {
    if let Some(setter) = crate::property_define::accessor(target, key, "set") {
        if !matches!(setter, Value::Undefined) {
            return Ok(Some(setter));
        }
    }
    if let Some(Value::Object(descriptor)) =
        crate::builtins::read_intrinsic_override(crate::ops::Builtin::ObjectPrototype, key)
    {
        let setter = crate::execute::get_property_result(&Value::Object(descriptor), "set")?;
        if !matches!(setter, Value::Undefined) {
            return Ok(Some(setter));
        }
    }
    Ok(None)
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
    let receiver = crate::construct::to_object(receiver.unwrap_or(&Value::Undefined))?;
    let length = crate::builtins::map_length(&receiver)?;
    let depth = flat_depth(arguments.first())?;
    let mut target = crate::builtins::array_species_create(&receiver, 0)?;
    let mut next = 0usize;
    flatten_into(&receiver, length, depth, &mut target, &mut next)?;
    Ok(target)
}

fn flat_depth(value: Option<&Value>) -> Result<usize, crate::execute::VmError> {
    let Some(value) = value.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(1);
    };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number <= 0.0 || number == f64::NEG_INFINITY {
        return Ok(0);
    }
    if number == f64::INFINITY {
        return Ok(usize::MAX);
    }
    Ok(number.trunc() as usize)
}

fn flatten_into(
    source: &Value,
    length: usize,
    depth: usize,
    target: &mut Value,
    next: &mut usize,
) -> Result<(), crate::execute::VmError> {
    for index in 0..length {
        let key = index.to_string();
        if !crate::with_scope::has_property(source, &key)? {
            continue;
        }
        let value = crate::execute::get_property_result(source, &key)?;
        if depth > 0
            && matches!(
                crate::builtins::is_array(Some(&value))?,
                Value::Boolean(true)
            )
        {
            let nested = crate::construct::to_object(&value)?;
            let nested_length = crate::builtins::map_length(&nested)?;
            flatten_into(&nested, nested_length, depth - 1, target, next)?;
            continue;
        }
        let updated = crate::builtins::create_data_property_or_throw(
            target.clone(),
            &next.to_string(),
            value,
        )?;
        *target = updated;
        *next = next.saturating_add(1);
    }
    Ok(())
}
pub(crate) fn flat_map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = crate::construct::to_object(receiver.unwrap_or(&Value::Undefined))?;
    let length = crate::builtins::map_length(&receiver)?;
    let Some(callback) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.flatMap callback is not callable",
        ));
    };
    if !crate::conversion::is_callable(callback) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.flatMap callback is not callable",
        ));
    }
    let mut target = crate::builtins::array_species_create(&receiver, 0)?;
    let mut next = 0usize;
    let this_arg = arguments.get(1).map_or(&Value::Undefined, |value| value);
    for index in 0..length {
        let Some(value) = crate::builtins::map_value(&receiver, index)? else {
            continue;
        };
        let args = [value, Value::Number(index as f64), receiver.clone()];
        let mapped = crate::functions::execute_target(callback, this_arg, &args)?;
        if matches!(
            crate::builtins::is_array(Some(&mapped))?,
            Value::Boolean(true)
        ) {
            let nested = crate::construct::to_object(&mapped)?;
            let nested_length = crate::builtins::map_length(&nested)?;
            flatten_into(&nested, nested_length, 0, &mut target, &mut next)?;
        } else {
            target =
                crate::builtins::create_data_property_or_throw(target, &next.to_string(), mapped)?;
            next = next.saturating_add(1);
        }
    }
    Ok(target)
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
    let length = array_like_length(receiver)?;
    let number = crate::conversion::to_number(arguments.first().unwrap_or(&Value::Undefined))?;
    if number.is_nan() {
        return crate::execute::get_property_result(receiver, "0");
    }
    let index = number.trunc();
    let length = length as f64;
    let position = if index < 0.0 { length + index } else { index };
    if position < 0.0 || position >= length {
        return Ok(Value::Undefined);
    }
    let position = position as usize;
    if receiver.is_typed_array() {
        if crate::typed_array_prototype::is_out_of_bounds(receiver)
            || !crate::typed_array_prototype::index_exists(receiver, position)
        {
            return Ok(Value::Undefined);
        }
    }
    crate::execute::get_property_result(receiver, &position.to_string())
}
pub(crate) fn to_reversed(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let this = receiver.cloned().unwrap_or(Value::Undefined);
    if matches!(this, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Array.prototype.toReversed called on null or undefined",
        ));
    }
    let length = array_like_length(&this)?;
    if length >= 1usize << 32 {
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
    let receiver = crate::construct::to_object(receiver)?;
    let length = crate::builtins::map_length(&receiver)?;
    let callback = arguments.first().ok_or_else(crate::vm::not_callable)?;
    if !crate::conversion::is_callable(callback) {
        return Err(crate::vm::not_callable());
    }
    let mut index = if reverse { length } else { 0 };
    let mut accumulator = if let Some(initial) = arguments.get(1) {
        initial.clone()
    } else {
        let mut found = None;
        while if reverse { index > 0 } else { index < length } {
            if reverse {
                index -= 1;
            }
            let key = index.to_string();
            if crate::with_scope::has_property(&receiver, &key)? {
                found = Some(crate::execute::get_property_result(&receiver, &key)?);
                if !reverse {
                    index += 1;
                }
                break;
            }
            if !reverse {
                index += 1;
            }
        }
        found.ok_or_else(|| {
            crate::value::error::throw_type_error("Reduce of empty array with no initial value")
        })?
    };
    while if reverse { index > 0 } else { index < length } {
        if reverse {
            index -= 1;
        }
        let current = index;
        let key = current.to_string();
        if crate::with_scope::has_property(&receiver, &key)? {
            let value = crate::execute::get_property_result(&receiver, &key)?;
            let args = [
                accumulator.clone(),
                value,
                Value::Number(current as f64),
                receiver.clone(),
            ];
            accumulator = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
        }
        if !reverse {
            index += 1;
        }
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
