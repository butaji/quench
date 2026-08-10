use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    facts::PrivateNameId,
    ops::Op,
    value::{PrivateSlot, Value},
};

type Slots = Rc<RefCell<Vec<(PrivateNameId, PrivateSlot)>>>;

pub(crate) fn get(value: &Value, name: PrivateNameId) -> Result<Value, VmError> {
    let slots = slots(value)?;
    let slots = slots.borrow();
    let Some((_, slot)) = slots.iter().find(|(id, _)| *id == name) else {
        return Err(private_brand_error());
    };
    match slot {
        PrivateSlot::Data(value) => Ok(value.clone()),
        PrivateSlot::Accessor { .. } => Err(private_brand_error()),
    }
}

pub(crate) fn set(value: &Value, name: PrivateNameId, new_value: Value) -> Result<(), VmError> {
    let slots = slots(value)?;
    let mut slots = slots.borrow_mut();
    let Some((_, PrivateSlot::Data(value))) = slots.iter_mut().find(|(id, _)| *id == name) else {
        return Err(private_brand_error());
    };
    *value = new_value;
    Ok(())
}

/// Defines an unforgeable data slot without exposing it as an ordinary key.
pub(crate) fn define(value: &Value, name: PrivateNameId, initial: Value) -> Result<(), VmError> {
    let slots = slots(value)?;
    let mut slots = slots.borrow_mut();
    if slots.iter().any(|(id, _)| *id == name) {
        return Err(private_brand_error());
    }
    slots.push((name, PrivateSlot::Data(initial)));
    Ok(())
}

pub(crate) fn execute_get(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::GetPrivate { dst, object, name } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let value = get(&object, *name)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn execute_set(registers: &mut [Value], op: &Op) -> Result<(), VmError> {
    let Op::SetPrivate { object, name, src } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let value = crate::execute::read_register(registers, *src)?;
    set(&object, *name, value)
}

pub(crate) fn execute_define(registers: &mut [Value], op: &Op) -> Result<(), VmError> {
    let Op::DefinePrivate { object, name, src } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let value = crate::execute::read_register(registers, *src)?;
    define(&object, *name, value)
}

fn slots(value: &Value) -> Result<Slots, VmError> {
    match value {
        Value::BindingCell(cell) => slots(&cell.borrow()),
        Value::Function(function) => Ok(function.private_slots.clone()),
        _ => Err(private_brand_error()),
    }
}

fn private_brand_error() -> VmError {
    crate::value::error::throw_type_error(
        "Private field access on an object without the required brand",
    )
}
