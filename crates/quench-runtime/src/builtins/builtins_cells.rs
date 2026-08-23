use std::rc::Rc;

use crate::value::{ObjectData, Value};

pub(super) fn set_object_property(properties: Rc<ObjectData>, key: &str, value: Value) -> Value {
    let cell = properties
        .iter()
        .rev()
        .find_map(|(name, current)| (name == key).then_some(current))
        .and_then(binding_cell)
        .or_else(|| deleted_binding_cell(&properties, key));
    let Some(cell) = cell else {
        return super::object_alias::set(properties, key, value);
    };
    if !has_binding_cell_property(&properties, key) {
        let mut values = properties.properties.clone();
        values.retain(|(name, _)| name != &crate::builtins::deleted_key(key));
        if let Some((_, current)) = values.iter_mut().rev().find(|(name, _)| name == key) {
            *current = Value::BindingCell(Rc::clone(&cell));
        } else {
            values.push((key.into(), Value::BindingCell(Rc::clone(&cell))));
        }
        let mut created = properties.created.clone();
        crate::builtins::object_alias::record_created(&mut created, key);
        let properties = Rc::new(ObjectData::with_creation_order(
            values,
            Rc::clone(&properties.private_slots),
            created,
        ));
        *cell.borrow_mut() = public_value(value);
        return Value::Object(properties);
    }
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

fn deleted_binding_cell(
    properties: &ObjectData,
    key: &str,
) -> Option<Rc<std::cell::RefCell<Value>>> {
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == &crate::builtins::deleted_key(key)).then_some(value))
        .and_then(binding_cell)
}

fn has_binding_cell_property(properties: &ObjectData, key: &str) -> bool {
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then_some(value))
        .and_then(binding_cell)
        .is_some()
}
