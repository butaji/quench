//! Legacy util extension: snapshot enumerable string keys, then copy in reverse.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn name(c: &mut Context<'_>, function: RootId, text: &str) -> Result<(), RootedError> {
    let key = c.string_rooted("name");
    let value = c.string_rooted(text);
    c.define_data_property_rooted(function, key, value, false, false, true)?;
    Ok(())
}

pub(crate) fn install(
    c: &mut Context<'_>,
    module: RootId,
    warned_codes: RootId,
) -> Result<(), RootedError> {
    // A strict property assignment is Core syntax/semantics. The copy policy and
    // enumeration live here; this bridge preserves original setter exceptions.
    let assign = c.evaluate_script_rooted(
        "(function(target, key, value) { 'use strict'; target[key] = value; })",
        "node:util/extend-assignment.js",
    )?;
    let inner =
        c.host_constructor_with_data(crate::host::shared_vm::operation("utilExtend"), assign)?;
    name(c, inner, "_extend")?;
    let data = c.object_rooted()?;
    put(c, data, "assign", assign)?;
    put(c, data, "codes", warned_codes)?;
    let has = get(c, warned_codes, "has")?;
    let add = get(c, warned_codes, "add")?;
    put(c, data, "has", has)?;
    put(c, data, "add", add)?;
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    put(c, data, "process", process)?;
    let no = c.boolean(false);
    put(c, data, "warned", no)?;
    let outer = c.host_constructor_with_data(
        crate::host::shared_vm::operation("utilExtendDeprecated"),
        data,
    )?;
    put(c, data, "function", outer)?;
    name(c, outer, "deprecated")?;
    c.set_prototype_rooted(outer, inner)?;
    let prototype = get(c, inner, "prototype")?;
    let key = c.string_rooted("prototype");
    c.set_property_rooted(outer, key, prototype, outer)?;
    put(c, module, "_extend", outer)
}

pub(crate) fn deprecated(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let process = get(c, data, "process")?;
    let no_deprecation = get(c, process, "noDeprecation")?;
    if !c.truthy_rooted(no_deprecation)? {
        let warned = get(c, data, "warned")?;
        if !c.truthy_rooted(warned)? {
            let yes = c.boolean(true);
            put(c, data, "warned", yes)?;
            let codes = get(c, data, "codes")?;
            let has = get(c, data, "has")?;
            let code = c.string_rooted("DEP0060");
            let already = c.call_rooted(has, codes, &[code])?;
            if !c.truthy_rooted(already)? {
                let emit = get(c, process, "emitWarning")?;
                let message = c.string_rooted(
                    "The `util._extend` API is deprecated. Please use Object.assign() instead.",
                );
                let kind = c.string_rooted("DeprecationWarning");
                // Node passes the public wrapper as the stack cutoff function.
                let function = get(c, data, "function")?;
                c.call_rooted(emit, process, &[message, kind, code, function])?;
                let add = get(c, data, "add")?;
                c.call_rooted(add, codes, &[code])?;
            }
        }
    }
    let assign = get(c, data, "assign")?;
    copy(c, args, assign)
}

pub(crate) fn extend(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let assign = c.host_function_data()?;
    copy(c, args, assign)
}

fn copy(c: &mut Context<'_>, args: &[RootId], assign: RootId) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let target = args.first().copied().unwrap_or(undefined);
    let source = args.get(1).copied().unwrap_or(undefined);
    if !c.is_object_rooted(source)? || c.is_callable_rooted(source)? {
        return Ok(target);
    }
    let keys = c.enumerable_own_keys_rooted(source, false, true)?;
    for key in keys.into_iter().rev() {
        let value = c.get_property_rooted(source, key)?;
        c.call_rooted(assign, undefined, &[target, key, value])?;
    }
    Ok(target)
}
