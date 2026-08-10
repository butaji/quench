use std::{cell::RefCell, rc::Rc};

use crate::{
    execute::VmError,
    value::{ObjectAliasValue, Value},
};

thread_local! {
    static CURRENT: RefCell<Option<(Value, Value)>> = const { RefCell::new(None) };
}

pub(crate) struct Guard {
    previous: Option<(Value, Value)>,
}

impl Guard {
    pub(crate) fn install(function: &crate::value::FunctionValue, receiver: &Value) -> Self {
        let home = function
            .properties
            .borrow()
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "\0home_object").then(|| value.clone()));
        let current = home.map(|home| (home, receiver.clone()));
        let previous = CURRENT.with(|slot| slot.replace(current));
        Self { previous }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.with(|slot| slot.replace(self.previous.take()));
    }
}

pub(crate) fn is_active() -> bool {
    CURRENT.with(|slot| slot.borrow().is_some())
}

pub(crate) fn execute_get(registers: &mut Vec<Value>, op: &crate::ops::Op) -> Result<(), VmError> {
    let crate::ops::Op::GetSuperProperty { dst, key } = op else {
        return Err(VmError::MissingReturn);
    };
    let (home, receiver) = current()?;
    let prototype = crate::execute::get_property(&home, "\0prototype");
    let value = get_with_receiver(&prototype, key, &receiver)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn execute_call(registers: &mut Vec<Value>, op: &crate::ops::Op) -> Result<(), VmError> {
    if matches!(op, crate::ops::Op::GetSuperProperty { .. }) {
        return execute_get(registers, op);
    }
    let crate::ops::Op::CallSuperMethod { dst, key, args } = op else {
        return Err(VmError::MissingReturn);
    };
    let (home, receiver) = current()?;
    let prototype = crate::execute::get_property(&home, "\0prototype");
    let callee = get_with_receiver(&prototype, key, &receiver)?;
    let arguments = args
        .iter()
        .map(|index| crate::execute::read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let value = crate::functions::execute_target(&callee, &receiver, &arguments)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

fn current() -> Result<(Value, Value), VmError> {
    CURRENT
        .with(|slot| slot.borrow().clone())
        .ok_or_else(super_error)
}

fn get_with_receiver(target: &Value, key: &str, receiver: &Value) -> Result<Value, VmError> {
    if matches!(target, Value::Proxy(_)) {
        return crate::proxy::proxy_get(target, key, Some(receiver));
    }
    let Some(getter) = crate::property_define::accessor(target, key, "get") else {
        return Ok(crate::execute::get_property(target, key));
    };
    if matches!(getter, Value::Undefined) {
        return Ok(Value::Undefined);
    }
    crate::functions::execute_target(&getter, receiver, &[])
}

pub(crate) fn attach_home_objects(value: &Value) {
    let Value::Object(object) = value else { return };
    let alias = Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(
        object,
    )))));
    for (_, property) in object.iter() {
        attach(property, &alias);
    }
}

fn attach(value: &Value, home: &Value) {
    match value {
        Value::Function(function) => {
            let mut properties = function.properties.borrow_mut();
            properties.retain(|(name, _)| name != "\0home_object");
            properties.push(("\0home_object".to_string(), home.clone()));
        }
        Value::Object(properties) => {
            for (name, value) in properties.iter() {
                if matches!(name.as_str(), "get" | "set") {
                    attach(value, home);
                }
            }
        }
        _ => {}
    }
}

fn super_error() -> VmError {
    crate::value::error::throw_reference_error("super has no home object")
}
