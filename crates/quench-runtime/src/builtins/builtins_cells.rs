use std::rc::Rc;

use crate::value::Value;

pub(super) fn set_object_property(
    properties: Rc<Vec<(String, Value)>>,
    key: &str,
    value: Value,
) -> Value {
    let cell = properties
        .iter()
        .rev()
        .find_map(|(name, current)| (name == key).then_some(current))
        .and_then(binding_cell);
    let Some(cell) = cell else {
        return super::object_alias::set(properties, key, value);
    };
    if !same_binding_cell(&cell, &value) {
        *cell.borrow_mut() = public_value(value);
    }
    Value::Object(properties)
}

fn same_binding_cell(cell: &Rc<std::cell::RefCell<Value>>, value: &Value) -> bool {
    matches!(value, Value::BindingCell(value_cell) if Rc::ptr_eq(cell, value_cell))
}

fn public_value(value: Value) -> Value {
    match value {
        Value::BindingCell(cell) => cell.borrow().clone(),
        value => value,
    }
}

fn binding_cell(value: &Value) -> Option<Rc<std::cell::RefCell<Value>>> {
    match value {
        Value::BindingCell(cell) => Some(Rc::clone(cell)),
        _ => None,
    }
}
