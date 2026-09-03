use std::cell::RefCell;

use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};

thread_local! {
    static OBJECTS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static CAPTURED_BASE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct ScopeGuard;

pub(crate) struct FunctionGuard {
    previous: Vec<Value>,
    previous_base: usize,
}

// Closures created under `with` still need a dedicated dynamic-environment
// capture path; the current guard intentionally isolates each function call.
impl FunctionGuard {
    pub(crate) fn isolate() -> Self {
        let previous = OBJECTS.with(|objects| objects.replace(Vec::new()));
        let previous_base = CAPTURED_BASE.with(|base| base.replace(0));
        Self {
            previous,
            previous_base,
        }
    }

    pub(crate) fn install(captured: &[Value]) -> Self {
        let live = captured.iter().map(live_object).collect();
        let previous = OBJECTS.with(|objects| objects.replace(live));
        let previous_base = CAPTURED_BASE.with(|base| base.replace(captured.len()));
        Self {
            previous,
            previous_base,
        }
    }
}

impl Drop for FunctionGuard {
    fn drop(&mut self) {
        OBJECTS.with(|objects| objects.replace(std::mem::take(&mut self.previous)));
        CAPTURED_BASE.with(|base| base.set(self.previous_base));
    }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        OBJECTS.with(|objects| {
            objects.borrow_mut().pop();
        });
    }
}

pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<Completion, VmError> {
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
    Ok(
        match crate::vm::execute_function_code_completion_in_current_frame(body, registers)? {
            crate::completion::Completion::Break { label, value: None } => {
                crate::completion::Completion::Break {
                    label,
                    value: Some(Value::Undefined),
                }
            }
            crate::completion::Completion::Continue { label, value: None } => {
                crate::completion::Completion::Continue {
                    label,
                    value: Some(Value::Undefined),
                }
            }
            completion => completion,
        },
    )
}

pub(crate) fn capture() -> Vec<Value> {
    OBJECTS.with(|objects| objects.borrow().clone())
}

pub(crate) fn is_active() -> bool {
    OBJECTS.with(|objects| !objects.borrow().is_empty())
}

