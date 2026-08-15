use std::{cell::RefCell, rc::Rc};

use crate::{environment::Environment, execute::VmError, value::Value};

thread_local! {
    static CURRENT_ENVIRONMENT: RefCell<Option<Rc<Environment>>> = const { RefCell::new(None) };
    static GLOBAL_LEXICAL_ENVIRONMENT: RefCell<Option<Rc<Environment>>> = const { RefCell::new(None) };
    static REPLACEMENTS: RefCell<Vec<(Value, Value)>> = const { RefCell::new(Vec::new()) };
    static STRICT_EVAL: RefCell<bool> = const { RefCell::new(false) };
    static ACTIVE_EVAL: RefCell<bool> = const { RefCell::new(false) };
}

pub(crate) struct StrictEvalGuard {
    previous: bool,
    previous_active: bool,
}

impl StrictEvalGuard {
    pub(crate) fn install(active: bool) -> Self {
        let previous = STRICT_EVAL.with(|value| value.replace(active));
        let previous_active = ACTIVE_EVAL.with(|value| value.replace(true));
        Self {
            previous,
            previous_active,
        }
    }
}

impl Drop for StrictEvalGuard {
    fn drop(&mut self) {
        STRICT_EVAL.with(|value| value.replace(self.previous));
        ACTIVE_EVAL.with(|value| value.replace(self.previous_active));
    }
}

pub(crate) struct EnvironmentGuard {
    previous: Option<Rc<Environment>>,
    previous_global: Option<Rc<Environment>>,
}

pub(crate) struct IterationBinding {
    environment: Rc<Environment>,
    slot: u16,
    previous: Option<Rc<RefCell<Value>>>,
}

impl IterationBinding {
    pub(crate) fn install(slot: u16, value: Value) -> Self {
        let environment = current();
        let previous = Some(environment.replace_slot(slot, value));
        Self {
            environment,
            slot,
            previous,
        }
    }
}

impl Drop for IterationBinding {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            self.environment.restore_slot(self.slot, previous);
        }
    }
}

impl EnvironmentGuard {
    pub(crate) fn install(environment: Rc<Environment>) -> Self {
        let previous = CURRENT_ENVIRONMENT.with(|current| current.replace(Some(environment)));
        let previous_global = GLOBAL_LEXICAL_ENVIRONMENT.with(|global| {
            let previous_global = global.borrow().clone();
            if previous.is_none() && previous_global.is_none() {
                let current = CURRENT_ENVIRONMENT.with(|current| current.borrow().clone());
                global.replace(current);
            }
            previous_global
        });
        Self {
            previous,
            previous_global,
        }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        CURRENT_ENVIRONMENT.with(|current| current.replace(self.previous.take()));
        GLOBAL_LEXICAL_ENVIRONMENT.with(|global| global.replace(self.previous_global.take()));
    }
}

pub(crate) fn current() -> Rc<Environment> {
    CURRENT_ENVIRONMENT
        .with(|current| current.borrow().clone())
        .unwrap_or_default()
}

pub(crate) fn global_lexical() -> Option<Rc<Environment>> {
    GLOBAL_LEXICAL_ENVIRONMENT.with(|global| global.borrow().clone())
}

pub(crate) fn global_has_own_name(name: &str) -> bool {
    global_lexical().is_some_and(|environment| environment.has_own_name(name))
}

pub(crate) fn has_name(name: &str) -> bool {
    current().has_name(name)
        || global_lexical().is_some_and(|environment| environment.has_name(name))
}

pub(crate) fn is_installed() -> bool {
    CURRENT_ENVIRONMENT.with(|current| current.borrow().is_some())
}

pub(crate) fn store(registers: &[Value], slot: u16, source: u16) -> Result<(), VmError> {
    let value = crate::execute::read_register(registers, source)?;
    if current().is_immutable_slot(slot) && !current().is_uninitialized(slot) {
        if ACTIVE_EVAL.with(|active| *active.borrow())
            && !STRICT_EVAL.with(|strict| *strict.borrow())
        {
            return Ok(());
        }
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to immutable binding",
        ));
    }
    current().set(slot, value);
    if slot == 0 {
        crate::vm::initialize_global_object(&current().get(slot));
    }
    Ok(())
}

