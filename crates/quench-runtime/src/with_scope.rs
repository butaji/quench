use std::cell::RefCell;

use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};

thread_local! {
    static OBJECTS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

struct ScopeGuard;

pub(crate) struct FunctionGuard {
    previous: Vec<Value>,
}

// Closures created under `with` still need a dedicated dynamic-environment
// capture path; the current guard intentionally isolates each function call.
impl FunctionGuard {
    pub(crate) fn isolate() -> Self {
        let previous = OBJECTS.with(|objects| objects.replace(Vec::new()));
        Self { previous }
    }

    pub(crate) fn install(captured: &[Value]) -> Self {
        let previous = OBJECTS.with(|objects| objects.replace(captured.to_vec()));
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
    let Some(body) = body.ops() else {
        return Err(VmError::MissingReturn);
    };
    OBJECTS.with(|objects| objects.borrow_mut().push(object));
    let _guard = ScopeGuard;
    crate::execute::execute_completion_in_place(body, registers)
}

pub(crate) fn capture() -> Vec<Value> {
    OBJECTS.with(|objects| objects.borrow().clone())
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
    if matches!(value, Value::Undefined) && !has_property(&target, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn execute_name(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    match op {
        Op::ResolveName { dst, key } => resolve_name(registers, *dst, key),
        Op::SetName { key, src, strict } => set_name(registers, key, *src, *strict),
        Op::CheckStrictName { key } => check_strict_name(key),
        _ => Err(VmError::MissingReturn),
    }
}

pub(crate) fn set_resolved(
    registers: &mut [Value],
    target_register: u16,
    key: &str,
    src: u16,
    strict: bool,
) -> Result<(), VmError> {
    let mut target = crate::execute::read_register(registers, target_register)?;
    while let Some(updated) = crate::locals::replacement(&target) {
        target = updated;
    }
    let value = crate::execute::read_register(registers, src)?;
    let captured_global = crate::locals::current().get(0);
    if has_property(&captured_global, key)? {
        let updated = crate::proxy::proxy_set(&captured_global, key, &value, None)?;
        crate::vm::replace_global_object(&captured_global, &updated);
        return Ok(());
    }
    if matches!(target, Value::Undefined) {
        return set_name_value(key, value, strict);
    }
    if strict && !has_property(&target, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    let updated = crate::proxy::proxy_set(&target, key, &value, None)?;
    crate::locals::replace_value(&target, &updated);
    Ok(())
}

fn set_name_value(key: &str, value: Value, strict: bool) -> Result<(), VmError> {
    if crate::locals::is_immutable_name(key)
        || (strict && crate::globals::immutable_value(key).is_some())
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    if set_if_bound(key, &value)? {
        return Ok(());
    }
    if crate::globals::immutable_value(key).is_some() {
        return reject_immutable_global(strict);
    }
    if crate::locals::set_eval_named(key, value.clone())
        || crate::locals::set_named(key, value.clone())
    {
        return Ok(());
    }
    let global = crate::vm::current_global_object();
    if strict && !has_property(&global, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    if crate::builtins::descriptor_flag(&global, key, "writable") == Some(false) {
        return if strict {
            Err(crate::value::error::throw_type_error(
                "Cannot assign to read-only property",
            ))
        } else {
            Ok(())
        };
    }
    let updated = crate::builtins::set_property(global.clone(), key, value);
    crate::vm::synchronize_global_object(&mut Vec::new(), &global, &updated);
    Ok(())
}

pub(crate) fn execute_delete_name(
    registers: &mut Vec<Value>,
    dst: u16,
    key: &str,
    strict: bool,
) -> Result<(), VmError> {
    let deleted = if matches!(key, "NaN" | "Infinity" | "undefined") {
        false
    } else {
        match delete_from_with_objects(key)? {
            Some(value) => value,
            None => delete_from_global(key).unwrap_or(true),
        }
    };
    if !deleted && strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete non-configurable property",
        ));
    }
    crate::execute::write_value(registers, dst, Value::Boolean(deleted));
    Ok(())
}

fn delete_from_with_objects(key: &str) -> Result<Option<bool>, VmError> {
    let objects = OBJECTS.with(|objects| objects.borrow().clone());
    for (index, object) in objects.iter().enumerate().rev() {
        if !has_property(object, key)? || is_unscopable(object, key)? {
            continue;
        }
        let (updated, deleted) = crate::builtins::delete_property(object.clone(), key);
        crate::locals::replace_value(object, &updated);
        OBJECTS.with(|objects| objects.borrow_mut()[index] = updated);
        return Ok(Some(deleted));
    }
    Ok(None)
}

fn delete_from_global(key: &str) -> Option<bool> {
    let global = crate::vm::current_global_object();
    if !has_property(&global, key).ok()? {
        return Some(true);
    }
    let (updated, deleted) = crate::builtins::delete_property(global.clone(), key);
    crate::vm::replace_global_object(&global, &updated);
    Some(deleted)
}

fn resolve_name(registers: &mut Vec<Value>, dst: u16, key: &str) -> Result<(), VmError> {
    let global = match crate::locals::current().get(0) {
        Value::Object(_) => crate::locals::current().get(0),
        _ => crate::vm::current_global_object(),
    };
    let binding = resolve_binding(key)?;
    let immutable = crate::globals::immutable_value(key);
    let eval = crate::locals::resolve_eval_name(key);
    let bound =
        binding.is_some() || eval.is_some() || crate::locals::has_name(key) || immutable.is_some();
    let value = match binding
        .or(eval)
        .or_else(|| crate::locals::resolve_name(key))
        .or(immutable)
    {
        Some(value) => value,
        None => {
            let value = crate::execute::get_property_result(&global, key)?;
            if matches!(value, Value::Undefined) && !global_builtin_deleted(&global, key) {
                crate::vm::global_builtin_value(key).unwrap_or(value)
            } else {
                value
            }
        }
    };
    if matches!(value, Value::Undefined) && !bound && !has_property(&global, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

fn global_builtin_deleted(global: &Value, key: &str) -> bool {
    let Value::Object(properties) = global else {
        return false;
    };
    let marker = crate::builtins::deleted_key(key);
    properties.iter().any(|(name, _)| name == &marker)
}

fn set_name(registers: &mut Vec<Value>, key: &str, src: u16, strict: bool) -> Result<(), VmError> {
    let value = crate::execute::read_register(registers, src)?;
    if crate::locals::is_immutable_name(key)
        || (strict && crate::globals::immutable_value(key).is_some())
    {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    if set_if_bound(key, &value)? {
        return Ok(());
    }
    if crate::globals::immutable_value(key).is_some() {
        return reject_immutable_global(strict);
    }
    if crate::locals::set_named(key, value.clone()) {
        return Ok(());
    }
    let global = crate::vm::current_global_object();
    if strict && !has_property(&global, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    if crate::builtins::descriptor_flag(&global, key, "writable") == Some(false) {
        return if strict {
            Err(crate::value::error::throw_type_error(
                "Cannot assign to read-only property",
            ))
        } else {
            Ok(())
        };
    }
    let updated = crate::builtins::set_property(global.clone(), key, value);
    crate::vm::synchronize_global_object(registers, &global, &updated);
    Ok(())
}

fn reject_immutable_global(strict: bool) -> Result<(), VmError> {
    if strict {
        Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable global property",
        ))
    } else {
        Ok(())
    }
}

fn check_strict_name(key: &str) -> Result<(), VmError> {
    if crate::globals::immutable_value(key).is_some()
        || binding_target(key)?.is_some()
        || crate::locals::resolve_eval_name(key).is_some()
        || crate::locals::resolve_name(key).is_some()
        || has_property(&crate::vm::current_global_object(), key)?
    {
        return Ok(());
    }
    Err(crate::value::error::throw_reference_error(&format!(
        "{key} is not defined"
    )))
}

fn resolve(key: &str) -> Result<Option<Value>, VmError> {
    if let Some(value) = crate::locals::resolve_eval_name(key) {
        return Ok(Some(value));
    }
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
        if has_property(object, key)? && !is_unscopable(object, key)? {
            return Ok(Some(object.clone()));
        }
    }
    Ok(None)
}

pub(crate) fn set_if_bound(key: &str, value: &Value) -> Result<bool, VmError> {
    let objects = OBJECTS.with(|objects| objects.borrow().clone());
    for (index, object) in objects.iter().enumerate().rev() {
        if has_property(object, key)? && !is_unscopable(object, key)? {
            let updated = crate::proxy::proxy_set(object, key, value, None)?;
            let success = crate::execute::is_truthy(&updated);
            if success {
                OBJECTS.with(|objects| objects.borrow_mut()[index] = updated);
            }
            return Ok(success);
        }
    }
    Ok(false)
}

fn is_unscopable(object: &Value, key: &str) -> Result<bool, VmError> {
    let unscopables = crate::execute::get_property_result(object, "Symbol.unscopables\0")?;
    if matches!(unscopables, Value::Undefined | Value::Null) {
        return Ok(false);
    }
    let blocked = crate::execute::get_property_result(&unscopables, key)?;
    Ok(crate::execute::is_truthy(&blocked))
}

pub(crate) fn has_property(value: &Value, key: &str) -> Result<bool, VmError> {
    if let Some(id) = crate::vm::consume_deferred_namespace_marker(value, key) {
        crate::vm::execute_deferred_module(id)?;
    }
    if crate::builtins::is_module_namespace(value) {
        if let Value::Object(properties) = value {
            return Ok(properties.iter().any(|(name, _)| name == key));
        }
    }
    if let Value::ObjectAlias(alias) = value {
        let Some(object) = alias.0.borrow().upgrade().map(Value::Object) else {
            return Ok(false);
        };
        return has_property(&object, key);
    }
    if let Value::Proxy(proxy) = value {
        let trapped = crate::proxy::get_handler_trap(proxy, "has").is_some();
        let result = crate::proxy::proxy_has(value, key)?;
        if trapped {
            return Ok(crate::execute::is_truthy(&result));
        }
        return has_property(&proxy.target, key);
    }
    if crate::vm::is_global_object(value)
        && (crate::vm::global_builtin_exists(key)
            || matches!(key, "NaN" | "Infinity" | "undefined"))
    {
        return Ok(true);
    }
    let key_value = Value::String(key.to_string());
    let own = crate::builtins::object::has_own_property(Some(value), Some(&key_value));
    if own == Value::Boolean(true) {
        return Ok(true);
    }
    if let Some(prototype) = primitive_boxed_prototype(value) {
        return has_property(&prototype, key);
    }
    let prototype = crate::builtins::object::get_prototype_of(Some(value))?;
    if !matches!(prototype, Value::Null) && prototype != *value {
        return has_property(&prototype, key);
    }
    Ok(false)
}

/// The prototype a primitive value boxes to for property walks.
fn primitive_boxed_prototype(value: &Value) -> Option<Value> {
    use crate::ops::Builtin;
    Some(match value {
        Value::Number(_) => Value::Builtin(Builtin::NumberPrototype),
        Value::Boolean(_) => Value::Builtin(Builtin::BooleanPrototype),
        Value::BigInt(_) => Value::Builtin(Builtin::BigIntPrototype),
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            Value::Builtin(Builtin::SymbolPrototype)
        }
        Value::String(_) | Value::StringUnits(_) => Value::Builtin(Builtin::StringPrototype),
        _ => return None,
    })
}

pub(crate) fn execute_has_property(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::HasPropertyDynamic { dst, object, key } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    if !crate::value::is_object(&object) {
        return Err(crate::value::error::throw_type_error(
            "right-hand side of 'in' is not an object",
        ));
    }
    let key = crate::execute::read_register(registers, *key)?;
    let key = crate::conversion::to_property_key(&key)?;
    let result = Value::Boolean(has_property(&object, &key)?);
    crate::execute::write_value(registers, *dst, result);
    Ok(())
}
