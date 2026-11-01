//! Deprecation wrappers share one per-realm code history and preserve constructor targets.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
const CODES: &str = "\0warnings:deprecationCodes";

pub(crate) fn wrap(
    c: &mut Context<'_>,
    target: RootId,
    message: &str,
    code: &str,
) -> Result<RootId, RootedError> {
    let data = c.null_object_rooted()?;
    c.assign_named_property_rooted(data, "target", target)?;
    let message = c.string_rooted(message);
    c.assign_named_property_rooted(data, "message", message)?;
    let code = c.string_rooted(code);
    c.assign_named_property_rooted(data, "code", code)?;
    let no = c.boolean(false);
    c.assign_named_property_rooted(data, "warned", no)?;
    let wrapper = c.host_constructor_named_with_data(
        crate::host::shared_vm::operation("utilDeprecatedCall"),
        "deprecated",
        data,
    )?;
    c.assign_named_property_rooted(data, "wrapper", wrapper)?;
    c.set_prototype_rooted(wrapper, target)?;
    let prototype = c.get_named_property_rooted(target, "prototype")?;
    if c.truthy_rooted(prototype)? {
        c.assign_named_property_rooted(wrapper, "prototype", prototype)?;
    }
    let length = c.string_rooted("length");
    if let Some(descriptor) = c.own_property_fields_rooted(target, length)? {
        c.define_property_fields_rooted(wrapper, length, descriptor)?;
    }
    Ok(wrapper)
}

pub(crate) fn emit(
    c: &mut Context<'_>,
    message: RootId,
    code: RootId,
    cutoff: RootId,
) -> Result<(), RootedError> {
    let codes = crate::host::shared_vm::commonjs::cached_native_binding(c, CODES, |c| {
        c.null_object_rooted()
    })?;
    let seen = c.get_property_rooted(codes, code)?;
    if !c.truthy_rooted(seen)? {
        let global = c.global_root()?;
        let process = c.get_named_property_rooted(global, "process")?;
        let warning = c.get_named_property_rooted(process, "emitWarning")?;
        let kind = c.string_rooted("DeprecationWarning");
        c.call_rooted(warning, process, &[message, kind, code, cutoff])?;
        let yes = c.boolean(true);
        c.assign_property_rooted(codes, code, yes)?;
    }
    Ok(())
}

pub(crate) fn call(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let global = c.global_root()?;
    let process = c.get_named_property_rooted(global, "process")?;
    let disabled = c.get_named_property_rooted(process, "noDeprecation")?;
    if !c.truthy_rooted(disabled)? {
        let warned = c.get_named_property_rooted(data, "warned")?;
        if !c.truthy_rooted(warned)? {
            // Node marks the emitter before invoking a potentially throwing warning hook.
            let yes = c.boolean(true);
            c.assign_named_property_rooted(data, "warned", yes)?;
            let message = c.get_named_property_rooted(data, "message")?;
            let code = c.get_named_property_rooted(data, "code")?;
            let wrapper = c.get_named_property_rooted(data, "wrapper")?;
            emit(c, message, code, wrapper)?;
        }
    }
    let target = c.get_named_property_rooted(data, "target")?;
    match c.new_target_rooted() {
        Some(new_target) => c.construct_rooted(target, new_target, args),
        None => c.call_rooted(target, this, args),
    }
}