pub(crate) fn store_function_name(
    _registers: &[Value],
    _slot: u16,
    _source: u16,
    strict: bool,
) -> Result<(), VmError> {
    if strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to function name",
        ));
    }
    Ok(())
}

pub(crate) fn load_binding(
    registers: &mut Vec<Value>,
    dst: u16,
    slot: u16,
    name: &str,
    dynamic: bool,
) -> Result<(), VmError> {
    if let Some(value) = crate::with_scope::resolve_binding(name)? {
        ensure_module_binding_initialized(&value, name)?;
        crate::execute::write_value(registers, dst, value);
        return Ok(());
    }
    ensure_initialized(slot, name)?;
    if dynamic {
        if let Some(value) = resolve_eval_name(name) {
            crate::execute::write_value(registers, dst, value);
            return Ok(());
        }
    }
    let value = current().get(slot);
    ensure_module_binding_initialized(&value, name)?;
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

pub(crate) fn resolve_target(
    registers: &mut Vec<Value>,
    dst: u16,
    name: &str,
) -> Result<(), VmError> {
    let target = crate::with_scope::binding_target(name)?.unwrap_or(Value::Undefined);
    crate::execute::write_value(registers, dst, target);
    Ok(())
}

pub(crate) fn initialize_resolved(
    registers: &[Value],
    target: u16,
    slot: u16,
    name: &str,
    source: u16,
) -> Result<(), VmError> {
    let value = crate::execute::read_register(registers, source)?;
    let target = crate::execute::read_register(registers, target)?;
    current().set(slot, value.clone());
    if matches!(target, Value::Undefined) {
        return Ok(());
    } else {
        crate::proxy::proxy_set(&target, name, &value, None)?;
    }
    Ok(())
}

pub(crate) fn set_resolved_local(
    registers: &[Value],
    target: u16,
    slot: u16,
    name: &str,
    strict: bool,
    source: u16,
) -> Result<(), VmError> {
    let mut target = crate::execute::read_register(registers, target)?;
    let value = crate::execute::read_register(registers, source)?;
    while let Some(updated) = replacement(&target) {
        target = updated;
    }
    if matches!(target, Value::Undefined) {
        ensure_initialized(slot, name)?;
        write(slot, value);
        return Ok(());
    }
    if strict && !crate::with_scope::has_property(&target, name)? {
        return Err(crate::value::error::throw_reference_error(&format!(
            "{name} is not defined"
        )));
    }
    let updated = crate::proxy::proxy_set(&target, name, &value, None)?;
    replace_value(&target, &updated);
    Ok(())
}

pub(crate) fn load_resolved_local(
    registers: &mut Vec<Value>,
    dst: u16,
    target: u16,
    slot: u16,
    name: &str,
) -> Result<(), VmError> {
    let target = crate::execute::read_register(registers, target)?;
    let value = if matches!(target, Value::Undefined) {
        ensure_initialized(slot, name)?;
        current().get(slot)
    } else {
        crate::execute::get_property_result(&target, name)?
    };
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

pub(crate) fn load(registers: &mut Vec<Value>, dst: u16, slot: u16) -> Result<(), VmError> {
    ensure_initialized(slot, "binding")?;
    let value = current().get(slot);
    ensure_module_binding_initialized(&value, "binding")?;
    crate::execute::write_value(registers, dst, value);
    Ok(())
}

fn ensure_module_binding_initialized(value: &Value, name: &str) -> Result<(), VmError> {
    if crate::module_bindings::ModuleBindingCell::is_uninitialized(value) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access an uninitialized module binding: {name}"
        )));
    }
    Ok(())
}

pub(crate) fn write(slot: u16, value: Value) {
    current().set(slot, value);
}

pub(crate) fn mark_uninitialized(slot: u16) {
    current().mark_uninitialized(slot);
}

pub(crate) fn check_initialized(slot: u16, name: &str) -> Result<(), VmError> {
    ensure_initialized(slot, name)
}

