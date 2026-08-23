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
            let prototype = require_context_super_base(&context)?;
            let value = get_with_receiver(&prototype, key, &context.receiver)?;
            crate::execute::write_value(registers, *dst, value);
            Ok(())
        }
        crate::ops::Op::GetSuperPropertyDynamic { dst, key, base } => {
            let context = current()?;
            require_initialized_this(&context)?;
            let prototype = super_base(&context, registers, *base)?;
            let key_value = crate::execute::read_register(registers, *key)?;
            let key_string = crate::properties::dynamic_property_key(&key_value)?;
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
            let prototype = require_context_super_base(&context)?;
            let value = crate::execute::read_register(registers, *src)?.clone();
            let receiver = active_this(&context)?;
            put_with_receiver(&prototype, key, value, &receiver)?;
            Ok(())
        }
        crate::ops::Op::SetSuperPropertyDynamic { key, src, base } => {
            let context = current()?;
            require_initialized_this(&context)?;
            let prototype = super_base(&context, registers, *base)?;
            let key_value = crate::execute::read_register(registers, *key)?;
            let key_string = crate::properties::dynamic_property_key(&key_value)?;
            let value = crate::execute::read_register(registers, *src)?.clone();
            let receiver = active_this(&context)?;
            put_with_receiver(&prototype, &key_string, value, &receiver)?;
            Ok(())
        }
        _ => Err(VmError::MissingReturn),
    }
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
    let prototype = require_context_super_base(&context)?;
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
    if !super_is_constructor(&superclass) {
        return Err(crate::value::error::throw_type_error(
            "Super constructor is not a constructor",
        ));
    }
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

fn super_is_constructor(value: &Value) -> bool {
    match value {
        Value::Function(function) => crate::functions::is_constructible(function),
        Value::BoundFunction(bound) => super_is_constructor(&bound.target),
        Value::Builtin(builtin) => crate::builtin_meta::constructor_name(*builtin).is_some(),
        Value::Proxy(proxy) => super_is_constructor(&proxy.target),
        _ => false,
    }
}

fn current() -> Result<Context, VmError> {
    CURRENT
        .with(|slot| slot.borrow().clone())
        .ok_or_else(super_error)
}

fn derived_this_slot(function: &crate::value::FunctionValue) -> Result<u16, VmError> {
    let slot = function
        .captures
        .len()
        .saturating_add(usize::from(function.params))
        .saturating_add(1);
    u16::try_from(slot).map_err(|_| VmError::MissingReturn)
}

fn require_initialized_this(context: &Context) -> Result<(), VmError> {
    if !crate::functions::is_derived_constructor(&context.function) {
        return Ok(());
    }
    let slot = derived_this_slot(&context.function)?;
    if crate::locals::current().is_uninitialized(slot) {
        return Err(crate::value::error::throw_reference_error(
            "this is uninitialized",
        ));
    }
    Ok(())
}

fn active_this(context: &Context) -> Result<Value, VmError> {
    if !crate::functions::is_derived_constructor(&context.function) {
        return Ok(context.receiver.clone());
    }
    let slot = derived_this_slot(&context.function)?;
    Ok(crate::locals::current().get(slot))
}

pub(crate) fn check_initialized_this() -> Result<(), VmError> {
    let context = current()?;
    require_initialized_this(&context)
}

pub(crate) fn execute_capture_base(registers: &mut Vec<Value>, dst: u16) -> Result<(), VmError> {
    let context = current()?;
    require_initialized_this(&context)?;
    let prototype = require_context_super_base(&context)?;
    crate::execute::write_value(registers, dst, prototype);
    Ok(())
}

fn super_base(context: &Context, registers: &[Value], base: Option<u16>) -> Result<Value, VmError> {
    match base {
        Some(slot) => crate::execute::read_register(registers, slot),
        None => require_context_super_base(context),
    }
}

