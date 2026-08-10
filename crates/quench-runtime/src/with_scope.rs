use std::cell::RefCell;

use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};

thread_local! {
    static OBJECTS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

struct ScopeGuard;

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        OBJECTS.with(|objects| {
            objects.borrow_mut().pop();
        });
    }
}

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<Completion, VmError> {
    let Op::With { object, body } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    if matches!(object, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "with object cannot be null or undefined",
        ));
    }
    OBJECTS.with(|objects| objects.borrow_mut().push(object));
    let _guard = ScopeGuard;
    crate::execute::execute_completion_in_place(body, registers)
}

pub(crate) fn execute_resolve_global(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::ResolveGlobal { dst, object, key } = op else {
        return Err(VmError::MissingReturn);
    };
    let target = crate::execute::read_register(registers, *object)?;
    let value = match resolve(key)? {
        Some(value) => value,
        None => crate::execute::get_property_result(&target, key)?,
    };
    if matches!(value, Value::Undefined) && !has_key(&target, key) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

fn resolve(key: &str) -> Result<Option<Value>, VmError> {
    OBJECTS.with(|objects| {
        objects
            .borrow()
            .iter()
            .rev()
            .find(|object| has_key(object, key))
            .map(|object| crate::execute::get_property_result(object, key))
            .transpose()
    })
}

pub(crate) fn set_if_bound(key: &str, value: &Value) -> bool {
    OBJECTS.with(|objects| {
        let mut objects = objects.borrow_mut();
        let Some(object) = objects.iter_mut().rev().find(|object| has_key(object, key)) else {
            return false;
        };
        *object = crate::builtins::set_property(object.clone(), key, value.clone());
        true
    })
}

fn has_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(properties) => properties.iter().any(|(name, _)| name == key),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .is_some_and(|properties| properties.iter().any(|(name, _)| name == key)),
        Value::Array(values) => {
            key == "length" || key.parse::<usize>().is_ok_and(|index| index < values.len())
        }
        _ => !matches!(crate::execute::get_property(value, key), Value::Undefined),
    }
}
