use crate::value::{IteratorData, IteratorState, Value};
use std::rc::Rc;
#[path = "iterator_map.rs"]
mod iterator_map;
#[path = "iterator_protocol.rs"]
mod iterator_protocol;
#[path = "iterator_step.rs"]
mod iterator_step;
#[path = "iterator_typed.rs"]
mod iterator_typed;
#[path = "iterator_values.rs"]
mod iterator_values;
pub(crate) use iterator_protocol::{should_update_protocol_receiver, ReceiverUpdateGuard};
pub(crate) use iterator_step::step_value;
pub(crate) use iterator_step::{resume_async_result, step_value_await};
pub(crate) use iterator_values::{
    builtin_for, from_map, from_map_keys, from_map_values, from_set, from_set_entries, make,
    make_array, make_array_entries, make_array_keys, make_regexp_string, make_string, make_typed,
    make_typed_entries, make_typed_keys, next, next_map, next_set, property_for, prototype_of,
    result,
};

pub(crate) fn concat(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let mut items = Vec::new();
    for argument in arguments {
        if !crate::value::is_object(argument) {
            return Err(crate::value::error::throw_type_error(
                "Iterator.concat requires an object",
            ));
        }
        let method = crate::execute::get_property_result(argument, "Symbol.iterator")?;
        if !crate::conversion::is_callable(&method) {
            return Err(crate::value::error::throw_type_error(
                "Iterator.concat iterator method is not callable",
            ));
        }
        items.push((argument.clone(), method));
    }
    let opened = vec![None; items.len()];
    Ok(Value::Iterator(Rc::new(IteratorData::new(
        IteratorState::Concat {
            items,
            opened,
            index: 0,
            current: None,
            done: false,
        },
    ))))
}

pub(crate) fn zip(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let inputs = arguments.first().cloned().unwrap_or(Value::Undefined);
    let options = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if !crate::value::is_object(&inputs) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.zip iterables",
        ));
    }
    let (mode, padding_option) = zip_mode(&options)?;
    let iterators = collect_zip_iterators(inputs)?;
    let padding = if let Some(padding) = padding_option {
        collect_zip_padding(padding, iterators.len())
            .map_err(|error| close_zip_iterators(iterators.clone(), error))?
    } else {
        Vec::new()
    };
    Ok(Value::Iterator(Rc::new(IteratorData::new(
        IteratorState::Zip {
            iterators,
            padding,
            mode,
            keys: None,
            started: false,
            done: false,
        },
    ))))
}

fn collect_zip_keyed_padding(
    padding: Value,
    keys: &[String],
) -> Result<Vec<Value>, crate::execute::VmError> {
    if matches!(padding, Value::Undefined) {
        return Ok(vec![Value::Undefined; keys.len()]);
    }
    keys.iter()
        .map(|key| crate::execute::get_property_result(&padding, key))
        .collect()
}

pub(crate) fn zip_keyed(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let inputs = arguments.first().cloned().unwrap_or(Value::Undefined);
    let options = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if !crate::value::is_object(&inputs) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.zipKeyed iterables",
        ));
    }
    let (mode, padding_option) = zip_mode(&options)?;
    let (keys, iterators) = collect_zip_keyed_iterators(inputs)?;
    let padding = if let Some(padding) = padding_option {
        collect_zip_keyed_padding(padding, &keys)
            .map_err(|error| close_zip_iterators(iterators.clone(), error))?
    } else {
        Vec::new()
    };
    Ok(Value::Iterator(Rc::new(IteratorData::new(
        IteratorState::Zip {
            iterators,
            padding,
            mode,
            keys: Some(keys),
            started: false,
            done: false,
        },
    ))))
}

pub(crate) fn dispose(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    if crate::builtins::same_value(
        Some(receiver),
        Some(&crate::vm::realm_intrinsic(
            crate::ops::Builtin::IteratorPrototype,
        )),
    ) {
        return Ok(Value::Undefined);
    }
    let method = crate::execute::get_property_result(receiver, "return")?;
    if !matches!(method, Value::Null | Value::Undefined) {
        if !crate::conversion::is_callable(&method) {
            return Err(crate::vm::not_callable());
        }
        let _ = call(&method, receiver)?;
    }
    Ok(Value::Undefined)
}

pub(crate) fn prototype_constructor_getter(
    _receiver: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    Ok(crate::vm::realm_intrinsic(crate::ops::Builtin::Iterator))
}

pub(crate) fn prototype_tag_getter(
    _receiver: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    Ok(Value::String("Iterator".to_string()))
}

