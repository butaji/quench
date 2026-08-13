//! Explicit resource-management stack intrinsics.

use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{ArrayData, ObjectData, Value},
};

const STATE: &str = "\0disposable:disposed";
const RECORDS: &str = "\0disposable:records";

pub(crate) fn construct() -> Result<Value, VmError> {
    Ok(stack(Value::Array(Rc::new(ArrayData::new(Vec::new())))))
}

pub(crate) fn construct_async() -> Result<Value, VmError> {
    Ok(Value::Object(Rc::new(ObjectData::new(vec![(
        "\0prototype".into(),
        Value::Builtin(Builtin::AsyncDisposableStackPrototype),
    )]))))
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
        _ => return None,
    })
}

pub(crate) fn accessor(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    (key == "disposed" && is_stack(value)).then(|| disposed(Some(receiver)))
}

fn stack(records: Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        (
            "\0prototype".into(),
            Value::Builtin(Builtin::DisposableStackPrototype),
        ),
        (STATE.into(), cell(Value::Boolean(false))),
        (RECORDS.into(), cell(records)),
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
