use std::{cell::RefCell, rc::Rc};

use crate::{execute::VmError, ops::Op, value::Value};

struct Descriptor {
    value: Value,
    writable: bool,
    enumerable: bool,
    configurable: bool,
    data: bool,
}

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    match op {
        Op::CheckGlobalFunction { name } => check_function(name),
        Op::CheckGlobalVar { name } => check_var(name),
        Op::CreateGlobalFunction {
            name,
            slot,
            deletable,
        } => create_function(registers, name, *slot, *deletable),
        Op::CreateGlobalVar {
            name,
            slot,
            deletable,
        } => create_var(registers, name, *slot, *deletable),
        _ => Ok(()),
    }
}

fn check_function(name: &str) -> Result<(), VmError> {
    let Some(descriptor) = own_descriptor(name) else {
        return Ok(());
    };
    let compatible =
        descriptor.configurable || descriptor.data && descriptor.writable && descriptor.enumerable;
    if compatible {
        Ok(())
    } else {
        Err(crate::value::error::throw_type_error(&format!(
            "Cannot declare global function '{name}'"
        )))
    }
}

fn check_var(_name: &str) -> Result<(), VmError> {
    // The current global representation is always extensible.
    Ok(())
}

fn create_var(
    registers: &mut Vec<Value>,
    name: &str,
    slot: u16,
    deletable: bool,
) -> Result<(), VmError> {
    let current = own_descriptor(name);
    let cell = binding_cell(name, slot, current.as_ref().map(|value| &value.value));
    let descriptor = match current {
        Some(current) => descriptor_with_flags(cell, &current),
        None => data_descriptor(cell, true, true, deletable),
    };
    define_global(registers, name, descriptor)
}

fn create_function(
    registers: &mut Vec<Value>,
    name: &str,
    slot: u16,
    deletable: bool,
) -> Result<(), VmError> {
    let current = own_descriptor(name);
    let value = crate::locals::slot_cell(slot).borrow().clone();
    let cell = binding_cell(name, slot, Some(&value));
    let descriptor = match current {
        Some(current) if !current.configurable => value_descriptor(cell),
        _ => data_descriptor(cell, true, true, deletable),
    };
    define_global(registers, name, descriptor)
}

fn binding_cell(name: &str, slot: u16, value: Option<&Value>) -> Rc<RefCell<Value>> {
    let cell = raw_binding_cell(name).unwrap_or_else(|| crate::locals::slot_cell(slot));
    if let Some(value) = value {
        *cell.borrow_mut() = value.clone();
    }
    crate::locals::install_slot_cell(slot, Rc::clone(&cell));
    cell
}

fn raw_binding_cell(name: &str) -> Option<Rc<RefCell<Value>>> {
    let Value::Object(properties) = crate::vm::current_global_object() else {
        return None;
    };
    properties.iter().rev().find_map(|(key, value)| {
        if key != name {
            return None;
        }
        match value {
            Value::BindingCell(cell) => Some(Rc::clone(cell)),
            _ => None,
        }
    })
}

fn own_descriptor(name: &str) -> Option<Descriptor> {
    let global = crate::vm::current_global_object();
    let descriptor =
        crate::builtins::object::descriptor(Some(&global), Some(&Value::String(name.to_string())));
    let Value::Object(fields) = descriptor else {
        return immutable_descriptor(name);
    };
    let immutable = crate::globals::immutable_value(name).is_some();
    Some(Descriptor {
        value: field(&fields, "value").unwrap_or(Value::Undefined),
        writable: !immutable && flag(&fields, "writable"),
        enumerable: !immutable && flag(&fields, "enumerable"),
        configurable: !immutable && flag(&fields, "configurable"),
        data: has_field(&fields, "value") || has_field(&fields, "writable"),
    })
}

fn immutable_descriptor(name: &str) -> Option<Descriptor> {
    Some(Descriptor {
        value: crate::globals::immutable_value(name)?,
        writable: false,
        enumerable: false,
        configurable: false,
        data: true,
    })
}

fn value_descriptor(cell: Rc<RefCell<Value>>) -> Vec<(String, Value)> {
    vec![("value".to_string(), Value::BindingCell(cell))]
}

fn descriptor_with_flags(cell: Rc<RefCell<Value>>, current: &Descriptor) -> Vec<(String, Value)> {
    data_descriptor(
        cell,
        current.writable,
        current.enumerable,
        current.configurable,
    )
}

fn data_descriptor(
    cell: Rc<RefCell<Value>>,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Vec<(String, Value)> {
    vec![
        ("value".to_string(), Value::BindingCell(cell)),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(enumerable)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ]
}

fn define_global(
    registers: &mut Vec<Value>,
    name: &str,
    descriptor: Vec<(String, Value)>,
) -> Result<(), VmError> {
    let global = crate::vm::current_global_object();
    let updated = crate::builtins::define_own_property(&global, name, &descriptor)?;
    crate::vm::synchronize_global_object(registers, &global, &updated);
    Ok(())
}

fn field(fields: &[(String, Value)], name: &str) -> Option<Value> {
    fields
        .iter()
        .rev()
        .find_map(|(key, value)| (key == name).then(|| value.clone()))
}

fn has_field(fields: &[(String, Value)], name: &str) -> bool {
    fields.iter().any(|(key, _)| key == name)
}

fn flag(fields: &[(String, Value)], name: &str) -> bool {
    matches!(field(fields, name), Some(Value::Boolean(true)))
}