pub(crate) fn prototype_setter(
    receiver: Option<&Value>,
    arguments: &[Value],
    key: &str,
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| {
            crate::value::error::throw_type_error("Iterator prototype setter requires an object")
        })?;
    if crate::builtins::same_value(
        Some(receiver),
        Some(&crate::vm::realm_intrinsic(
            crate::ops::Builtin::IteratorPrototype,
        )),
    ) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to Iterator.prototype intrinsic",
        ));
    }
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    crate::execute::set_property_in_place(receiver, key, value);
    Ok(Value::Undefined)
}

fn collect_zip_padding(
    padding: Value,
    count: usize,
) -> Result<Vec<Value>, crate::execute::VmError> {
    if matches!(padding, Value::Undefined) {
        return Ok(vec![Value::Undefined; count]);
    }
    let iterator = open(padding)?;
    let mut values = Vec::with_capacity(count);
    let mut exhausted = false;
    for _ in 0..count {
        match step_value(&iterator) {
            Ok(Some(value)) => values.push(value),
            Ok(None) => {
                exhausted = true;
                break;
            }
            Err(error) => return Err(error),
        }
    }
    if !exhausted {
        let completion = close(iterator, crate::completion::Completion::Normal)?;
        if !matches!(completion, crate::completion::Completion::Normal) {
            return completion.into_vm_error().map(|_| values);
        }
    }
    values.resize(count, Value::Undefined);
    Ok(values)
}

include!("iterator_combinators.rs");

fn zip_iterators(data: &IteratorData) -> Option<Vec<Value>> {
    match &*data.state.borrow() {
        IteratorState::Zip { iterators, .. } => Some(iterators.clone()),
        _ => None,
    }
}

pub(crate) fn close_iterators(
    iterators: Vec<Value>,
    mut completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    for iterator in iterators.into_iter().rev() {
        match close(iterator, completion.clone()) {
            Ok(next) => completion = next,
            Err(error) if !matches!(completion, crate::completion::Completion::Throw(_)) => {
                completion = crate::completion::Completion::from_vm_error(error)?;
            }
            Err(_) => {}
        }
    }
    Ok(completion)
}
fn make_protocol(iterator: Value) -> Value {
    make_protocol_with_next_mode(iterator, Value::Undefined, false)
}
fn make_protocol_with_next(iterator: Value, next: Value) -> Value {
    make_protocol_with_next_mode(iterator, next, false)
}
fn make_protocol_async(iterator: Value) -> Value {
    make_protocol_with_next_mode(iterator, Value::Undefined, true)
}
fn make_protocol_with_next_mode(iterator: Value, next: Value, await_value: bool) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Protocol {
        iterator,
        next,
        done: false,
        await_value,
    })))
}
pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &crate::ops::Op,
) -> Result<(), crate::execute::VmError> {
    use crate::ops::Op;
    match op {
        Op::RequireObjectCoercible { src } => require_object_coercible(read(registers, *src)?)?,
        Op::GetIterator { dst, iterable } => {
            let iterator = open(read(registers, *iterable)?)?;
            crate::execute::write_value(registers, *dst, iterator);
        }
        Op::IteratorStep { dst, iterator } => {
            let value = step(read(registers, *iterator)?)?;
            crate::execute::write_value(registers, *dst, value);
        }
        Op::IteratorRest { dst, iterator } => {
            let value = rest(read(registers, *iterator)?)?;
            crate::execute::write_value(registers, *dst, value);
        }
        _ => return Err(crate::execute::VmError::MissingReturn),
    }
    Ok(())
}
include!("iterator_binding.rs");
pub(crate) fn close(
    record: Value,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    if !matches!(record, Value::Iterator(_)) {
        return close_user_iter(record, completion);
    }
    let Some(iterator) = close_target(&record)? else {
        return Ok(completion);
    };
    let method = match get_return_method(&iterator) {
        Ok(method) => method,
        Err(error) => return close_error(completion, error),
    };
    let Some(method) = method else {
        return Ok(completion);
    };
    let result = call(&method, &iterator);
    finish_close(completion, result)
}