pub(crate) fn execute_resolve_global(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), VmError> {
    let Op::ResolveGlobal { dst, object, key } = op else {
        return Err(VmError::MissingReturn);
    };
    let target = crate::execute::read_register(registers, *object)?;
    let value = match resolve(key)? {
        Some(value) => value,
        None => crate::execute::get_property_result(&target, key)?,
    };
    let value = if key == "Math"
        && matches!(value, Value::Null | Value::Undefined)
        && crate::vm::realm_id_for_global_value(&target).is_some()
    {
        crate::vm::realm_intrinsic(crate::ops::Builtin::Math)
    } else {
        value
    };
    if matches!(value, Value::Undefined) && !has_property(&target, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn execute_name(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), VmError> {
    match op {
        Op::ResolveName { dst, key } => resolve_name(registers, *dst, key),
        Op::ResolveStrictName { dst, key } => resolve_strict_name(registers, *dst, key),
        Op::SetName { key, src, strict } => set_name(registers, key, *src, *strict),
        Op::CheckStrictName { key } => check_strict_name(key),
        _ => Err(VmError::MissingReturn),
    }
}

pub(crate) fn set_resolved(
    registers: &mut crate::register_file::RegisterFile,
    target_register: u16,
    key: &str,
    src: u16,
    strict: bool,
) -> Result<(), VmError> {
    let mut target = crate::execute::read_register(registers, target_register)?;
    while let Some(updated) = crate::locals::replacement(&target) {
        if target == updated {
            break;
        }
        target = updated;
    }
    if let Some(owner) = crate::vm::resolve_global_owner(&target) {
        if strict && !has_property(&owner, key)? {
            return Err(crate::value::error::throw_reference_error(&format!(
                "{key} is not defined"
            )));
        }
        if !strict {
            target = owner;
        }
    }
    if strict
        && crate::vm::is_global_object(&target)
        && !has_property(&crate::vm::current_global_object(), key)?
    {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    let value = crate::execute::read_register(registers, src)?;
    if matches!(target, Value::Undefined) {
        return set_name_value(key, value, strict);
    }
    if !has_property(&target, key)? {
        if strict {
            return Err(crate::value::error::throw_reference_error(&format!(
                "{key} is not defined"
            )));
        }
        if crate::globals::immutable_value(key).is_some() {
            return Ok(());
        }
    }
    if key == "length" && is_boxed_string_target(&target) {
        return Ok(());
    }
    publish_set(&target, key, &value)?;
    // Dynamic local assignments use a captured `with` target directly. Keep
    // the active object stack on the copy-on-write replacement so later
    // identifier resolution in the same `with` body observes the write.
    publish_active_replacement(&target);
    Ok(())
}

fn is_boxed_string_target(value: &Value) -> bool {
    match value {
        Value::Object(properties) => properties
            .iter()
            .any(|(name, value)| name == "_value" && matches!(value, Value::String(_))),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .is_some_and(|object| is_boxed_string_target(&Value::Object(object))),
        Value::BindingCell(cell) => is_boxed_string_target(&cell.borrow()),
        _ => false,
    }
}

fn set_name_value(key: &str, value: Value, strict: bool) -> Result<(), VmError> {
    if crate::locals::is_immutable_name(key) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    if strict && crate::globals::immutable_value(key).is_some() {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    if set_if_bound(key, &value)? {
        return Ok(());
    }
    if crate::locals::set_eval_named(key, value.clone())
        || crate::locals::set_named(key, value.clone())
    {
        return Ok(());
    }
    let captured_global = crate::locals::current().get(0);
    let global = if matches!(captured_global, Value::Object(_) | Value::ObjectAlias(_))
        && has_property(&captured_global, key)?
    {
        crate::vm::resolve_global_owner(&captured_global).unwrap_or(captured_global)
    } else {
        crate::vm::current_global_object()
    };
    let semantic_global =
        crate::vm::resolve_global_owner(&global).unwrap_or_else(|| global.clone());
    if strict && !has_property(&semantic_global, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    if crate::builtins::descriptor_flag(&semantic_global, key, "writable") != Some(false) {
        let _ = crate::global_environment::store_global_binding(key, value.clone());
    }
    let updated = crate::builtins::set_property(global.clone(), key, value);
    crate::vm::synchronize_global_object(
        &mut crate::register_file::RegisterFile::new(),
        &global,
        &updated,
    );
    Ok(())
}

pub(crate) fn execute_delete_name(
    registers: &mut crate::register_file::RegisterFile,
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
        let object = live_object(object);
        if !has_property(&object, key)? || is_unscopable(&object, key)? {
            continue;
        }
        let (updated, deleted) = crate::builtins::delete_property(object.clone(), key);
        crate::locals::replace_value(&object, &updated);
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

pub(crate) fn resolve_name(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    key: &str,
) -> Result<(), VmError> {
    if crate::locals::is_initializing_class_name(key) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access '{key}' before initialization"
        )));
    }
    let global = crate::vm::current_global_object();
    let binding = resolve_binding(key)?;
    let immutable = crate::globals::immutable_value(key);
    let eval = crate::locals::resolve_eval_name(key);
    let context = crate::vm::current_context_or_default();
    let host_value = context.host_value(key);
    let host_binding = context.host_binding(key);
    let bound = binding.is_some()
        || eval.is_some()
        || crate::locals::has_name(key)
        || immutable.is_some()
        || host_value.is_some()
        || host_binding.is_some();
    let value = match host_value
        .or(binding)
        .or(eval)
        .or_else(|| crate::locals::resolve_name(key))
        .or(immutable)
        .or_else(|| host_binding.map(crate::host_api::capability_function))
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
    let value = if key == "Math" && matches!(value, Value::Null | Value::Undefined) {
        crate::vm::realm_intrinsic(crate::ops::Builtin::Math)
    } else {
        value
    };
    if matches!(value, Value::Undefined) && !bound && !has_property(&global, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

pub(crate) fn resolve_strict_name(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    key: &str,
) -> Result<(), VmError> {
    let Some(target) = binding_target(key)? else {
        return resolve_name(registers, dst, key);
    };
    if !has_property(&target, key)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{key} is not defined"
        )));
    }
    let value = crate::execute::get_property_result(&target, key)?;
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

fn set_name(
    registers: &mut crate::register_file::RegisterFile,
    key: &str,
    src: u16,
    strict: bool,
) -> Result<(), VmError> {
    let value = crate::execute::read_register(registers, src)?;
    if crate::locals::is_immutable_name(key) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    if strict && crate::globals::immutable_value(key).is_some() {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    if set_if_bound(key, &value)? {
        return Ok(());
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
    let updated = crate::builtins::set_property(global.clone(), key, value);
    crate::vm::synchronize_global_object(registers, &global, &updated);
    Ok(())
}

fn check_strict_name(key: &str) -> Result<(), VmError> {
    if binding_target(key)?.is_some()
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

/// WithBaseObject for an identifier call: the innermost `with` object
/// that owns this callable.
pub(crate) fn receiver_for_callable(callee: &Value) -> Option<Value> {
    let objects = OBJECTS.with(|objects| objects.borrow().clone());
    for object in objects.iter().rev() {
        let object = live_object(object);
        if callable_on(&object, callee) {
            return Some(object);
        }
    }
    None
}

fn callable_on(object: &Value, callee: &Value) -> bool {
    let Value::Object(object) = object else {
        return false;
    };
    object.properties.iter().any(|(name, value)| {
        !name.starts_with('\0') && crate::builtins::same_value(Some(&value), Some(callee))
    })
}

pub(crate) fn resolve_binding(key: &str) -> Result<Option<Value>, VmError> {
    let Some(target) = binding_target(key)? else {
        return Ok(None);
    };
    if matches!(target, Value::Proxy(_)) && !has_property(&target, key)? {
        return Ok(None);
    }
    crate::execute::get_property_result(&target, key).map(Some)
}

/// Resolve only objects introduced by a `with` in the current activation.
/// Captured `with` objects remain an outer environment and therefore do not
/// shadow the function's own var bindings.
pub(crate) fn resolve_active_binding(key: &str) -> Result<Option<Value>, VmError> {
    let (objects, base) = OBJECTS.with(|objects| {
        (
            objects.borrow().clone(),
            CAPTURED_BASE.with(|base| base.get()),
        )
    });
    for object in objects.iter().skip(base).rev() {
        let object = live_object(object);
        if has_property(&object, key)? && !is_unscopable(&object, key)? {
            if matches!(object, Value::Proxy(_)) && !has_property(&object, key)? {
                return Ok(None);
            }
            return crate::execute::get_property_result(&object, key).map(Some);
        }
    }
    Ok(None)
}

pub(crate) fn binding_target(key: &str) -> Result<Option<Value>, VmError> {
    let objects = OBJECTS.with(|objects| objects.borrow().clone());
    for object in objects.iter().rev() {
        let object = live_object(object);
        if !crate::value::is_object(&object) {
            continue;
        }
        if has_property(&object, key)? && !is_unscopable(&object, key)? {
            return Ok(Some(object));
        }
    }
    Ok(None)
}

pub(crate) fn active_binding_target(key: &str) -> Result<Option<Value>, VmError> {
    let (objects, base) = OBJECTS.with(|objects| {
        (
            objects.borrow().clone(),
            CAPTURED_BASE.with(|base| base.get()),
        )
    });
    for object in objects.iter().skip(base).rev() {
        let object = live_object(object);
        if !crate::value::is_object(&object) {
            continue;
        }
        if has_property(&object, key)? && !is_unscopable(&object, key)? {
            return Ok(Some(object));
        }
    }
    Ok(None)
}

/// Copy-on-write object sets publish a replacement; `with` must see it.
fn live_object(value: &Value) -> Value {
    crate::locals::resolved_replacement(value.clone())
}

pub(crate) fn publish_active_replacement(original: &Value) {
    let updated = live_object(original);
    OBJECTS.with(|objects| {
        let mut objects = objects.borrow_mut();
        for object in objects.iter_mut() {
            let same = match (&*object, original) {
                (Value::Object(left), Value::Object(right)) => left.identity() == right.identity(),
                _ => false,
            };
            if same {
                *object = updated.clone();
            }
        }
    });
}

/// `[[Set]]` returns a boolean; the live object is the published replacement.
fn publish_set(target: &Value, key: &str, value: &Value) -> Result<bool, VmError> {
    let succeeded = crate::proxy::proxy_set(target, key, value, None)?;
    Ok(crate::execute::is_truthy(&succeeded))
}

pub(crate) fn set_if_bound(key: &str, value: &Value) -> Result<bool, VmError> {
    let objects = OBJECTS.with(|objects| objects.borrow().clone());
    for (index, object) in objects.iter().enumerate().rev() {
        let object = live_object(object);
        if !crate::value::is_object(&object) {
            continue;
        }
        if has_property(&object, key)? && !is_unscopable(&object, key)? {
            let global =
                crate::vm::is_global_object(&object).then(crate::vm::current_global_object);
            publish_set(&object, key, value)?;
            let updated = live_object(&object);
            if let Some(global) = global {
                crate::vm::synchronize_global_object(
                    &mut crate::register_file::RegisterFile::new(),
                    &global,
                    &updated,
                );
            }
            OBJECTS.with(|objects| objects.borrow_mut()[index] = updated);
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_unscopable(object: &Value, key: &str) -> Result<bool, VmError> {
    let unscopables = crate::execute::get_property_result(object, "Symbol.unscopables")?;
    if matches!(unscopables, Value::Undefined | Value::Null) {
        return Ok(false);
    }
    if !crate::value::is_object(&unscopables) {
        return Ok(false);
    }
    let blocked = crate::execute::get_property_result(&unscopables, key)?;
    Ok(crate::execute::is_truthy(&blocked))
}

pub(crate) fn has_property(value: &Value, key: &str) -> Result<bool, VmError> {
    let owned = crate::vm::resolve_global_owner(value).unwrap_or_else(|| value.clone());
    let value = &owned;
    // Host globals live in the execution context rather than as ordinary
    // properties on the realm global object.  They must nevertheless
    // participate in global binding existence checks (notably `process` and
    // the capability-backed `require`).
    if crate::vm::is_global_object(value) {
        let context = crate::vm::current_context_or_default();
        if context.host_value(key).is_some() || context.host_binding(key).is_some() {
            return Ok(true);
        }
    }
    crate::module_bindings::exports(value, key)?;
    if let Value::BindingCell(cell) = value {
        return has_property(&cell.borrow(), key);
    }
    if let Value::ObjectAlias(alias) = value {
        let Some(object) = alias.0.borrow().upgrade().map(Value::Object) else {
            return Ok(false);
        };
        return has_property(&object, key);
    }
    if matches!(value, Value::Proxy(_)) {
        let result = crate::proxy::proxy_has(value, key)?;
        return Ok(crate::execute::is_truthy(&result));
    }
    if value.is_typed_array() && crate::typed_array_ops::canonical_numeric_index(key) {
        return Ok(crate::typed_array_ops::typed_array_index(key)
            .is_some_and(|index| crate::typed_array_prototype::index_exists(value, index)));
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

pub(crate) fn execute_has_property(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), VmError> {
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

pub(crate) fn execute_has_private(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), VmError> {
    let Op::HasPrivate { dst, object, name } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let name = crate::private_environment::resolve(*name).ok_or_else(|| {
        crate::value::error::throw_type_error(
            "Private field access on an object without the required brand",
        )
    })?;
    let slots = crate::private_slots::slots(&object)?;
    let result = Value::Boolean(slots.borrow().iter().any(|(id, _)| id == &name));
    crate::execute::write_value(registers, *dst, result);
    Ok(())
}
