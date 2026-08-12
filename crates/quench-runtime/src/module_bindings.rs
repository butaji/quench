use std::{cell::RefCell, rc::Rc};

use crate::value::Value;

/// A live binding shared by module environments.
///
/// Imports and exports observe the same mutable cell rather than copied
/// values. The wrapper keeps module linkage independent from slot storage.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModuleBindingCell(Rc<RefCell<Value>>);

impl ModuleBindingCell {
    pub(crate) fn new(value: Value) -> Self {
        Self(Rc::new(RefCell::new(value)))
    }

    pub(crate) fn from_shared(cell: Rc<RefCell<Value>>) -> Self {
        Self(cell)
    }

    pub(crate) fn get(&self) -> Value {
        self.0.borrow().clone()
    }

    pub(crate) fn set(&self, value: Value) {
        self.0.replace(value);
    }

    pub(crate) fn shared(&self) -> Rc<RefCell<Value>> {
        Rc::clone(&self.0)
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
}
