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
        let receiver = function
            .properties
            .borrow()
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "\0super_receiver").then(|| value.clone()))
            .unwrap_or_else(|| receiver.clone());
        let lexical_function = function
            .properties
            .borrow()
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "\0super_function").then(|| value.clone()));
        let context_function = match lexical_function {
            Some(Value::Function(value)) => value,
            _ => Rc::clone(function),
        };
        let current = home.map(|home| Context {
            home,
            receiver,
            function: context_function,
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

pub(crate) fn capture_lexical() -> Option<(Value, Value, Value)> {
    CURRENT.with(|slot| {
        slot.borrow().as_ref().map(|context| {
            (
                context.home.clone(),
                context.receiver.clone(),
                Value::Function(context.function.clone()),
            )
        })
    })
}

pub(crate) fn execute_get(registers: &mut Vec<Value>, op: &crate::ops::Op) -> Result<(), VmError> {
    match op {
        crate::ops::Op::GetSuperProperty { dst, key } => {
            let context = current()?;
            require_initialized_this(&context)?;
            let prototype = require_super_base(&context.home)?;
            trigger_deferred(&prototype, key)?;
            let value = get_with_receiver(&prototype, key, &context.receiver)?;
            crate::execute::write_value(registers, *dst, value);
            Ok(())
        }
        crate::ops::Op::GetSuperPropertyDynamic { dst, key } => {
            let context = current()?;
            require_initialized_this(&context)?;
            let prototype = require_super_base(&context.home)?;
            let key_value = crate::execute::read_register(registers, *key)?;
            let key_string = crate::properties::dynamic_property_key(&key_value)?;
            trigger_deferred(&prototype, &key_string)?;
            let value = get_with_receiver(&prototype, &key_string, &context.receiver)?;
            crate::execute::write_value(registers, *dst, value);
            Ok(())
        }
        _ => Err(VmError::MissingReturn),
    }
}

pub(crate) fn execute_set(registers: &mut [Value], op: &crate::ops::Op) -> Result<(), VmError> {
    match op {
        crate::ops::Op::SetSuperProperty { key, src } => {
            let context = current()?;
            require_initialized_this(&context)?;
            let prototype = require_super_base(&context.home)?;
            let receiver = actual_receiver(&context)?;
            let value = crate::execute::read_register(registers, *src)?.clone();
            trigger_deferred(&prototype, key)?;
            put_with_receiver(&prototype, key, value, &receiver)?;
            Ok(())
        }
        crate::ops::Op::SetSuperPropertyDynamic { key, src } => {
            let context = current()?;
            require_initialized_this(&context)?;
            let prototype = require_super_base(&context.home)?;
            let receiver = actual_receiver(&context)?;
            let key_value = crate::execute::read_register(registers, *key)?;
            let key_string = crate::properties::dynamic_property_key(&key_value)?;
            let value = crate::execute::read_register(registers, *src)?.clone();
            trigger_deferred(&prototype, &key_string)?;
            put_with_receiver(&prototype, &key_string, value, &receiver)?;
            Ok(())
        }
        _ => Err(VmError::MissingReturn),
    }
}

fn trigger_deferred(target: &Value, key: &str) -> Result<(), VmError> {
    if let Some(id) = crate::vm::consume_deferred_namespace_marker(target, key) {
        crate::vm::execute_deferred_module(id)?;
    }
    Ok(())
}

pub(crate) fn execute_call(registers: &mut Vec<Value>, op: &crate::ops::Op) -> Result<(), VmError> {
    if matches!(
        op,
        crate::ops::Op::GetSuperProperty { .. } | crate::ops::Op::GetSuperPropertyDynamic { .. }
    ) {
        return execute_get(registers, op);
    }
    let crate::ops::Op::CallSuperMethod { dst, key, args } = op else {
        return Err(VmError::MissingReturn);
    };
    let context = current()?;
    require_initialized_this(&context)?;
    let prototype = require_super_base(&context.home)?;
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
    let arguments = crate::vm::vm_ops::collect_call_arguments(registers, args, spreads)?;
    let superclass = crate::construct::derived_constructor(&context.function)?;
    let this_slot = context
        .function
        .captures
        .len()
        .saturating_add(usize::from(context.function.params))
        .saturating_add(1);
    let this_slot = u16::try_from(this_slot).map_err(|_| VmError::MissingReturn)?;
    let new_target = crate::locals::current().get(this_slot.saturating_add(1));
    let receiver = crate::construct::construct_super(&superclass, &new_target, &arguments)?;
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

fn current() -> Result<Context, VmError> {
    CURRENT
        .with(|slot| slot.borrow().clone())
        .ok_or_else(super_error)
}

fn require_initialized_this(context: &Context) -> Result<(), VmError> {
    if !crate::functions::is_derived_constructor(&context.function) {
        return Ok(());
    }
    let slot = context
        .function
        .captures
        .len()
        .saturating_add(usize::from(context.function.params))
        .saturating_add(1);
    let slot = u16::try_from(slot).map_err(|_| VmError::MissingReturn)?;
    if crate::locals::current().is_uninitialized(slot) {
        return Err(crate::value::error::throw_reference_error(
            "this is uninitialized",
        ));
    }
    Ok(())
}

fn actual_receiver(context: &Context) -> Result<Value, VmError> {
    if !crate::functions::is_derived_constructor(&context.function) {
        return Ok(context.receiver.clone());
    }
    let slot = context
        .function
        .captures
        .len()
        .saturating_add(usize::from(context.function.params))
        .saturating_add(1);
    let slot = u16::try_from(slot).map_err(|_| VmError::MissingReturn)?;
    Ok(crate::locals::current().get(slot))
}

pub(crate) fn check_initialized_this() -> Result<(), VmError> {
    let context = current()?;
    require_initialized_this(&context)
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

fn put_with_receiver(
    target: &Value,
    key: &str,
    value: Value,
    receiver: &Value,
) -> Result<(), VmError> {
    let receiver = crate::locals::resolved_replacement(receiver.clone());
    if crate::builtins::namespace_uninitialized(&receiver, key) {
        return Err(crate::value::error::throw_reference_error(
            "Cannot access an uninitialized module binding",
        ));
    }
    if matches!(target, Value::Proxy(_)) {
        crate::proxy::proxy_set(target, key, &value, Some(&receiver))?;
        return Ok(());
    }
    if let Some(setter) = crate::property_define::accessor(target, key, "set") {
        if matches!(setter, Value::Undefined) {
            return strict_write_failure();
        }
        crate::functions::execute_target(&setter, &receiver, std::slice::from_ref(&value))?;
        return Ok(());
    }
    if crate::builtins::descriptor_flag(target, key, "writable") == Some(false) {
        return strict_write_failure();
    }
    if crate::properties::rejects_new_property(&receiver, key) {
        return strict_write_failure();
    }
    let result = crate::builtins::set_property(receiver.clone(), key, value);
    crate::locals::replace_value(&receiver, &result);
    Ok(())
}

fn strict_write_failure() -> Result<(), VmError> {
    if is_strict() {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    Ok(())
}

/// Read the [[HomeObject]][[Prototype]] for `super`. Throws TypeError when
/// the home's prototype is null/undefined (Spec: RequireObjectCoercible).
fn require_super_base(home: &Value) -> Result<Value, VmError> {
    let prototype = match home {
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map(Value::Object)
            .unwrap_or(Value::Null),
        home => home.clone(),
    };
    let prototype = crate::builtins::object::get_prototype_of(Some(&prototype))?;
    match prototype {
        Value::Null | Value::Undefined => Err(crate::value::error::throw_type_error(
            "Super has no prototype",
        )),
        _ => Ok(prototype),
    }
}

pub(crate) fn attach_home_objects(value: &Value) {
    match value {
        Value::Object(object) => {
            let alias = Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(
                object,
            )))));
            for (_, property) in object.iter() {
                attach(property, &alias);
            }
        }
        Value::Function(function) => {
            let home = Value::Function(Rc::clone(function));
            let properties = function.properties.borrow().clone();
            for (_, property) in &properties {
                attach(property, &home);
            }
        }
        _ => {}
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
