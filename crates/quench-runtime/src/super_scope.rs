use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use crate::{
    execute::VmError,
    value::{ObjectAliasValue, Value},
};

thread_local! {
    static CURRENT: RefCell<Option<Context>> = const { RefCell::new(None) };
    static STRICT: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone)]
struct Context {
    home: Value,
    receiver: Value,
    function: Rc<crate::value::FunctionValue>,
}

pub(crate) struct Guard {
    previous: Option<Context>,
    previous_strict: bool,
}

impl Guard {
    pub(crate) fn install(function: &Rc<crate::value::FunctionValue>, receiver: &Value) -> Self {
        let home = function
            .properties
            .borrow()
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "\0home_object").then(|| value.clone()));
        let current = home.map(|home| Context {
            home,
            receiver: receiver.clone(),
            function: Rc::clone(function),
        });
        let current = current.or_else(|| CURRENT.with(|slot| slot.borrow().clone()));
        let previous = CURRENT.with(|slot| slot.replace(current));
        let strict = matches!(function.strictness, crate::ops::FunctionStrictness::Strict);
        let previous_strict = STRICT.with(|slot| slot.replace(strict));
        Self {
            previous,
            previous_strict,
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.with(|slot| slot.replace(self.previous.take()));
        STRICT.with(|slot| slot.set(self.previous_strict));
    }
}

pub(crate) fn is_active() -> bool {
    CURRENT.with(|slot| slot.borrow().is_some())
}

pub(crate) fn is_strict() -> bool {
    STRICT.with(Cell::get)
}

pub(crate) fn execute_get(registers: &mut Vec<Value>, op: &crate::ops::Op) -> Result<(), VmError> {
    let crate::ops::Op::GetSuperProperty { dst, key } = op else {
        return Err(VmError::MissingReturn);
    };
    let context = current()?;
    let prototype = crate::execute::get_property(&context.home, "\0prototype");
    let value = get_with_receiver(&prototype, key, &context.receiver)?;
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
    let context = current()?;
    let prototype = crate::execute::get_property(&context.home, "\0prototype");
    let callee = get_with_receiver(&prototype, key, &context.receiver)?;
    let arguments = args
        .iter()
        .map(|index| crate::execute::read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let value = crate::functions::execute_target(&callee, &context.receiver, &arguments)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn execute_constructor(
    registers: &mut Vec<Value>,
    op: &crate::ops::Op,
) -> Result<(), VmError> {
    let crate::ops::Op::CallSuperConstructor { dst, args, spreads } = op else {
        return Err(VmError::MissingReturn);
    };
    let context = current()?;
    let arguments = call_arguments(registers, args, spreads)?;
    let superclass = crate::construct::derived_constructor(&context.function)?;
    let receiver = crate::construct::construct_super(&superclass, &context.function, &arguments)?;
    let this_slot = context
        .function
        .captures
        .len()
        .saturating_add(usize::from(context.function.params))
        .saturating_add(1);
    let this_slot = u16::try_from(this_slot).map_err(|_| VmError::MissingReturn)?;
    if !crate::locals::current().is_uninitialized(this_slot) {
        return Err(crate::value::error::throw_reference_error(
            "Super constructor may only be called once",
        ));
    }
    crate::locals::write(this_slot, receiver.clone());
    let receiver = crate::construct::initialize_instance_fields(&context.function, receiver)?;
    crate::locals::write(this_slot, receiver.clone());
    crate::execute::write_value(registers, *dst, receiver);
    Ok(())
}

fn call_arguments(
    registers: &[Value],
    args: &[u16],
    spreads: &[bool],
) -> Result<Vec<Value>, VmError> {
    let mut arguments = Vec::new();
    for (index, spread) in args.iter().zip(spreads) {
        let value = crate::execute::read_register(registers, *index)?;
        match (spread, value) {
            (true, Value::Array(values)) => arguments.extend(values.iter().cloned()),
            (_, value) => arguments.push(value),
        }
    }
    Ok(arguments)
}

fn current() -> Result<Context, VmError> {
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
