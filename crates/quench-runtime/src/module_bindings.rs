use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{execute::VmError, value::Value};

/// Host-registered namespace evaluators, keyed by object identity.
type Evaluators = HashMap<*const crate::value::ObjectData, Rc<dyn Fn()>>;
/// Host resolver for `import()` / `import.defer()`.
type DynamicImportResolver = Rc<dyn Fn(&str, bool) -> Option<Value>>;

/// All mutable module-linking facts are kept in one record.  This makes reset
/// and observation data-driven instead of relying on several independent
/// thread-local cells with subtly different lifetimes.
#[derive(Default)]
struct ModuleBindingState {
    evaluators: Evaluators,
    pending_type_error: bool,
    pending_throw: Option<Value>,
    dynamic_import: Option<DynamicImportResolver>,
    await_advanced: bool,
    defer_fulfilled_await: bool,
}

thread_local! {
    static MODULE_STATE: RefCell<ModuleBindingState> =
        RefCell::new(ModuleBindingState::default());
}

/// Host-owned GetModuleNamespace for `import()` / `import.defer()`.
pub fn install_dynamic_import(resolve: DynamicImportResolver) -> DynamicImportGuard {
    MODULE_STATE.with(|state| state.borrow_mut().dynamic_import = Some(resolve));
    DynamicImportGuard
}

pub struct DynamicImportGuard;

impl Drop for DynamicImportGuard {
    fn drop(&mut self) {
        MODULE_STATE.with(|state| state.borrow_mut().dynamic_import = None);
    }
}

pub fn resolve_dynamic_import(specifier: &str, deferred: bool) -> Option<Value> {
    MODULE_STATE.with(|state| {
        state
            .borrow()
            .dynamic_import
            .as_ref()
            .and_then(|resolve| resolve(specifier, deferred))
    })
}

/// GetModuleExportsList: evaluate a deferred namespace unless the key is
/// symbol-like (`then`, `Symbol.toStringTag`, or an encoded symbol) or private.
pub fn exports(value: &Value, key: &str) -> Result<(), VmError> {
    if skips_deferred_evaluation(key) {
        return Ok(());
    }
    if let Value::BindingCell(cell) = value {
        return exports(&cell.borrow(), key);
    }
    let Value::Object(object) = value else {
        return Ok(());
    };
    let Some(evaluate) =
        MODULE_STATE.with(|state| state.borrow().evaluators.get(&Rc::as_ptr(object)).cloned())
    else {
        return Ok(());
    };
    evaluate();
    if MODULE_STATE
        .with(|state| std::mem::replace(&mut state.borrow_mut().pending_type_error, false))
    {
        return Err(crate::value::error::throw_type_error(
            "deferred namespace is not ready",
        ));
    }
    if let Some(thrown) = MODULE_STATE.with(|state| state.borrow_mut().pending_throw.take()) {
        return Err(crate::execute::VmError::Thrown(thrown));
    }
    Ok(())
}

fn skips_deferred_evaluation(key: &str) -> bool {
    key == "then"
        || key == "Symbol.toStringTag"
        || key.starts_with('#')
        || crate::conversion::is_symbol_string(key)
}

pub fn request_ensure_throw(value: Value) {
    MODULE_STATE.with(|state| state.borrow_mut().pending_throw = Some(value));
}

pub fn take_pending_throw() -> Option<Value> {
    MODULE_STATE.with(|state| state.borrow_mut().pending_throw.take())
}

pub fn mark_await_advanced(advanced: bool) {
    MODULE_STATE.with(|state| state.borrow_mut().await_advanced = advanced);
}

pub fn await_advanced() -> bool {
    MODULE_STATE.with(|state| state.borrow().await_advanced)
}

pub fn has_evaluator(value: &Value) -> bool {
    let Value::Object(object) = unwrap_cells(value) else {
        return false;
    };
    MODULE_STATE.with(|state| state.borrow().evaluators.contains_key(&Rc::as_ptr(&object)))
}

pub fn attach_evaluator(value: &Value, evaluate: Rc<dyn Fn()>) {
    let Value::Object(object) = value else {
        return;
    };
    MODULE_STATE.with(|state| {
        state
            .borrow_mut()
            .evaluators
            .insert(Rc::as_ptr(object), evaluate);
    });
}

pub fn rehome_evaluator(from: &Value, to: &Value) {
    let Value::Object(old) = from else {
        return;
    };
    let Some(evaluate) =
        MODULE_STATE.with(|state| state.borrow().evaluators.get(&Rc::as_ptr(old)).cloned())
    else {
        return;
    };
    attach_evaluator(to, evaluate);
}

pub fn request_ensure_type_error() {
    MODULE_STATE.with(|state| state.borrow_mut().pending_type_error = true);
}

pub fn defer_fulfilled_await(enable: bool) {
    MODULE_STATE.with(|state| state.borrow_mut().defer_fulfilled_await = enable);
}

pub fn fulfilled_await_defers() -> bool {
    MODULE_STATE.with(|state| state.borrow().defer_fulfilled_await)
}