fn close_user_iter(
    value: Value,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let method = match crate::execute::get_property_result(&value, "return") {
        Ok(method) => method,
        Err(_) => return Ok(completion),
    };
    if matches!(method, Value::Null | Value::Undefined) {
        return Ok(completion);
    }
    if !crate::conversion::is_callable(&method) {
        return Ok(completion);
    }
    let result = call(&method, &value);
    finish_close(completion, result)
}
fn close_target(record: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Iterator(data) = record else {
        return Err(not_iterable());
    };
    let state = data.state.borrow();
    match &*state {
        IteratorState::Native { done: true, .. }
        | IteratorState::String { done: true, .. }
        | IteratorState::Set { done: true, .. }
        | IteratorState::Map { done: true, .. }
        | IteratorState::Protocol { done: true, .. }
        | IteratorState::RegExpString { done: true, .. }
        | IteratorState::Native { .. } => Ok(None),
        IteratorState::String { .. } => Ok(None),
        IteratorState::Set { .. } => Ok(None),
        IteratorState::Map { .. } => Ok(None),
        IteratorState::RegExpString { .. } => Ok(None),
        IteratorState::Protocol { iterator, .. } => Ok(Some(iterator.clone())),
        IteratorState::Mapped { iterator, .. } => Ok(Some(iterator.clone())),
        IteratorState::Filtered { iterator, .. } => Ok(Some(iterator.clone())),
        IteratorState::FlatMapped { inner, .. } => Ok(Some(inner.clone())),
        IteratorState::Dropped { inner, .. } => Ok(Some(inner.clone())),
        IteratorState::Take { inner, .. } => Ok(Some(inner.clone())),
        IteratorState::Concat { current, done, .. } if !*done => Ok(current.clone()),
        IteratorState::Concat { .. } => Ok(None),
        IteratorState::Zip { .. } => Ok(None),
    }
}
fn get_return_method(iterator: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let method = crate::execute::get_property_result(iterator, "return")?;
    if matches!(method, Value::Null | Value::Undefined) {
        return Ok(None);
    }
    if !crate::conversion::is_callable(&method) {
        return Err(crate::vm::not_callable());
    }
    Ok(Some(method))
}
fn finish_close(
    completion: crate::completion::Completion,
    result: Result<Value, crate::execute::VmError>,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let result = match result {
        Ok(result) => result,
        Err(error) => return close_error(completion, error),
    };
    if matches!(completion, crate::completion::Completion::Throw(_)) {
        return Ok(completion);
    }
    if !crate::value::is_object(&result) {
        return close_error(completion, close_result_error());
    }
    Ok(completion)
}
fn close_error(
    completion: crate::completion::Completion,
    error: crate::execute::VmError,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    if matches!(completion, crate::completion::Completion::Throw(_)) {
        return Ok(completion);
    }
    match error {
        crate::execute::VmError::Thrown(value) => Ok(crate::completion::Completion::Throw(value)),
        error => Err(error),
    }
}
fn close_result_error() -> crate::execute::VmError {
    crate::value::error::throw_type_error("iterator return result is not an object")
}
fn read(
    registers: &crate::register_file::RegisterFile,
    index: u16,
) -> Result<Value, crate::execute::VmError> {
    crate::execute::read_register(registers, index)
}
fn require_object_coercible(value: Value) -> Result<(), crate::execute::VmError> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Cannot destructure null or undefined",
        ));
    }
    Ok(())
}
pub(crate) fn open(value: Value) -> Result<Value, crate::execute::VmError> {
    if matches!(value, Value::Iterator(_)) {
        return Ok(value);
    }
    let method = crate::execute::get_property_result(&value, "Symbol.iterator")?;
    if matches!(method, Value::Undefined) {
        if matches!(value, Value::Generator(_)) {
            return open_self_iterator(value);
        }
        if matches!(value, Value::Array(_))
            && crate::builtins::builtin_prototype_property_is_removed(
                crate::ops::Builtin::ArrayPrototype,
                "Symbol.iterator",
            )
        {
            return Err(not_iterable());
        }
        return iterable_values(value).map(make);
    }
    let iterator = call(&method, &value)?;
    if !crate::value::is_object(&iterator) {
        return Err(not_iterable());
    }
    if matches!(iterator, Value::Iterator(_)) {
        if let Value::Iterator(data) = &iterator {
            if matches!(&*data.state.borrow(), IteratorState::Native { .. }) {
                if let Some(next) = crate::vm::intrinsic_override_property(
                    crate::ops::Builtin::ArrayIteratorPrototype,
                    "next",
                    &iterator,
                ) {
                    return Ok(make_protocol_with_next(
                        iterator.clone(),
                        crate::vm::bind_method(&iterator, next),
                    ));
                }
            }
        }
        return Ok(iterator);
    }
    let next = crate::execute::get_property_result(&iterator, "next")?;
    Ok(make_protocol_with_next(iterator, next))
}

