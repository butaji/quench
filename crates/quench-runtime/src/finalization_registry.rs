//! FinalizationRegistry intrinsics.

use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    ops::Builtin,
    value::{ArrayData, ObjectData, Value},
};

const CALLBACK: &str = "\0finalization:callback";
const CELLS: &str = "\0finalization:cells";
const TARGET: &str = "\0finalization:target";
const HELD: &str = "\0finalization:held";
const TOKEN: &str = "\0finalization:token";

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::conversion::is_callable(&callback) {
        return Err(crate::value::error::throw_type_error(
            "FinalizationRegistry cleanup callback is not callable",
        ));
    }
    Ok(Value::Object(Rc::new(ObjectData::new(vec![
        (
            "\0prototype".into(),
            Value::Builtin(Builtin::FinalizationRegistryPrototype),
        ),
        (CALLBACK.into(), callback),
        (
            CELLS.into(),
            Value::BindingCell(Rc::new(RefCell::new(Value::Array(Rc::new(
                ArrayData::new(Vec::new()),
            ))))),
        ),
    ]))))
}

pub(crate) fn execute(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    use Builtin::*;
    Some(match builtin {
        FinalizationRegistry => Err(crate::value::error::throw_type_error(
            "Constructor FinalizationRegistry requires 'new'",
        )),
        FinalizationRegistryRegister => register(receiver, arguments),
        FinalizationRegistryUnregister => unregister(receiver, arguments),
        _ => return None,
    })
}

fn register(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let cells = cells(receiver)?;
    let target = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !can_be_held_weakly(&target) {
        return Err(crate::value::error::throw_type_error(
            "FinalizationRegistry target cannot be held weakly",
        ));
    }
    let held = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if crate::builtins::same_value(Some(&target), Some(&held)) {
        return Err(crate::value::error::throw_type_error(
            "FinalizationRegistry target and held value must differ",
        ));
    }
    let token = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    if !matches!(token, Value::Undefined) && !can_be_held_weakly(&token) {
        return Err(crate::value::error::throw_type_error(
            "FinalizationRegistry unregisterToken cannot be held weakly",
        ));
    }
    push_cell(&cells, target, held, token);
    Ok(Value::Undefined)
}

fn unregister(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let cells = cells(receiver)?;
    let token = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !can_be_held_weakly(&token) {
        return Err(crate::value::error::throw_type_error(
            "FinalizationRegistry unregisterToken cannot be held weakly",
        ));
    }
    Ok(Value::Boolean(remove_cells(&cells, &token)))
}

fn can_be_held_weakly(value: &Value) -> bool {
    crate::value::is_object(value)
        || matches!(value, Value::String(text) if crate::conversion::is_symbol(value) && !text.starts_with("Symbol.for."))
}

fn cells(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(properties)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "FinalizationRegistry method called on incompatible receiver",
        ));
    };
    properties
        .iter()
        .find_map(|(name, value)| (name == CELLS).then(|| value.clone()))
        .ok_or_else(|| {
            crate::value::error::throw_type_error(
                "FinalizationRegistry method called on incompatible receiver",
            )
        })
}

fn push_cell(cells: &Value, target: Value, held: Value, token: Value) {
    let Value::BindingCell(cell) = cells else { return };
    let Value::Array(records) = &mut *cell.borrow_mut() else { return };
    let record = Value::Object(Rc::new(ObjectData::new(vec![
        (TARGET.into(), target),
        (HELD.into(), held),
        (TOKEN.into(), token),
    ])));
    let index = records.logical_len();
    Rc::make_mut(records).set_index(index, record);
}

fn remove_cells(cells: &Value, token: &Value) -> bool {
    let Value::BindingCell(cell) = cells else { return false };
    let mut value = cell.borrow_mut();
    let Value::Array(records) = &*value else { return false };
    let snapshot = records.snapshot();
    let kept: Vec<Value> = snapshot
        .into_iter()
        .filter(|record| !same_token(record, token))
        .collect();
    let removed = kept.len() != records.len();
    *value = Value::Array(Rc::new(ArrayData::new(kept)));
    removed
}

fn same_token(record: &Value, token: &Value) -> bool {
    let Value::Object(properties) = record else { return false };
    let record_token = properties
        .iter()
        .find_map(|(name, value)| (name == TOKEN).then(|| value.clone()));
    crate::builtins::same_value(record_token.as_ref(), Some(token))
}
