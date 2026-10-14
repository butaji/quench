//! Child environment enumeration and coercion shared by async and sync spawn.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::{collections::HashSet, process::Command};

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

// Node intentionally includes enumerable prototype properties. A nearer own
// property shadows an inherited key even when the nearer property is hidden.
fn keys(
    context: &mut NativeContext<'_, NodeHost>,
    env: RootId,
) -> Result<Vec<(String, RootId)>, RootedError> {
    let mut current = env;
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    loop {
        for key in context.own_keys_rooted(current)? {
            let Some(name) = context.string_text(key)? else {
                continue;
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            let descriptor = context.own_property_descriptor_rooted(current, key)?;
            if context
                .rooted_value(descriptor)
                .is_some_and(|value| value.is_undefined())
            {
                continue;
            }
            let enumerable = get(context, descriptor, "enumerable")?;
            if context.truthy_rooted(enumerable)? {
                keys.push((name, key));
            }
        }
        current = context.prototype_rooted(current)?;
        if context
            .rooted_value(current)
            .is_some_and(|value| value.is_null())
        {
            break;
        }
    }
    #[cfg(windows)]
    {
        keys.sort_by_key(|(name, _)| name.encode_utf16().collect::<Vec<_>>());
        let mut seen = HashSet::new();
        keys.retain(|(name, _)| seen.insert(name.to_uppercase()));
    }
    Ok(keys)
}

fn null_check(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<(), RootedError> {
    if context
        .string_text(value)?
        .is_some_and(|value| value.contains('\0'))
    {
        let undefined = context.undefined();
        let quoted = super::util_shared_vm::inspect_quote(context, undefined, &[value])?;
        let quoted = context.to_string(quoted)?;
        let error = context.type_error_rooted(&format!(
            "The property '{name}' must be a string without null bytes. Received {quoted}"
        ))?;
        let key = context.string_rooted("code");
        let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
        context.set_property_rooted(error, key, code, error)?;
        return Err(context.throw(error));
    }
    Ok(())
}

pub(crate) fn configure(
    context: &mut NativeContext<'_, NodeHost>,
    command: &mut Command,
    env: RootId,
) -> Result<(), RootedError> {
    let keys = keys(context, env)?;
    command.env_clear();
    for (name, key) in keys {
        let value = context.get_property_rooted(env, key)?;
        if context
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined())
        {
            continue;
        }
        let argument = format!("options.env['{name}']");
        null_check(context, key, &argument)?;
        null_check(context, value, &argument)?;
        command.env(name, context.to_string(value)?);
    }
    Ok(())
}