pub(crate) fn open_with_method(
    value: &Value,
    method: Value,
) -> Result<Value, crate::execute::VmError> {
    let iterator = call(&method, value)?;
    if !crate::value::is_object(&iterator) {
        return Err(not_iterable());
    }
    if matches!(iterator, Value::Iterator(_)) {
        return Ok(iterator);
    }
    let next = crate::execute::get_property_result(&iterator, "next")?;
    Ok(make_protocol_with_next(iterator, next))
}

pub(crate) fn open_async(value: Value) -> Result<Value, crate::execute::VmError> {
    let method = crate::execute::get_property_result(&value, "Symbol.asyncIterator")?;
    if !matches!(method, Value::Undefined) {
        let iterator = call(&method, &value)?;
        if !crate::value::is_object(&iterator) {
            return Err(not_iterable());
        }
        return Ok(if matches!(iterator, Value::Iterator(_)) {
            iterator
        } else {
            make_protocol_async(iterator)
        });
    }
    Ok(make_protocol_async(open(value)?))
}
pub(crate) fn open_self_iterator(iterator: Value) -> Result<Value, crate::execute::VmError> {
    Ok(make_protocol(iterator))
}
fn step(value: Value) -> Result<Value, crate::execute::VmError> {
    match step_value(&value) {
        Ok(value) => Ok(value.unwrap_or(Value::Undefined)),
        Err(error) => {
            if let Value::Iterator(data) = &value {
                mark_done(data);
            }
            Err(error)
        }
    }
}

fn rest(value: Value) -> Result<Value, crate::execute::VmError> {
    collect_rest(&value).map(Value::array)
}

fn collect_rest(value: &Value) -> Result<Vec<Value>, crate::execute::VmError> {
    let mut values = Vec::new();
    loop {
        match step_value(value) {
            Ok(Some(value)) => values.push(value),
            Ok(None) => return Ok(values),
            Err(error) => {
                if let Value::Iterator(data) = value {
                    mark_done(data);
                }
                return Err(error);
            }
        }
    }
}

pub(crate) fn collect(value: &Value) -> Result<Vec<Value>, crate::execute::VmError> {
    let mut values = Vec::new();
    loop {
        match step_value(value) {
            Ok(Some(value)) => values.push(value),
            Ok(None) => return Ok(values),
            Err(error) => {
                if let Value::Iterator(data) = value {
                    if let crate::execute::VmError::Thrown(reason) = &error {
                        let _ = close(
                            value.clone(),
                            crate::completion::Completion::Throw(reason.clone()),
                        );
                    }
                    mark_done(data);
                }
                return Err(error);
            }
        }
    }
}

pub fn collect_iterable(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    let iterator = open(value)?;
    collect(&iterator)
}

pub(crate) fn collect_with_method(
    value: &Value,
    method: Value,
) -> Result<Vec<Value>, crate::execute::VmError> {
    let iterator = open_with_method(value, method)?;
    collect(&iterator)
}

pub(crate) fn for_each_iterable<F>(value: Value, callback: F) -> Result<(), crate::execute::VmError>
where
    F: FnMut(Value) -> Result<(), crate::execute::VmError>,
{
    let iterator = open(value)?;
    for_each_open_iterator(iterator, callback)
}

pub(crate) fn for_each_open_iterator<F>(
    iterator: Value,
    mut callback: F,
) -> Result<(), crate::execute::VmError>
where
    F: FnMut(Value) -> Result<(), crate::execute::VmError>,
{
    loop {
        let item = match step_value(&iterator) {
            Ok(Some(item)) => item,
            Ok(None) => return Ok(()),
            Err(error) => {
                if let crate::execute::VmError::Thrown(reason) = &error {
                    let _ = close(
                        iterator.clone(),
                        crate::completion::Completion::Throw(reason.clone()),
                    );
                }
                return Err(error);
            }
        };
        if let Err(error) = callback(item) {
            if let crate::execute::VmError::Thrown(reason) = &error {
                let _ = close(
                    iterator.clone(),
                    crate::completion::Completion::Throw(reason.clone()),
                );
            }
            return Err(error);
        }
    }
}
pub enum DelegationResult {
    Done(Value),
    Ongoing { value: Value, passthrough: bool },
}
include!("iterator_delegation.rs");

