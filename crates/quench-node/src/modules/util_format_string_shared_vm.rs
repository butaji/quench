//! The `%s` projection shared by util formatting and Node coded errors.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn owns(c: &mut Context<'_>, object: RootId, key: RootId) -> Result<bool, RootedError> {
    let descriptor = c.own_property_descriptor_rooted(object, key)?;
    Ok(!c.rooted_value(descriptor).is_some_and(|v| v.is_undefined()))
}
fn built_in_to_string(
    c: &mut Context<'_>,
    mut value: RootId,
    primitive: RootId,
) -> Result<bool, RootedError> {
    while let Some(target) = c.proxy_target_rooted(value)? {
        if c.rooted_value(target)
            .is_some_and(|v| v.is_null() || v.is_undefined())
        {
            return Ok(true);
        }
        value = target;
    }
    let string_key = c.string_rooted("toString");
    let to_string = c.get_property_rooted(value, string_key)?;
    let string_callable = c.is_callable_rooted(to_string)?;
    let (check_string, check_primitive) = if !string_callable {
        let method = c.get_property_rooted(value, primitive)?;
        if !c.is_callable_rooted(method)? {
            return Ok(true);
        }
        if owns(c, value, primitive)? {
            return Ok(false);
        }
        (false, true)
    } else {
        if owns(c, value, string_key)? {
            return Ok(false);
        }
        let method = c.get_property_rooted(value, primitive)?;
        let callable = c.is_callable_rooted(method)?;
        if callable && owns(c, value, primitive)? {
            return Ok(false);
        }
        (true, callable)
    };
    loop {
        value = c.prototype_rooted(value)?;
        if c.rooted_value(value).is_some_and(|v| v.is_null()) {
            return Ok(true);
        }
        if (check_string && owns(c, value, string_key)?)
            || (check_primitive && owns(c, value, primitive)?)
        {
            break;
        }
    }
    let constructor_key = c.string_rooted("constructor");
    let descriptor = c.own_property_descriptor_rooted(value, constructor_key)?;
    if c.rooted_value(descriptor).is_some_and(|v| v.is_undefined()) {
        return Ok(false);
    }
    let constructor = get(c, descriptor, "value")?;
    if !c.is_callable_rooted(constructor)? {
        return Ok(false);
    }
    let name = get(c, constructor, "name")?;
    let Some(name) = c.string_text(name)? else {
        return Ok(false);
    };
    let mut chars = name.chars();
    let built_in_name = chars.next().is_some_and(|ch| ch.is_ascii_uppercase())
        && chars.clone().next().is_some()
        && chars.all(|ch| ch.is_ascii_alphanumeric());
    Ok(built_in_name && c.has_startup_global_name(&name))
}
pub(crate) fn format(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    let data = c.host_function_data()?;
    let primitive = get(c, data, "primitive")?;
    let should_inspect =
        if c.is_object_rooted(value)? && !c.rooted_value(value).is_some_and(|v| v.is_null()) {
            built_in_to_string(c, value, primitive)?
        } else {
            !c.is_callable_rooted(value)?
                && c.string_text(value)?.is_none()
                && !c.is_symbol_rooted(value)?
        };
    if should_inspect {
        let inspect = args
            .get(1)
            .copied()
            .ok_or_else(|| RootedError::host("original util inspector missing"))?;
        let options = get(c, data, "options")?;
        let undefined = c.undefined();
        c.call_rooted(inspect, undefined, &[value, options])
    } else {
        // String() additionally accepts Symbols, unlike ordinary ToString.
        let string = get(c, data, "string")?;
        let undefined = c.undefined();
        c.call_rooted(string, undefined, &[value])
    }
}
pub(crate) fn function(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let data = c.null_object_rooted()?;
    let global = c.global_root()?;
    let string = get(c, global, "String")?;
    let symbol = get(c, global, "Symbol")?;
    let primitive = get(c, symbol, "toPrimitive")?;
    let options = c.null_object_rooted()?;
    for (name, value) in [("compact", 3.0), ("depth", 0.0)] {
        let key = c.string_rooted(name);
        let value = c.number(value);
        c.define_data_property_rooted(options, key, value, true, true, true)?;
    }
    let key = c.string_rooted("colors");
    let no = c.boolean(false);
    c.define_data_property_rooted(options, key, no, true, true, true)?;
    for (name, value) in [
        ("primitive", primitive),
        ("string", string),
        ("options", options),
    ] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(data, key, value, false, false, false)?;
    }
    c.host_function_with_data(crate::host::shared_vm::operation("utilFormatString"), data)
}
