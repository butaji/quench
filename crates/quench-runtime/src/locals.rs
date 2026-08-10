use std::{cell::RefCell, rc::Rc};

use crate::{environment::Environment, execute::VmError, value::Value};

thread_local! {
    static CURRENT_ENVIRONMENT: RefCell<Option<Rc<Environment>>> = const { RefCell::new(None) };
}

pub(crate) struct EnvironmentGuard {
    previous: Option<Rc<Environment>>,
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
        Self { previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        CURRENT_ENVIRONMENT.with(|current| current.replace(self.previous.take()));
    }
}

pub(crate) fn current() -> Rc<Environment> {
    CURRENT_ENVIRONMENT
        .with(|current| current.borrow().clone())
        .unwrap_or_default()
}

pub(crate) fn is_installed() -> bool {
    CURRENT_ENVIRONMENT.with(|current| current.borrow().is_some())
}

pub(crate) fn store(registers: &[Value], slot: u16, source: u16) -> Result<(), VmError> {
    let value = crate::execute::read_register(registers, source)?;
    current().set(slot, value);
    Ok(())
}

pub(crate) fn load(registers: &mut Vec<Value>, dst: u16, slot: u16) {
    crate::execute::write_value(registers, dst, current().get(slot));
}

pub(crate) fn write(slot: u16, value: Value) {
    current().set(slot, value);
}

pub(crate) fn capture(count: u16) -> Rc<Environment> {
    Environment::capture(&current(), count)
}

pub(crate) fn replace_value(old: &Value, new: &Value) {
    current().replace_value(old, new);
}
