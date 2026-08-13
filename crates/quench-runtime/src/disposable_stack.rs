//! Explicit resource-management stack intrinsics.

use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{ArrayData, ObjectData, PromiseData, PromiseState, Value},
};

const STATE: &str = "\0disposable:disposed";
const RECORDS: &str = "\0disposable:records";
const ASYNC: &str = "\0disposable:async";

pub(crate) fn construct() -> Result<Value, VmError> {
    Ok(stack(Value::Array(Rc::new(ArrayData::new(Vec::new())))))
}

pub(crate) fn construct_async() -> Result<Value, VmError> {
    Ok(stack_with(
        Value::Builtin(Builtin::AsyncDisposableStackPrototype),
        true,
        Value::Array(Rc::new(ArrayData::new(Vec::new()))),
    ))
}

pub(crate) fn execute(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    use Builtin::*;
    Some(match builtin {
        DisposableStack => Err(requires_new()),
        DisposableStackUse => use_resource(receiver, arguments),
        DisposableStackAdopt => adopt_resource(receiver, arguments),
        DisposableStackDefer => defer_resource(receiver, arguments),
        DisposableStackMove => move_stack(receiver),
        DisposableStackDispose => dispose_stack(receiver),
        DisposableStackDisposed => disposed(receiver),
        AsyncDisposableStack => Err(requires_new()),
        AsyncDisposableStackUse => async_use(receiver, arguments),
        AsyncDisposableStackAdopt => async_adopt(receiver, arguments),
        AsyncDisposableStackDefer => async_defer(receiver, arguments),
        AsyncDisposableStackMove => async_move(receiver),
        AsyncDisposableStackDisposeAsync => async_dispose(receiver),
        AsyncDisposableStackDisposed => async_disposed(receiver),
        _ => return None,
    })
}

pub(crate) fn accessor(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    (key == "disposed" && is_stack(value)).then(|| {
        if is_async(value) {
            async_disposed(Some(receiver))
        } else {
            disposed(Some(receiver))
        }
    })
}

fn stack(records: Value) -> Value {
    stack_with(
        Value::Builtin(Builtin::DisposableStackPrototype),
        false,
        records,
    )
}

fn stack_with(prototype: Value, async_: bool, records: Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("\0prototype".into(), prototype),
        (STATE.into(), cell(Value::Boolean(false))),
        (RECORDS.into(), cell(records)),
        (ASYNC.into(), Value::Boolean(async_)),
    ])))
}

fn cell(value: Value) -> Value {
    Value::BindingCell(Rc::new(RefCell::new(value)))
}

fn use_resource(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let resource = arguments.first().cloned().unwrap_or(Value::Undefined);
    let records = active_records(receiver)?;
    if matches!(resource, Value::Null | Value::Undefined) {
        return Ok(resource);
    }
    if !crate::value::is_object(&resource) {
        return Err(crate::vm::not_callable());
    }
    let method = crate::execute::get_property_result(&resource, "Symbol.dispose")?;
    if matches!(method, Value::Null | Value::Undefined) {
        return Err(crate::vm::not_callable());
    }
    ensure_callable(&method)?;
    push_record(&records, "use", resource.clone(), method);
    Ok(resource)
}

fn adopt_resource(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let resource = arguments.first().cloned().unwrap_or(Value::Undefined);
    let method = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    ensure_callable(&method)?;
    push_record(
        &active_records(receiver)?,
        "adopt",
        resource.clone(),
        method,
    );
    Ok(resource)
}

fn defer_resource(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let method = arguments.first().cloned().unwrap_or(Value::Undefined);
    ensure_callable(&method)?;
    push_record(
        &active_records(receiver)?,
        "defer",
        Value::Undefined,
        method,
    );
    Ok(Value::Undefined)
}

fn async_use(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    async_receiver(receiver)?;
    let resource = arguments.first().cloned().unwrap_or(Value::Undefined);
    let records = active_records(receiver)?;
    if matches!(resource, Value::Null | Value::Undefined) {
        return Ok(resource);
    }
    if !crate::value::is_object(&resource) {
        return Err(crate::vm::not_callable());
    }
    let method = async_dispose_method(&resource)?;
    ensure_callable(&method)?;
    push_record(&records, "use", resource.clone(), method);
    Ok(resource)
}

fn async_dispose_method(resource: &Value) -> Result<Value, VmError> {
    let async_method = crate::execute::get_property_result(resource, "Symbol.asyncDispose")?;
    if !matches!(async_method, Value::Null | Value::Undefined) {
        return Ok(async_method);
    }
    let method = crate::execute::get_property_result(resource, "Symbol.dispose")?;
    if matches!(method, Value::Null | Value::Undefined) {
        Err(crate::vm::not_callable())
    } else {
        Ok(method)
    }
}

fn async_adopt(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    async_receiver(receiver)?;
    adopt_resource(receiver, arguments)
}

fn async_defer(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    async_receiver(receiver)?;
    defer_resource(receiver, arguments)
}

fn async_move(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = async_receiver(receiver)?;
    let records = active_records(Some(receiver))?;
    set_disposed(receiver, true)?;
    let moved = records.borrow().clone();
    *records.borrow_mut() = Value::Array(Rc::new(ArrayData::new(Vec::new())));
    Ok(stack_with(
        Value::Builtin(Builtin::AsyncDisposableStackPrototype),
        true,
        moved,
    ))
}

fn async_dispose(receiver: Option<&Value>) -> Result<Value, VmError> {
    let result = async_receiver(receiver).and_then(|receiver| dispose_stack(Some(receiver)));
    Ok(promise(result))
}

fn async_disposed(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = async_receiver(receiver)?;
    disposed(Some(receiver))
}

