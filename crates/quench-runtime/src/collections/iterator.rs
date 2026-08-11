use crate::value::{IteratorData, IteratorState, Value};
use std::{cell::RefCell, rc::Rc};

#[path = "iterator_map.rs"]
mod iterator_map;
#[path = "iterator_typed.rs"]
mod iterator_typed;
#[path = "iterator_values.rs"]
mod iterator_values;
pub(crate) use iterator_values::{
    from_map, from_map_keys, from_map_values, from_set, make, next, property,
};

fn make_protocol(iterator: Value, next: Value) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Protocol {
            iterator,
            next,
            done: false,
        }),
    }))
}

pub(crate) fn execute(
    registers: &mut Vec<Value>,
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
pub(crate) fn execute_binding(
    registers: &mut Vec<Value>,
    op: &crate::ops::Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let crate::ops::Op::IteratorBinding { iterator, body } = op else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let iterator = read(registers, *iterator)?;
    let completion = crate::execute::execute_completion_in_place(body, registers)?;
    close(iterator, completion)
}
pub(crate) fn close(
    record: Value,
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
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

fn close_target(record: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Iterator(data) = record else {
        return Err(not_iterable());
    };
    let state = data.state.borrow();
    match &*state {
        IteratorState::Native { done: true, .. }
        | IteratorState::Set { done: true, .. }
        | IteratorState::Map { done: true, .. }
        | IteratorState::Protocol { done: true, .. }
        | IteratorState::Native { .. } => Ok(None),
        IteratorState::Set { .. } => Ok(None),
        IteratorState::Map { .. } => Ok(None),
        IteratorState::Protocol {
            iterator: Value::Generator(_),
            ..
        } => Ok(None),
        IteratorState::Protocol { iterator, .. } => Ok(Some(iterator.clone())),
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
fn read(registers: &[Value], index: u16) -> Result<Value, crate::execute::VmError> {
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
    let next = crate::execute::get_property_result(&iterator, "next")?;
    if !crate::conversion::is_callable(&next) {
        return Err(not_iterable());
    }
    Ok(make_protocol(iterator, next))
}

fn open_self_iterator(iterator: Value) -> Result<Value, crate::execute::VmError> {
    let next = crate::execute::get_property_result(&iterator, "next")?;
    if !crate::conversion::is_callable(&next) {
        return Err(not_iterable());
    }
    Ok(make_protocol(iterator, next))
}

pub(crate) fn collect_iterator_object(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    collect(&open_self_iterator(value)?)
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
    collect_iterable(value).map(Value::array)
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

pub(crate) fn collect_iterable(value: Value) -> Result<Vec<Value>, crate::execute::VmError> {
    let iterator = open(value)?;
    collect(&iterator)
}

pub(crate) fn for_each_iterable<F>(
    value: Value,
    mut callback: F,
) -> Result<(), crate::execute::VmError>
where
    F: FnMut(Value) -> Result<(), crate::execute::VmError>,
{
    let iterator = open(value)?;
    loop {
        let Some(item) = step_value(&iterator)? else {
            return Ok(());
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
pub fn delegate_start(value: Value) -> Result<Value, crate::execute::VmError> {
    open(value)
}
pub fn delegate_next(
    record: &Value,
    input: Value,
) -> Result<DelegationResult, crate::execute::VmError> {
    let Value::Iterator(data) = record else {
        return Err(not_iterable());
    };
    let protocol = {
        let mut state = data.state.borrow_mut();
        match &mut *state {
            IteratorState::Native {
                values,
                index,
                done,
            } => {
                return Ok(native_delegation_step(values, index, done));
            }
            IteratorState::Set { .. } | IteratorState::Map { .. } => {
                return Ok(DelegationResult::Done(Value::Undefined));
            }
            IteratorState::Protocol {
                iterator,
                next,
                done,
            } if !*done => Some((iterator.clone(), next.clone())),
            IteratorState::Protocol { .. } => None,
        }
    };
    let Some((iterator, next)) = protocol else {
        return Ok(DelegationResult::Done(Value::Undefined));
    };
    let result = call_with_arguments(&next, &iterator, std::slice::from_ref(&input))?;
    delegation_result(data, result)
}
pub fn delegate_return(
    record: &Value,
    input: Value,
) -> Result<DelegationResult, crate::execute::VmError> {
    let Some((data, iterator)) = delegation_target(record)? else {
        return Ok(DelegationResult::Done(input));
    };
    let Some(method) = get_return_method(&iterator)? else {
        mark_done(data);
        return Ok(DelegationResult::Done(input));
    };
    let result = call_with_arguments(&method, &iterator, std::slice::from_ref(&input))?;
    delegation_result(data, result)
}
pub fn delegate_throw(
    record: &Value,
    input: Value,
) -> Result<DelegationResult, crate::execute::VmError> {
    let Some((data, iterator)) = delegation_target(record)? else {
        return Err(missing_throw_method());
    };
    let Some(method) = get_method(&iterator, "throw")? else {
        close_after_missing_throw(data, &iterator)?;
        return Err(missing_throw_method());
    };
    let result = call_with_arguments(&method, &iterator, std::slice::from_ref(&input))?;
    delegation_result(data, result)
}
fn close_after_missing_throw(
    data: &IteratorData,
    iterator: &Value,
) -> Result<(), crate::execute::VmError> {
    if let Some(method) = get_return_method(iterator)? {
        let _ = call_with_arguments(&method, iterator, &[])?;
    }
    mark_done(data);
    Ok(())
}
fn native_delegation_step(
    values: &[Value],
    index: &mut usize,
    done: &mut bool,
) -> DelegationResult {
    let value = native_step(values, index, done).unwrap_or(Value::Undefined);
    if *done {
        DelegationResult::Done(value)
    } else {
        DelegationResult::Ongoing {
            value,
            passthrough: false,
        }
    }
}
fn delegation_target(
    record: &Value,
) -> Result<Option<(&IteratorData, Value)>, crate::execute::VmError> {
    let Value::Iterator(data) = record else {
        return Err(not_iterable());
    };
    let iterator = match &*data.state.borrow() {
        IteratorState::Protocol { iterator, .. } => Some(iterator.clone()),
        IteratorState::Native { .. } | IteratorState::Set { .. } | IteratorState::Map { .. } => {
            None
        }
    };
    Ok(iterator.map(|iterator| (data.as_ref(), iterator)))
}
fn delegation_result(
    data: &IteratorData,
    result: Value,
) -> Result<DelegationResult, crate::execute::VmError> {
    if !crate::value::is_object(&result) {
        return Err(not_iterable());
    }
    let done = crate::execute::is_truthy(&crate::execute::get_property_result(&result, "done")?);
    if !done {
        return Ok(DelegationResult::Ongoing {
            value: result,
            passthrough: true,
        });
    }
    let value = crate::execute::get_property_result(&result, "value")?;
    mark_done(data);
    Ok(DelegationResult::Done(value))
}
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
pub(crate) fn step_value(value: &Value) -> Result<Option<Value>, crate::execute::VmError> {
    let Value::Iterator(data) = value else {
        return Err(not_iterable());
    };
    let protocol = {
        let mut state = data.state.borrow_mut();
        match &mut *state {
            IteratorState::Native {
                values,
                index,
                done,
            } => return Ok(native_step(values, index, done)),
            IteratorState::Set { data, index, done } => {
                return Ok(set_step(data, index, done));
            }
            IteratorState::Map {
                data,
                index,
                kind,
                done,
            } => return Ok(iterator_map::step(data, index, kind, done)),
            IteratorState::Protocol {
                iterator,
                next,
                done,
            } if !*done => Some((iterator.clone(), next.clone())),
            IteratorState::Protocol { .. } => None,
        }
    };
    let Some((iterator, next)) = protocol else {
        return Ok(None);
    };
    let result = call(&next, &iterator)?;
    protocol_result(data, result)
}
fn native_step(values: &[Value], index: &mut usize, done: &mut bool) -> Option<Value> {
    if *done {
        return None;
    }
    let value = values.get(*index).cloned();
    *index = index.saturating_add(1);
    *done = value.is_none();
    value
}
fn set_step(data: &crate::value::SetData, index: &mut usize, done: &mut bool) -> Option<Value> {
    if *done {
        return None;
    }
    let value = data.values.borrow().get(*index).cloned();
    if value.is_none() {
        *done = true;
    } else {
        *index += 1;
    }
    value
}
fn protocol_result(
    data: &IteratorData,
    result: Value,
) -> Result<Option<Value>, crate::execute::VmError> {
    if !crate::value::is_object(&result) {
        return Err(not_iterable());
    }
    let done = crate::execute::get_property_result(&result, "done")?;
    if crate::execute::is_truthy(&done) {
        mark_done(data);
        return Ok(None);
    }
    crate::execute::get_property_result(&result, "value").map(Some)
}
fn mark_done(data: &IteratorData) {
    match &mut *data.state.borrow_mut() {
        IteratorState::Native { done, .. }
        | IteratorState::Set { done, .. }
        | IteratorState::Map { done, .. }
        | IteratorState::Protocol { done, .. } => *done = true,
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
