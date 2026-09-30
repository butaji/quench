use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{execute::VmError, value::Value};

/// Host-registered namespace evaluators, keyed by object identity.
type Evaluators = RefCell<HashMap<*const crate::value::ObjectData, Rc<dyn Fn()>>>;
/// Host resolver for `import()` / `import.defer()`.
type DynamicImportResolver = Rc<dyn Fn(&str, bool) -> Option<Value>>;

thread_local! {
    static EVALUATORS: Evaluators = RefCell::new(HashMap::new());
    static PENDING_TYPE_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PENDING_THROW: RefCell<Option<Value>> = const { RefCell::new(None) };
    static DYNAMIC_IMPORT: RefCell<Option<DynamicImportResolver>> =
        const { RefCell::new(None) };
}

/// Host-owned GetModuleNamespace for `import()` / `import.defer()`.
pub fn install_dynamic_import(resolve: DynamicImportResolver) -> DynamicImportGuard {
    DYNAMIC_IMPORT.with(|slot| slot.replace(Some(resolve)));
    DynamicImportGuard
}

pub struct DynamicImportGuard;

impl Drop for DynamicImportGuard {
    fn drop(&mut self) {
        DYNAMIC_IMPORT.with(|slot| slot.replace(None));
    }
}

pub fn resolve_dynamic_import(specifier: &str, deferred: bool) -> Option<Value> {
    DYNAMIC_IMPORT.with(|slot| {
        slot.borrow()
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
    let value = unwrap_cells(value);
    let Value::Object(object) = &value else {
        return Ok(());
    };
    let Some(evaluate) = EVALUATORS.with(|map| map.borrow().get(&Rc::as_ptr(object)).cloned())
    else {
        return Ok(());
    };
    evaluate();
    if PENDING_TYPE_ERROR.with(|flag| flag.replace(false)) {
        return Err(crate::value::error::throw_type_error(
            "deferred namespace is not ready",
        ));
    }
    if let Some(thrown) = PENDING_THROW.with(|slot| slot.borrow_mut().take()) {
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
    PENDING_THROW.with(|slot| *slot.borrow_mut() = Some(value));
}

pub fn take_pending_throw() -> Option<Value> {
    PENDING_THROW.with(|slot| slot.borrow_mut().take())
}

thread_local! {
    static AWAIT_ADVANCED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn mark_await_advanced(advanced: bool) {
    AWAIT_ADVANCED.with(|flag| flag.set(advanced));
}

pub fn await_advanced() -> bool {
    AWAIT_ADVANCED.with(std::cell::Cell::get)
}

pub fn has_evaluator(value: &Value) -> bool {
    let Value::Object(object) = unwrap_cells(value) else {
        return false;
    };
    EVALUATORS.with(|map| map.borrow().contains_key(&Rc::as_ptr(&object)))
}

pub fn attach_evaluator(value: &Value, evaluate: Rc<dyn Fn()>) {
    let Value::Object(object) = value else {
        return;
    };
    EVALUATORS.with(|map| {
        map.borrow_mut().insert(Rc::as_ptr(object), evaluate);
    });
}

pub fn rehome_evaluator(from: &Value, to: &Value) {
    let Value::Object(old) = from else {
        return;
    };
    let Some(evaluate) = EVALUATORS.with(|map| map.borrow().get(&Rc::as_ptr(old)).cloned()) else {
        return;
    };
    attach_evaluator(to, evaluate);
}

pub fn request_ensure_type_error() {
    PENDING_TYPE_ERROR.with(|flag| flag.set(true));
}

thread_local! {
    static DEFER_FULFILLED_AWAIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn defer_fulfilled_await(enable: bool) {
    DEFER_FULFILLED_AWAIT.with(|flag| flag.set(enable));
}

pub fn fulfilled_await_defers() -> bool {
    DEFER_FULFILLED_AWAIT.with(std::cell::Cell::get)
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

pub(crate) fn unwrap_cells(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => ModuleBindingCell::from_shared(Rc::clone(cell)).get(),
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
    defer_fulfilled_await(false);
    PENDING_TYPE_ERROR.with(|flag| flag.set(false));
    PENDING_THROW.with(|slot| slot.replace(None));
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
        let mut cell = Rc::clone(&self.cell);
        let mut seen = std::collections::HashSet::new();
        loop {
            if !seen.insert(Rc::as_ptr(&cell)) {
                return Value::Undefined;
            }
            match cell.load() {
                Value::BindingCell(next) => cell = next,
                value => return value,
            }
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
    fn regression_deep_module_forwarding_preserves_updates_and_breaks_cycles() {
        const FORWARDING_STRESS_DEPTH: usize = 20_000;
        let target = ModuleBindingCell::new(Value::Number(1.0));
        let mut owners = vec![target.clone()];
        for _ in 0..FORWARDING_STRESS_DEPTH {
            let cell = ModuleBindingCell::new(Value::Undefined);
            cell.forward_to(owners.last().unwrap());
            owners.push(cell);
        }
        let importer = owners.last().unwrap();
        assert_eq!(importer.get(), Value::Number(1.0));
        target.set(Value::Number(2.0));
        assert_eq!(importer.get(), Value::Number(2.0));
        let reference = Value::BindingCell(importer.shared());
        assert_eq!(
            crate::construct::peel_construct_value(&reference),
            Value::Number(2.0)
        );
        target.forward_to(importer);
        assert_eq!(importer.get(), Value::Undefined);
        assert_eq!(
            crate::construct::peel_construct_value(&reference),
            Value::Undefined
        );
        target.set(Value::Number(3.0));
        assert_eq!(importer.get(), Value::Number(3.0));
        let object = Value::object(Vec::new());
        target.set(object.clone());
        let updated_target = target.clone();
        super::attach_evaluator(
            &object,
            std::rc::Rc::new(move || {
                updated_target.set(Value::Number(4.0));
            }),
        );
        assert!(super::has_evaluator(&reference));
        super::exports(&reference, "value").expect("deferred alias evaluation");
        assert_eq!(importer.get(), Value::Number(4.0));
        assert!(!super::has_evaluator(&reference));
        let Value::Object(object) = object else {
            unreachable!()
        };
        super::EVALUATORS.with(|map| map.borrow_mut().remove(&std::rc::Rc::as_ptr(&object)));
        drop(reference);
        while owners.pop().is_some() {}
    }

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
