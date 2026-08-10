use std::cell::RefCell;

use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};

thread_local! {
    static OBJECTS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

struct ScopeGuard;

pub(crate) struct FunctionGuard {
    previous: Vec<Value>,
}

impl FunctionGuard {
    pub(crate) fn isolate() -> Self {
        let previous = OBJECTS.with(|objects| objects.replace(Vec::new()));
        Self { previous }
    }
}

impl Drop for FunctionGuard {
    fn drop(&mut self) {
        OBJECTS.with(|objects| objects.replace(std::mem::take(&mut self.previous)));
    }
}

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
    if matches!(value, Value::Undefined) && !has_key(&target, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

fn resolve(key: &str) -> Result<Option<Value>, VmError> {
    let Some(target) = binding_target(key)? else {
        return Ok(None);
    };
    crate::execute::get_property_result(&target, key).map(Some)
}

pub(crate) fn resolve_binding(key: &str) -> Result<Option<Value>, VmError> {
    let Some(target) = binding_target(key)? else {
        return Ok(None);
    };
    crate::execute::get_property_result(&target, key).map(Some)
}

pub(crate) fn binding_target(key: &str) -> Result<Option<Value>, VmError> {
    let objects = OBJECTS.with(|objects| objects.borrow().clone());
    for object in objects.iter().rev() {
        if has_key(object, key)? {
            return Ok(Some(object.clone()));
        }
    }
    Ok(None)
}

pub(crate) fn set_if_bound(key: &str, value: &Value) -> Result<bool, VmError> {
    let objects = OBJECTS.with(|objects| objects.borrow().clone());
    for (index, object) in objects.iter().enumerate().rev() {
        if has_key(object, key)? {
            let updated = crate::proxy::proxy_set(object, key, value, None)?;
            OBJECTS.with(|objects| objects.borrow_mut()[index] = updated);
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_key(value: &Value, key: &str) -> Result<bool, VmError> {
    Ok(match value {
        Value::Proxy(_) => crate::proxy::proxy_has(value, key)? == Value::Boolean(true),
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
    })
}
