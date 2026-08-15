use std::{cell::RefCell, rc::Rc};

use crate::value::Value;

/// A live binding shared by module environments.
///
/// Imports and exports observe the same mutable cell rather than copied
/// values. The wrapper keeps module linkage independent from slot storage.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleBindingCell(Rc<RefCell<Value>>);

impl ModuleBindingCell {
    pub fn new(value: Value) -> Self {
        Self(Rc::new(RefCell::new(value)))
    }

    pub fn unresolved() -> Self {
        Self::new(Value::Object(Rc::new(crate::value::ObjectData::new(vec![
            (
                "\0quench:unresolved-module-binding".to_string(),
                Value::Boolean(true),
            ),
        ]))))
    }

    pub fn from_shared(cell: Rc<RefCell<Value>>) -> Self {
        Self(cell)
    }

    pub fn get(&self) -> Value {
        self.get_with_seen(&mut Vec::new())
    }

    fn get_with_seen(&self, seen: &mut Vec<*const RefCell<Value>>) -> Value {
        let pointer = Rc::as_ptr(&self.0);
        if seen.contains(&pointer) {
            return Value::Undefined;
        }
        seen.push(pointer);
        match self.0.borrow().clone() {
            Value::BindingCell(cell) => Self::from_shared(cell).get_with_seen(seen),
            value => value,
        }
    }

    pub fn set(&self, value: Value) {
        self.0.replace(value);
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

    pub fn shared(&self) -> Rc<RefCell<Value>> {
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