pub fn enqueue_job(job: Rc<dyn Fn()>) {
    crate::promise::enqueue_job(job);
}

pub fn reject_promise(promise: &Rc<crate::value::PromiseData>, reason: Value) {
    crate::promise::reject_promise(promise, reason);
}

const MODULE_NAMESPACE: &str = "\0quench:module_namespace";

pub fn mark_namespace(properties: &mut Vec<(String, Value)>) {
    properties.push((MODULE_NAMESPACE.to_string(), Value::Boolean(true)));
}

pub fn is_namespace(value: &Value) -> bool {
    let Value::Object(properties) = unwrap_cells(value) else {
        return false;
    };
    let result = properties
        .iter()
        .any(|(name, value)| name == MODULE_NAMESPACE && matches!(value, Value::Boolean(true)));
    result
}

fn unwrap_cells(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => unwrap_cells(&cell.borrow()),
        value => value.clone(),
    }
}

pub fn drain_jobs() {
    loop {
        crate::atomics::expire_async_waiters();
        crate::promise::drain_microtasks_all();
        if !crate::promise::has_pending_jobs() {
            let Some(wait) = crate::atomics::next_async_wait_duration() else {
                break;
            };
            // Async Atomics waits are host jobs, not promise reactions. Sleep
            // only until the next finite deadline (or a short poll interval)
            // so expiry queues the reaction without a busy loop.
            std::thread::sleep(wait.min(std::time::Duration::from_millis(1)));
        }
    }
}

pub fn reset_module_jobs() {
    crate::promise::clear_jobs();
    MODULE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.defer_fulfilled_await = false;
        state.pending_type_error = false;
        state.pending_throw = None;
    });
}

/// A live binding shared by module environments.
///
/// Imports and exports observe the same mutable cell rather than copied
/// values. The wrapper keeps module linkage independent from slot storage.
#[derive(Clone)]
pub struct ModuleBindingCell {
    cell: Rc<crate::value::BindingCell>,
}

impl std::fmt::Debug for ModuleBindingCell {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModuleBindingCell")
            .field("value", &self.cell.load())
            .finish()
    }
}

impl PartialEq for ModuleBindingCell {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.cell, &other.cell)
    }
}

impl ModuleBindingCell {
    pub fn new(value: Value) -> Self {
        Self {
            cell: crate::value::BindingCell::new(value),
        }
    }

    pub fn unresolved() -> Self {
        Self::new(Value::Object(Rc::new(crate::value::ObjectData::new(vec![
            (
                "\0quench:unresolved-module-binding".to_string(),
                Value::Boolean(true),
            ),
        ]))))
    }

    pub fn from_shared(cell: Rc<crate::value::BindingCell>) -> Self {
        Self { cell }
    }

    pub fn get(&self) -> Value {
        self.get_with_seen(&mut Vec::new())
    }

    fn get_with_seen(&self, seen: &mut Vec<*const crate::value::BindingCell>) -> Value {
        let pointer = Rc::as_ptr(&self.cell);
        if seen.contains(&pointer) {
            return Value::Undefined;
        }
        seen.push(pointer);
        match self.cell.load() {
            Value::BindingCell(cell) => Self::from_shared(cell).get_with_seen(seen),
            value => value,
        }
    }

    pub fn set(&self, value: Value) {
        self.cell.replace(value);
    }

    pub fn forward_to(&self, target: &Self) {
        self.set(Value::BindingCell(target.shared()));
    }

    pub fn is_unresolved(value: &Value) -> bool {
        let Value::Object(properties) = value else {
            return false;
        };
        properties.iter().any(|(key, value)| {
            key == "\0quench:unresolved-module-binding" && matches!(value, Value::Boolean(true))
        })
    }

    pub fn shared(&self) -> Rc<crate::value::BindingCell> {
        Rc::clone(&self.cell)
    }
}

#[cfg(test)]
mod tests {
    use super::ModuleBindingCell;
    use crate::{environment::Environment, value::Value};

    #[test]
    fn module_aliases_observe_one_live_cell() {
        let cell = ModuleBindingCell::new(Value::Number(1.0));
        let importer = Environment::new();
        let exporter = Environment::new();
        exporter.alias_module_binding("value", cell.clone());
        importer.alias_module_binding("value", cell);

        assert_eq!(importer.resolve_name("value"), Some(Value::Number(1.0)));
        exporter.set_named("value", Value::Number(2.0));
        assert_eq!(importer.resolve_name("value"), Some(Value::Number(2.0)));
    }

    #[test]
    fn exports_evaluates_a_deferred_namespace_once() {
        use std::{cell::Cell, rc::Rc};
        let object = Value::object(Vec::new());
        let hits = Rc::new(Cell::new(0));
        let count = hits.clone();
        super::attach_evaluator(&object, Rc::new(move || count.set(count.get() + 1)));
        super::exports(&object, "foo").expect("exports");
        super::exports(&object, "then").expect("then is symbol-like");
        assert_eq!(hits.get(), 1);
    }
}
