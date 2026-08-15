use crate::value::{IteratorData, IteratorState, Value};
use std::{cell::RefCell, rc::Rc};
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
pub(crate) use iterator_values::{
    from_map, from_map_keys, from_map_values, from_set, from_set_entries, make, make_array,
    make_regexp_string, make_string, make_typed, make_typed_keys, next, next_map, next_set,
    property_for, prototype_of,
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
    Ok(Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Concat {
            items,
            index: 0,
            current: None,
            done: false,
        }),
    })))
}

pub(crate) fn zip(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let inputs = arguments.first().cloned().unwrap_or(Value::Undefined);
    let options = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let mode = zip_mode(&options)?;
    let values = array_values(&inputs)?;
    let iterators = values
        .into_iter()
        .map(|value| from(std::slice::from_ref(&value)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Zip {
            iterators,
            mode,
            done: false,
        }),
    })))
}

fn zip_mode(options: &Value) -> Result<u8, crate::execute::VmError> {
    if matches!(options, Value::Undefined) {
        return Ok(0);
    }
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.zip options",
        ));
    }
    let mode = crate::execute::get_property_result(options, "mode")?;
    match mode {
        Value::Undefined => Ok(0),
        Value::String(value) if value == "shortest" => Ok(0),
        Value::String(value) if value == "longest" => Ok(1),
        Value::String(value) if value == "strict" => Ok(2),
        _ => Err(crate::value::error::throw_type_error("Iterator.zip mode")),
    }
}

fn array_values(value: &Value) -> Result<Vec<Value>, crate::execute::VmError> {
    let Value::Array(array) = value else {
        return Err(crate::value::error::throw_type_error("Iterator.zip inputs"));
    };
    Ok((0..array.logical_len())
        .map(|index| array.get_index(index).unwrap_or(Value::Undefined))
        .collect())
}

pub(crate) fn from(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    if matches!(value, Value::Iterator(_) | Value::Generator(_)) {
        return Ok(value);
    }
    if !crate::value::is_object(&value) {
        return open(value);
    }
    let method = crate::execute::get_property_result(&value, "Symbol.iterator")?;
    if matches!(method, Value::Null | Value::Undefined) {
        let next = crate::execute::get_property_result(&value, "next")?;
        if !crate::conversion::is_callable(&next) {
            return Err(crate::value::error::throw_type_error(
                "Iterator.from requires an iterable or iterator",
            ));
        }
        return Ok(make_protocol(value));
    }
    open(value)
}

pub(crate) fn to_array(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let iterator = if matches!(receiver, Value::Generator(_)) {
        open(receiver.clone())?
    } else {
        from(std::slice::from_ref(receiver))?
    };
    Ok(Value::array(collect_rest(&iterator)?))
}

pub(crate) fn map(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let receiver = receiver.ok_or_else(not_iterable)?;
    let mapper = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&mapper) {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.map mapper is not callable",
        ));
    }
    let iterator = from(std::slice::from_ref(receiver))?;
    Ok(Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Mapped {
            iterator,
            mapper,
            index: 0,
            done: false,
        }),
    })))
}

pub(crate) fn next_string(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let branded = matches!(receiver, Some(Value::Iterator(data)) if matches!(
        &*data.state.borrow(), IteratorState::String { .. }
    ));
    if !branded {
        return Err(crate::value::error::throw_type_error(
            "String iterator called on incompatible receiver",
        ));
    }
    next(receiver)
}
fn make_protocol(iterator: Value) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Protocol {
            iterator,
            next: Value::Undefined,
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
include!("iterator_binding.rs");
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
    if matches!(iterator, Value::Iterator(_)) {
        return Ok(iterator);
    }
    Ok(make_protocol(iterator))
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
                receiver,
                typed_receiver,
                typed_keys,
                index,
                done,
            } => {
                return native_delegation_step(
                    values,
                    receiver.as_ref(),
                    typed_receiver.as_ref(),
                    *typed_keys,
                    index,
                    done,
                );
            }
            IteratorState::Set { .. }
            | IteratorState::Map { .. }
            | IteratorState::String { .. }
            | IteratorState::RegExpString { .. } => {
                return Ok(DelegationResult::Done(Value::Undefined));
            }
            IteratorState::Protocol { iterator, done, .. } if !*done => Some(iterator.clone()),
            IteratorState::Protocol { .. } => None,
            IteratorState::Mapped { .. } => None,
            IteratorState::Concat { .. } => None,
            IteratorState::Zip { .. } => None,
        }
    };
    let Some(iterator) = protocol else {
        return Ok(DelegationResult::Done(Value::Undefined));
    };
    let next = crate::execute::get_property_result(&iterator, "next")?;
    if !crate::conversion::is_callable(&next) {
        return Err(not_iterable());
    }
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
    receiver: Option<&Rc<crate::value::ArrayData>>,
    typed_receiver: Option<&Value>,
    typed_keys: bool,
    index: &mut usize,
    done: &mut bool,
) -> Result<DelegationResult, crate::execute::VmError> {
    let value = native_step(values, receiver, typed_receiver, typed_keys, index, done)?
        .unwrap_or(Value::Undefined);
    if *done {
        Ok(DelegationResult::Done(value))
    } else {
        Ok(DelegationResult::Ongoing {
            value,
            passthrough: false,
        })
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
        IteratorState::Native { .. }
        | IteratorState::Set { .. }
        | IteratorState::Map { .. }
        | IteratorState::String { .. }
        | IteratorState::RegExpString { .. } => None,
        IteratorState::Mapped { .. } => None,
        IteratorState::Concat { .. } => None,
        IteratorState::Zip { .. } => None,
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
pub(super) fn native_step(
    values: &[Value],
    receiver: Option<&Rc<crate::value::ArrayData>>,
    typed_receiver: Option<&Value>,
    typed_keys: bool,
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
            let values = iterator_typed::typed_values(value.clone())?;
            values.get(*index).cloned()
        }
    } else if let Some(data) = receiver {
        array_receiver_step(data, *index)?
    } else {
        values.get(*index).cloned()
    };
    *index = index.saturating_add(1);
    *done = value.is_none();
    Ok(value)
}

fn array_receiver_step(
    data: &Rc<crate::value::ArrayData>,
    index: usize,
) -> Result<Option<Value>, crate::execute::VmError> {
    if index >= data.logical_len() {
        return Ok(None);
    }
    let array = Value::Array(Rc::clone(data));
    crate::execute::get_property_result(&array, &index.to_string()).map(Some)
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
