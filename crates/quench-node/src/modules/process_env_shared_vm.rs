//! The environment proxy's property operations, over the shared runtime semantics.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};
use super::event_constructor_shared_vm::get;
type Context<'a> = NativeContext<'a, NodeHost>;

fn trap_arguments(context: &mut Context<'_>, args: &[RootId]) -> [RootId; 3] {
    let missing = context.undefined();
    std::array::from_fn(|index| args.get(index).copied().unwrap_or(missing))
}

fn invalid(context: &mut Context<'_>, message: &str) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(message)?;
    let code = context.string_rooted("ERR_INVALID_OBJECT_DEFINE_PROPERTY");
    context.assign_named_property_rooted(error, "code", code)?;
    Ok(context.throw(error))
}

fn reject_symbol(context: &mut Context<'_>, value: RootId) -> Result<(), RootedError> {
    if context.is_symbol_rooted(value)? {
        let error = context.type_error_rooted("Cannot convert a Symbol value to a string")?;
        return Err(context.throw(error));
    }
    Ok(())
}

fn store(context: &mut Context<'_>, target: RootId, key: RootId, value: RootId) -> Result<RootId, RootedError> {
    let text = context.to_string_code_units(value)?;
    let text = context.string_units_rooted(&text);
    let accepted = context.define_data_property_rooted(target, key, text, true, true, true)?;
    if accepted { super::process_shared_vm::set_env(context, target, &[key, text])?; }
    Ok(context.boolean(accepted))
}

pub(crate) fn set(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let [target, key, value] = trap_arguments(context, args);
    reject_symbol(context, key)?;
    reject_symbol(context, value)?;
    if context.string_text(key)?.as_deref() == Some("") { return Ok(context.boolean(true)); }
    if !matches!(context.value_type_rooted(value), Some(ValueType::String | ValueType::Number | ValueType::Boolean)) {
        let global = context.global_root()?;
        let process = get(context, global, "process")?;
        let flags = get(context, process, "execArgv")?;
        let includes = get(context, flags, "includes")?;
        let flag = context.string_rooted("--pending-deprecation");
        let enabled = context.call_rooted(includes, flags, &[flag])?;
        if context.truthy_rooted(enabled)? {
            let warning = get(context, process, "emitWarning")?;
            let message = context.string_rooted("Assigning any value other than a string, number, or boolean to a process.env property is deprecated. Please make sure to convert the value to a string before setting process.env with it.");
            let options = context.object_rooted()?;
            let kind = context.string_rooted("DeprecationWarning");
            let code = context.string_rooted("DEP0104");
            context.assign_named_property_rooted(options, "type", kind)?;
            context.assign_named_property_rooted(options, "code", code)?;
            context.call_rooted(warning, process, &[message, options])?;
        }
    }
    store(context, target, key, value)
}

pub(crate) fn define(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let [target, key, descriptor] = trap_arguments(context, args);
    if context.is_symbol_rooted(key)? { return Err(invalid(context, "'process.env' does not accept symbol properties")?); }
    if context.string_text(key)?.as_deref() == Some("") { return Ok(context.boolean(true)); }
    for name in ["get", "set"] {
        let field = context.string_rooted(name);
        if context.has_property_rooted(descriptor, field)? {
            return Err(invalid(context, "'process.env' does not accept an accessor(getter/setter) descriptor")?);
        }
    }
    for name in ["configurable", "writable", "enumerable"] {
        let value = get(context, descriptor, name)?;
        if context.rooted_value(value).and_then(|value| value.as_bool()) != Some(true) {
            return Err(invalid(context, "'process.env' only accepts a configurable, writable, and enumerable data descriptor")?);
        }
    }
    let value = get(context, descriptor, "value")?;
    store(context, target, key, value)
}

pub(crate) fn delete(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let [target, key, _] = trap_arguments(context, args);
    let accepted = context.delete_property_rooted(target, key)?;
    if accepted && !context.is_symbol_rooted(key)? && context.string_text(key)?.as_deref() != Some("") {
        super::process_shared_vm::delete_env(context, target, &[key])?;
    }
    Ok(context.boolean(accepted))
}

pub(crate) fn wrap(context: &mut Context<'_>, target: RootId) -> Result<RootId, RootedError> {
    let handler = context.object_rooted()?;
    for (name, operation) in [("set", "processEnvSetTrap"), ("defineProperty", "processEnvDefineTrap"), ("deleteProperty", "processEnvDeleteTrap")] {
        let callback = context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        context.assign_named_property_rooted(handler, name, callback)?;
    }
    let global = context.global_root()?;
    let proxy = get(context, global, "Proxy")?;
    context.construct_rooted(proxy, proxy, &[target, handler])
}