fn get_with_receiver(target: &Value, key: &str, receiver: &Value) -> Result<Value, VmError> {
    let mut current = crate::locals::resolved_replacement(target.clone());
    loop {
        // Module namespace [[Get]]: GetModuleExportsList(O) before own lookup.
        crate::module_bindings::exports(&current, key)?;
        if matches!(current, Value::Proxy(_)) {
            return crate::proxy::proxy_get(&current, key, Some(receiver));
        }
        if let Some(value) = super_own_value(&current, key, receiver)? {
            return Ok(value);
        }
        let prototype = crate::builtins::object::get_prototype_of(Some(&current))?;
        let prototype = crate::locals::resolved_replacement(prototype);
        if matches!(prototype, Value::Null | Value::Undefined)
            || crate::equality::strict_equal(&prototype, &current)
        {
            return Ok(Value::Undefined);
        }
        current = prototype;
    }
}

fn super_own_value(holder: &Value, key: &str, receiver: &Value) -> Result<Option<Value>, VmError> {
    let properties: Vec<(String, Value)> = match holder {
        Value::Object(properties) => properties
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.clone()))
            .collect(),
        Value::Function(function) => function.properties.borrow().clone(),
        Value::ObjectAlias(alias) => {
            let Some(object) = alias.0.borrow().upgrade() else {
                return Ok(None);
            };
            return super_own_value(&Value::Object(object), key, receiver);
        }
        Value::Builtin(_) => {
            let value = crate::execute::get_property(holder, key);
            return Ok((!matches!(value, Value::Undefined)).then_some(value));
        }
        _ => return Ok(None),
    };
    if !properties.iter().any(|(name, _)| name == key) {
        return Ok(None);
    }
    let descriptor_key = crate::builtins::descriptor_key(key);
    if let Some((_, Value::Object(descriptor))) = properties
        .iter()
        .rev()
        .find(|(name, _)| name == &descriptor_key)
    {
        if !descriptor.iter().any(|(name, _)| name == "value") {
            if let Some((_, getter)) = descriptor.iter().rev().find(|(name, _)| name == "get") {
                if crate::conversion::is_callable(getter) {
                    return crate::functions::execute_target(getter, receiver, &[]).map(Some);
                }
            }
        }
    }
    Ok(properties
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone()))
}

fn put_with_receiver(
    target: &Value,
    key: &str,
    value: Value,
    receiver: &Value,
) -> Result<(), VmError> {
    let receiver = crate::locals::resolved_replacement(receiver.clone());
    crate::module_bindings::exports(&receiver, key)?;
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
    let prototype = crate::locals::resolved_replacement(prototype);
    let prototype = crate::builtins::object::get_prototype_of(Some(&prototype))?;
    let prototype = crate::locals::resolved_replacement(prototype);
    match prototype {
        Value::Null | Value::Undefined => Err(crate::value::error::throw_type_error(
            "Super has no prototype",
        )),
        _ => Ok(prototype),
    }
}

fn require_context_super_base(context: &Context) -> Result<Value, VmError> {
    match require_super_base(&context.home) {
        Ok(base) => Ok(base),
        Err(error) if matches!(&context.home, Value::ObjectAlias(alias) if alias.target().is_none()) => {
            recover_home_object(context).map_or(Err(error), |home| require_super_base(&home))
        }
        Err(error) => Err(error),
    }
}

fn recover_home_object(context: &Context) -> Option<Value> {
    let mut current = crate::locals::resolved_replacement(context.receiver.clone());
    for _ in 0..1_024 {
        if holder_contains_function(&current, &context.function) {
            return Some(current);
        }
        let prototype = crate::builtins::object::get_prototype_of(Some(&current)).ok()?;
        if matches!(prototype, Value::Null | Value::Undefined) {
            return None;
        }
        current = crate::locals::resolved_replacement(prototype);
    }
    None
}

fn holder_contains_function(
    holder: &Value,
    function: &Rc<crate::value::FunctionValue>,
) -> bool {
    let properties = match holder {
        Value::Object(properties) => &properties.properties,
        _ => return false,
    };
    properties.iter().any(|(_, value)| contains_function(value, function))
}

fn contains_function(value: &Value, function: &Rc<crate::value::FunctionValue>) -> bool {
    match value {
        Value::Function(candidate) => Rc::ptr_eq(candidate, function),
        Value::Object(descriptor) => descriptor
            .iter()
            .filter(|(name, _)| matches!(name.as_str(), "value" | "get" | "set"))
            .any(|(_, value)| contains_function(value, function)),
        _ => false,
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

pub(crate) fn attach_home(value: &Value, home: &Value) {
    attach(value, home);
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
