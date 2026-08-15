use crate::{
    execute::VmError,
    ops::Op,
    value::{PrivateName, PrivateSlot, PrivateSlots, Value},
};

pub(crate) fn get(value: &Value, name: &PrivateName) -> Result<Value, VmError> {
    let slots = slots(value)?;
    let slot = slots
        .borrow()
        .iter()
        .find(|(id, _)| id == name)
        .map(|(_, slot)| slot.clone());
    let Some(slot) = slot else {
        return Err(private_brand_error());
    };
    match slot {
        PrivateSlot::Data(value) => Ok(value),
        PrivateSlot::Method(value) => Ok(value),
        PrivateSlot::Accessor { get: Some(get), .. } => {
            crate::functions::execute_target(&get, value, &[])
        }
        PrivateSlot::Accessor { get: None, .. } => Err(private_brand_error()),
    }
}

pub(crate) fn execute_has(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::HasPrivate { dst, object, name } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let name = resolve(*name)?;
    let result = slots(&object)?
        .borrow()
        .iter()
        .any(|(candidate, _)| candidate == &name);
    crate::execute::write_value(registers, *dst, Value::Boolean(result));
    Ok(())
}

pub(crate) fn define_accessor(
    value: &Value,
    name: PrivateName,
    get: Option<Value>,
    set: Option<Value>,
) -> Result<(), VmError> {
    let slots = slots(value)?;
    let mut slots = slots.borrow_mut();
    if let Some((
        _,
        PrivateSlot::Accessor {
            get: old_get,
            set: old_set,
        },
    )) = slots.iter_mut().find(|(id, _)| id == &name)
    {
        if get.is_some() {
            *old_get = get;
        }
        if set.is_some() {
            *old_set = set;
        }
        return Ok(());
    }
    if slots.iter().any(|(id, _)| id == &name) {
        return Err(private_brand_error());
    }
    slots.push((name, PrivateSlot::Accessor { get, set }));
    Ok(())
}

pub(crate) fn define_method(
    value: &Value,
    name: PrivateName,
    method: Value,
) -> Result<(), VmError> {
    define_slot(value, name, PrivateSlot::Method(method))
}

pub(crate) fn set(value: &Value, name: &PrivateName, new_value: Value) -> Result<(), VmError> {
    let slots = slots(value)?;
    let slot = slots
        .borrow()
        .iter()
        .find(|(id, _)| id == name)
        .map(|(_, slot)| slot.clone());
    let Some(slot) = slot else {
        return Err(private_brand_error());
    };
    set_slot(value, name, new_value, slot)
}

fn set_slot(
    value: &Value,
    name: &PrivateName,
    new_value: Value,
    slot: PrivateSlot,
) -> Result<(), VmError> {
    match slot {
        PrivateSlot::Data(_) => set_data_slot(value, name, new_value),
        PrivateSlot::Method(_) => Err(private_brand_error()),
        PrivateSlot::Accessor { set: Some(set), .. } => {
            crate::functions::execute_target(&set, value, &[new_value]).map(|_| ())
        }
        PrivateSlot::Accessor { set: None, .. } => Err(private_brand_error()),
    }
}

fn set_data_slot(value: &Value, name: &PrivateName, new_value: Value) -> Result<(), VmError> {
    let slots = slots(value)?;
    let mut slots = slots.borrow_mut();
    let Some((_, PrivateSlot::Data(old_value))) = slots.iter_mut().find(|(id, _)| id == name)
    else {
        return Err(private_brand_error());
    };
    *old_value = new_value;
    Ok(())
}

/// Defines an unforgeable data slot without exposing it as an ordinary key.
pub(crate) fn define(value: &Value, name: PrivateName, initial: Value) -> Result<(), VmError> {
    define_slot(value, name, PrivateSlot::Data(initial))
}

fn define_slot(value: &Value, name: PrivateName, slot: PrivateSlot) -> Result<(), VmError> {
    if crate::builtins::is_module_namespace(value) {
        return Err(private_brand_error());
    }
    let slots = slots(value)?;
    let mut slots = slots.borrow_mut();
    if slots.iter().any(|(id, _)| id == &name) {
        return Err(private_brand_error());
    }
    slots.push((name, slot));
    Ok(())
}

pub(crate) fn execute_get(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    let Op::GetPrivate { dst, object, name } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let name = resolve(*name)?;
    let value = get(&object, &name)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn execute_set(registers: &mut [Value], op: &Op) -> Result<(), VmError> {
    let Op::SetPrivate { object, name, src } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let value = crate::execute::read_register(registers, *src)?;
    let name = resolve(*name)?;
    set(&object, &name, value)
}

pub(crate) fn execute_define(registers: &mut [Value], op: &Op) -> Result<(), VmError> {
    let Op::DefinePrivate { object, name, src } = op else {
        return Err(VmError::MissingReturn);
    };
    let object = crate::execute::read_register(registers, *object)?;
    let value = crate::execute::read_register(registers, *src)?;
    define(&object, resolve(*name)?, value)
}

fn resolve(name: crate::facts::PrivateNameId) -> Result<PrivateName, VmError> {
    crate::private_environment::resolve(name).ok_or_else(private_brand_error)
}

fn slots(value: &Value) -> Result<PrivateSlots, VmError> {
    match value {
        Value::BindingCell(cell) => slots(&cell.borrow()),
        Value::Function(function) => Ok(function.private_slots.clone()),
        Value::Object(object) => Ok(object.private_slots.clone()),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map(|object| object.private_slots.clone())
            .ok_or_else(private_brand_error),
        _ => Err(private_brand_error()),
    }
}

fn private_brand_error() -> VmError {
    crate::value::error::throw_type_error(
        "Private field access on an object without the required brand",
    )
}
