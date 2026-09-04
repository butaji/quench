use std::rc::Rc;

use crate::value::{ObjectData, Value};

pub(super) fn set_object_property(properties: Rc<ObjectData>, key: &str, value: Value) -> Value {
    crate::cycle_collector::track_object(&properties);
    crate::cycle_collector::track_value(&value);
    let cell = properties
        .iter()
        .rev()
        .find_map(|(name, current)| (name == key).then_some(current))
        .and_then(|value| binding_cell(&value))
        .or_else(|| deleted_binding_cell(&properties, key));
    let Some(cell) = cell else {
        let result = super::object_alias::set(properties, key, value);
        crate::cycle_collector::track_value(&result);
        crate::cycle_collector::checkpoint();
        return result;
    };
    if !has_binding_cell_property(&properties, key) {
        let mut values = properties.properties.clone();
        values.retain(|(name, _)| name != &crate::builtins::deleted_key(key));
        if let Some(slot) = values.position_rev(key) {
            values.store_slot(slot, Value::BindingCell(Rc::clone(&cell)));
        } else {
            values.push((key.into(), Value::BindingCell(Rc::clone(&cell))));
        }
        let mut created = properties.creation_order_values();
        crate::builtins::object_alias::record_created(&mut created, key);
        let properties = Rc::new(ObjectData::with_creation_order(
            values,
            Rc::clone(&properties.private_slots),
            created,
        ));
        cell.store(public_value(value));
        let result = Value::Object(properties);
        crate::cycle_collector::checkpoint();
        return result;
    }
    if !same_binding_cell(&cell, &value) {
        cell.store(public_value(value));
    }
    let result = Value::Object(properties);
    crate::cycle_collector::checkpoint();
    result
}

fn same_binding_cell(cell: &Rc<crate::value::BindingCell>, value: &Value) -> bool {
    matches!(value, Value::BindingCell(value_cell) if Rc::ptr_eq(cell, value_cell))
}

fn public_value(value: Value) -> Value {
    match value {
        Value::BindingCell(cell) => cell.load(),
        value => value,
    }
}

fn binding_cell(value: &Value) -> Option<Rc<crate::value::BindingCell>> {
    match value {
        Value::BindingCell(cell) => Some(Rc::clone(cell)),
        _ => None,
    }
}

fn deleted_binding_cell(
    properties: &ObjectData,
    key: &str,
) -> Option<Rc<crate::value::BindingCell>> {
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == &crate::builtins::deleted_key(key)).then_some(value))
        .and_then(|value| binding_cell(&value))
}

fn has_binding_cell_property(properties: &ObjectData, key: &str) -> bool {
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then_some(value))
        .and_then(|value| binding_cell(&value))
        .is_some()
}