fn get_method(iterator: &Value, name: &str) -> Result<Option<Value>, crate::execute::VmError> {
    let method = crate::execute::get_property_result(iterator, name)?;
    if matches!(method, Value::Null | Value::Undefined) {
        return Ok(None);
    }
    if !crate::conversion::is_callable(&method) {
        return Err(crate::vm::not_callable());
    }
    Ok(Some(method))
}
fn missing_throw_method() -> crate::execute::VmError {
    crate::value::error::throw_type_error("delegated iterator has no throw method")
}
pub(super) fn native_step(
    values: &[Value],
    receiver: Option<&Rc<crate::value::ArrayData>>,
    typed_receiver: Option<&Value>,
    typed_keys: bool,
    entries: bool,
    keys: bool,
    index: &mut usize,
    done: &mut bool,
) -> Result<Option<Value>, crate::execute::VmError> {
    if *done {
        return Ok(None);
    }
    let value = if let Some(value) = typed_receiver {
        if typed_keys {
            let length = iterator_typed::typed_length(value)?;
            (*index < length).then_some(Value::Number(*index as f64))
        } else {
            let detached = typed_receiver_is_detached(value);
            if detached {
                return Err(crate::value::error::throw_type_error(
                    "Array iterator called on detached TypedArray",
                ));
            }
            let values = iterator_typed::typed_values(value.clone())?;
            values.get(*index).cloned()
        }
    } else if let Some(data) = receiver {
        array_receiver_step(data, *index)?
    } else {
        values.get(*index).cloned()
    };
    let current_index = *index;
    *index = index.saturating_add(1);
    *done = value.is_none();
    Ok(value.map(|value| {
        if keys {
            Value::Number(current_index as f64)
        } else if entries {
            Value::array(vec![Value::Number(current_index as f64), value])
        } else {
            value
        }
    }))
}

fn typed_receiver_is_detached(value: &Value) -> bool {
    let buffer = match value {
        Value::Float64Array(data) => &data.buffer,
        Value::Float32Array(data) => &data.buffer,
        Value::Int8Array(data) => &data.buffer,
        Value::Int16Array(data) => &data.buffer,
        Value::Int32Array(data) => &data.buffer,
        Value::Uint8Array(data) => &data.buffer,
        Value::Uint8ClampedArray(data) => &data.buffer,
        Value::Uint16Array(data) => &data.buffer,
        Value::Uint32Array(data) => &data.buffer,
        Value::BigInt64Array(data) => &data.buffer,
        Value::BigUint64Array(data) => &data.buffer,
        _ => return false,
    };
    *buffer.detached.borrow()
}

fn array_receiver_step(
    data: &Rc<crate::value::ArrayData>,
    index: usize,
) -> Result<Option<Value>, crate::execute::VmError> {
    let receiver = crate::locals::resolved_replacement(Value::Array(Rc::clone(data)));
    let Value::Array(data) = receiver else {
        return Ok(None);
    };
    if index >= data.logical_len() {
        return Ok(None);
    }
    crate::execute::get_property_result(&Value::Array(data), &index.to_string()).map(Some)
}
pub(super) fn mark_done(data: &IteratorData) {
    match &mut *data.state.borrow_mut() {
        IteratorState::Native { done, .. }
        | IteratorState::String { done, .. }
        | IteratorState::Set { done, .. }
        | IteratorState::Map { done, .. }
        | IteratorState::Protocol { done, .. }
        | IteratorState::RegExpString { done, .. } => *done = true,
        IteratorState::Mapped { done, .. } => *done = true,
        IteratorState::Filtered { done, .. } => *done = true,
        IteratorState::FlatMapped { done, .. } => *done = true,
        IteratorState::Dropped { done, .. } => *done = true,
        IteratorState::Take { remaining, .. } => *remaining = u64::MAX,
        IteratorState::Concat { done, .. } => *done = true,
        IteratorState::Zip { done, .. } => *done = true,
    }
}
fn call(callee: &Value, receiver: &Value) -> Result<Value, crate::execute::VmError> {
    call_with_arguments(callee, receiver, &[])
}
fn call_with_arguments(
    callee: &Value,
    receiver: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match callee {
        Value::Proxy(_) => crate::proxy::proxy_apply(callee, receiver, arguments),
        _ => crate::functions::execute_target(callee, receiver, arguments),
    }
}
fn iterable_values(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    match value {
        Value::Array(values) => Ok(values.snapshot()),
        Value::String(value) if !crate::conversion::is_symbol_string(&value) => Ok(value
            .chars()
            .map(|c| Value::String(c.to_string()))
            .collect()),
        value => iterator_typed::typed_values(value),
    }
}
pub(super) fn not_iterable() -> crate::execute::VmError {
    crate::value::error::throw_type_error("value is not iterable")
}
