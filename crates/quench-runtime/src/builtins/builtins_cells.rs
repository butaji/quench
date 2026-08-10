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
    *cell.borrow_mut() = value;
    Value::Object(properties)
}

fn binding_cell(value: &Value) -> Option<Rc<std::cell::RefCell<Value>>> {
    match value {
        Value::BindingCell(cell) => Some(Rc::clone(cell)),
        _ => None,
    }
}