pub(crate) fn initialize(slot: u16) {
    current().initialize(slot);
}

pub(crate) fn load_parameter(
    registers: &mut Vec<Value>,
    dst: u16,
    slot: u16,
) -> Result<(), VmError> {
    crate::execute::write_value(registers, dst, current().get(slot));
    Ok(())
}

pub(crate) fn slot_cell(slot: u16) -> Rc<RefCell<Value>> {
    current().slot_cell(slot)
}

pub(crate) fn install_slot_cell(slot: u16, cell: Rc<RefCell<Value>>) {
    current().install_slot_cell(slot, cell);
}

fn ensure_initialized(slot: u16, name: &str) -> Result<(), VmError> {
    if current().is_uninitialized(slot) {
        return Err(crate::value::error::throw_reference_error(&format!(
            "Cannot access '{name}' before initialization"
        )));
    }
    Ok(())
}

pub(crate) fn alias_eval_name(name: &str, slot: u16) {
    let environment = current();
    if !STRICT_EVAL.with(|value| *value.borrow()) {
        environment.alias_eval_caller_name(name, slot);
    }
}

pub(crate) fn declare_global_lexical(name: &str, slot: u16, immutable: bool) {
    let binding = current().slot_cell(slot);
    let environment = global_lexical().unwrap_or_else(current);
    environment.alias_binding(name, binding);
    if immutable {
        environment.mark_immutable(name);
        current().mark_immutable_slot(slot);
    }
}

pub(crate) fn is_immutable_name(name: &str) -> bool {
    global_lexical().is_some_and(|environment| environment.is_immutable_name(name))
}

pub(crate) fn resolve_name(name: &str) -> Option<Value> {
    current().resolve_name(name).or_else(|| {
        global_has_own_name(name)
            .then(global_lexical)
            .flatten()?
            .resolve_name(name)
    })
}

pub(crate) fn resolve_eval_name(name: &str) -> Option<Value> {
    current().resolve_eval_name(name)
}

pub(crate) fn resolve_name_or_undefined(name: &str) -> Result<Value, VmError> {
    if let Some(value) = crate::with_scope::resolve_binding(name)? {
        return Ok(value);
    }
    if let Some(value) = resolve_eval_name(name).or_else(|| resolve_name(name)) {
        return Ok(value);
    }
    if let Some(value) = crate::globals::immutable_value(name) {
        return Ok(value);
    }
    crate::execute::get_property_result(&crate::vm::current_global_object(), name)
}

pub(crate) fn set_named(name: &str, value: Value) -> bool {
    current().set_named(name, value)
}

pub(crate) fn set_eval_named(name: &str, value: Value) -> bool {
    current().set_eval_named(name, value)
}

pub(crate) fn delete_named(name: &str, slot: u16) -> bool {
    let environment = current();
    environment.delete_eval_caller_name(name, slot)
}

pub(crate) fn capture(count: u16) -> Rc<Environment> {
    Environment::capture(&current(), count)
}

pub(crate) fn replace_value(old: &Value, new: &Value) {
    current().replace_value(old, new);
    REPLACEMENTS.with(|replacements| {
        replacements.borrow_mut().push((old.clone(), new.clone()));
    });
}

pub(crate) fn replacement(value: &Value) -> Option<Value> {
    REPLACEMENTS.with(|replacements| {
        replacements
            .borrow()
            .iter()
            .rev()
            .find_map(|(old, new)| same_identity(old, value).then(|| new.clone()))
    })
}

pub(crate) fn reset_replacements() {
    REPLACEMENTS.with(|replacements| replacements.borrow_mut().clear());
}

pub(crate) fn resolved_replacement(value: Value) -> Value {
    let mut value = value;
    while let Some(updated) = replacement(&value) {
        if same_identity(&value, &updated) {
            break;
        }
        value = updated;
    }
    value
}

fn same_identity(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => Rc::ptr_eq(left, right),
        (Value::Object(left), Value::Object(right)) => Rc::ptr_eq(left, right),
        (Value::Function(left), Value::Function(right)) => Rc::ptr_eq(left, right),
        _ => false,
    }
}