fn promise(result: Result<Value, VmError>) -> Value {
    let state = match result {
        Ok(value) => PromiseState::Fulfilled(value),
        Err(VmError::Thrown(value)) => PromiseState::Rejected(value),
        Err(_) => PromiseState::Rejected(Value::Undefined),
    };
    Value::Promise(Rc::new(PromiseData::new(state)))
}

fn move_stack(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(incompatible_receiver)?;
    let records = active_records(Some(receiver))?;
    set_disposed(receiver, true)?;
    let moved = records.borrow().clone();
    *records.borrow_mut() = Value::Array(Rc::new(ArrayData::new(Vec::new())));
    Ok(stack(moved))
}

fn dispose_stack(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(incompatible_receiver)?;
    let state = state(receiver)?;
    if is_disposed(&state.borrow()) {
        return Ok(Value::Undefined);
    }
    let records = records(receiver)?;
    set_disposed(receiver, true)?;
    let values = take_records(&records)?;
    dispose_records(values)
}

fn dispose_records(mut values: Vec<Value>) -> Result<Value, VmError> {
    let mut error = None;
    while let Some(record) = values.pop() {
        if let Err(next) = dispose_record(&record) {
            error = Some(next);
        }
    }
    error.map_or(Ok(Value::Undefined), Err)
}

fn dispose_record(record: &Value) -> Result<Value, VmError> {
    let kind = record_field(record, "kind").ok_or_else(incompatible_receiver)?;
    let method = record_field(record, "method").ok_or_else(incompatible_receiver)?;
    let value = record_field(record, "value").unwrap_or(Value::Undefined);
    let arguments = (kind == Value::String("adopt".into())).then_some(value.clone());
    match arguments {
        Some(argument) => crate::functions::execute_target(&method, &Value::Undefined, &[argument]),
        None => crate::functions::execute_target(&method, &value, &[]),
    }
}

fn disposed(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(incompatible_receiver)?;
    Ok(Value::Boolean(is_disposed(&state(receiver)?.borrow())))
}

fn active_records(receiver: Option<&Value>) -> Result<Rc<RefCell<Value>>, VmError> {
    let receiver = receiver.ok_or_else(incompatible_receiver)?;
    if is_disposed(&state(receiver)?.borrow()) {
        return Err(crate::value::error::throw_reference_error(
            "DisposableStack is disposed",
        ));
    }
    records(receiver)
}

fn is_stack(value: &Value) -> bool {
    slot(value, STATE).is_some() && slot(value, RECORDS).is_some()
}

fn is_async(value: &Value) -> bool {
    matches!(slot_value(value, ASYNC), Some(Value::Boolean(true)))
}

fn async_receiver(receiver: Option<&Value>) -> Result<&Value, VmError> {
    let receiver = receiver.ok_or_else(incompatible_receiver)?;
    is_async(receiver)
        .then_some(receiver)
        .ok_or_else(incompatible_receiver)
}

fn state(value: &Value) -> Result<Rc<RefCell<Value>>, VmError> {
    slot(value, STATE).ok_or_else(incompatible_receiver)
}

fn records(value: &Value) -> Result<Rc<RefCell<Value>>, VmError> {
    slot(value, RECORDS).ok_or_else(incompatible_receiver)
}

fn slot(value: &Value, name: &str) -> Option<Rc<RefCell<Value>>> {
    let Value::Object(object) = value else {
        return None;
    };
    object
        .iter()
        .find_map(|(key, value)| (key == name).then(|| value))
        .and_then(|value| {
            if let Value::BindingCell(cell) = value {
                Some(Rc::clone(cell))
            } else {
                None
            }
        })
}

fn slot_value(value: &Value, name: &str) -> Option<Value> {
    let Value::Object(object) = value else {
        return None;
    };
    object
        .iter()
        .find_map(|(key, value)| (key == name).then(|| value.clone()))
}

fn set_disposed(value: &Value, disposed: bool) -> Result<(), VmError> {
    *state(value)?.borrow_mut() = Value::Boolean(disposed);
    Ok(())
}

fn is_disposed(value: &Value) -> bool {
    matches!(value, Value::Boolean(true))
}

fn push_record(records: &Rc<RefCell<Value>>, kind: &str, value: Value, method: Value) {
    let Value::Array(values) = &mut *records.borrow_mut() else {
        return;
    };
    let values = Rc::make_mut(values);
    let index = values.logical_len();
    values.set_index(index, record(kind, value, method));
}

fn record(kind: &str, value: Value, method: Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("kind".into(), Value::String(kind.into())),
        ("value".into(), value),
        ("method".into(), method),
    ])))
}

fn take_records(records: &Rc<RefCell<Value>>) -> Result<Vec<Value>, VmError> {
    let mut value = records.borrow_mut();
    let Value::Array(values) = &*value else {
        return Err(incompatible_receiver());
    };
    let result = values.snapshot();
    *value = Value::Array(Rc::new(ArrayData::new(Vec::new())));
    Ok(result)
}

fn record_field(record: &Value, name: &str) -> Option<Value> {
    let Value::Object(record) = record else {
        return None;
    };
    record
        .iter()
        .find_map(|(key, value)| (key == name).then(|| value.clone()))
}

fn ensure_callable(value: &Value) -> Result<(), VmError> {
    crate::conversion::is_callable(value)
        .then_some(())
        .ok_or_else(crate::vm::not_callable)
}

fn incompatible_receiver() -> VmError {
    crate::value::error::throw_type_error("incompatible DisposableStack receiver")
}

fn requires_new() -> VmError {
    crate::value::error::throw_type_error("Constructor DisposableStack requires 'new'")
}
