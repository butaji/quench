//! Socket option requests and their cached Node-visible transitions.
use crate::host::NodeHost;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError};

const MILLISECONDS_PER_SECOND: f64 = 1000.0;
pub(super) const SIGNED_INTEGER_MASK: f64 = -1.0;
const FIELDS: [(&str, &str); 5] = [
    ("noDelay", "kSetNoDelay"),
    ("keepAlive", "kSetKeepAlive"),
    ("initialDelay", "kSetKeepAliveInitialDelay"),
    ("interval", "kSetKeepAliveInterval"),
    ("count", "kSetKeepAliveCount"),
];

fn get(context: &mut NativeContext<'_, NodeHost>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn field(context: &mut NativeContext<'_, NodeHost>, data: RootId, socket: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = get(context, data, name)?;
    context.get_property_rooted(socket, key)
}
fn set(context: &mut NativeContext<'_, NodeHost>, data: RootId, socket: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = get(context, data, name)?;
    context.set_property_rooted(socket, key, value, socket)?;
    Ok(())
}
fn seconds(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<RootId, RootedError> {
    let scale = context.number(MILLISECONDS_PER_SECOND);
    let quotient = context.arithmetic_rooted(NativeArithmetic::Divide, value, scale)?;
    let mask = context.number(SIGNED_INTEGER_MASK);
    // Bitwise AND with all signed bits applies the shared ToInt32 conversion.
    context.arithmetic_rooted(NativeArithmetic::BitwiseAnd, quotient, mask)
}

pub(crate) fn install(context: &mut NativeContext<'_, NodeHost>) -> Result<[RootId; 4], RootedError> {
    let data = context.object_rooted()?;
    for (name, description) in FIELDS {
        let key = context.string_rooted(name);
        let symbol = context.symbol_rooted(Some(description));
        context.define_data_property_rooted(data, key, symbol, false, false, false)?;
    }
    let retained = context.retain(data)?;
    context.host_mut().shared_state().borrow_mut().net_socket_option_keys = Some(retained);
    let initialize = context.host_function_with_data(crate::host::shared_vm::operation("netSocketInitializeOptions"), data)?;
    let no_delay = context.host_function_with_data(crate::host::shared_vm::operation("netSocketSetNoDelay"), data)?;
    let keep_alive = context.host_function_with_data(crate::host::shared_vm::operation("netSocketSetKeepAlive"), data)?;
    let factory = context.evaluate_script_rooted(quench_js_check::checked_js!(
        "(noDelay,keepAlive)=>[function(enable){'use strict';return noDelay(this,enable);},function(enable,initialDelayMsecs,intervalMsecs,count){'use strict';return keepAlive(this,enable,initialDelayMsecs,intervalMsecs,count);}]"
    ), "node:net/socket-options.js")?;
    let undefined = context.undefined();
    let methods = context.call_rooted(factory, undefined, &[no_delay, keep_alive])?;
    let server = context.host_function_with_data(crate::host::shared_vm::operation("netServerInitializeOptions"), data)?;
    Ok([initialize, get(context, methods, "0")?, get(context, methods, "1")?, server])
}

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    for (field_name, option) in [("noDelay", "noDelay"), ("keepAlive", "keepAlive")] {
        let value = get(context, args[1], option)?;
        let value = context.truthy_rooted(value)?;
        let value = context.boolean(value);
        set(context, data, args[0], field_name, value)?;
    }
    let mut delay = get(context, args[1], "keepAliveInitialDelay")?;
    if !context.rooted_value(delay).is_some_and(|value| value.is_undefined()) && context.rooted_value(delay).and_then(|value| value.as_number()).is_none() {
        return Err(super::util_shared_vm::argument_type_error(context, delay, "options.keepAliveInitialDelay", "number")?);
    }
    if context.rooted_value(delay).and_then(|value| value.as_number()).is_some_and(|value| value < 0.0) { delay = context.number(0.0); }
    let delay = seconds(context, delay)?;
    set(context, data, args[0], "initialDelay", delay)?;
    Ok(context.undefined())
}

pub(crate) fn no_delay(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let socket = args[0];
    let input = args[1];
    let enable = if context.rooted_value(input).is_some_and(|value| value.is_undefined()) { true } else { context.truthy_rooted(input)? };
    let enable = context.boolean(enable);
    let data = context.host_function_data()?;
    let handle = get(context, socket, "_handle")?;
    if !context.truthy_rooted(handle)? {
        set(context, data, socket, "noDelay", enable)?;
        return Ok(socket);
    }
    let handle = get(context, socket, "_handle")?;
    let method = get(context, handle, "setNoDelay")?;
    if context.truthy_rooted(method)? {
        let previous = field(context, data, socket, "noDelay")?;
        if !context.strict_equal_rooted(enable, previous)? {
            set(context, data, socket, "noDelay", enable)?;
            let handle = get(context, socket, "_handle")?;
            let method = get(context, handle, "setNoDelay")?;
            context.call_rooted(method, handle, &[enable])?;
        }
    }
    Ok(socket)
}

pub(crate) fn keep_alive(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let socket = args[0];
    let mut enable = args[1];
    let mut delay = args[2];
    let mut interval = args[3];
    let mut count = args[4];
    if !context.rooted_value(enable).is_some_and(|value| value.is_null()) && context.is_object_rooted(enable)? {
        let options = enable;
        enable = get(context, options, "enable")?;
        delay = get(context, options, "initialDelay")?;
        interval = get(context, options, "interval")?;
        count = get(context, options, "count")?;
    }
    let enabled = context.truthy_rooted(enable)?;
    let enable = context.boolean(enabled);
    let delay = seconds(context, delay)?;
    let interval = if context.rooted_value(interval).is_some_and(|value| value.is_undefined()) { interval } else { seconds(context, interval)? };
    let data = context.host_function_data()?;
    let handle = get(context, socket, "_handle")?;
    if !context.truthy_rooted(handle)? {
        for (name, value) in [("keepAlive", enable), ("initialDelay", delay), ("interval", interval), ("count", count)] {
            set(context, data, socket, name, value)?;
        }
        return Ok(socket);
    }
    let handle = get(context, socket, "_handle")?;
    let method = get(context, handle, "setKeepAlive")?;
    if !context.truthy_rooted(method)? { return Ok(socket); }
    let previous = field(context, data, socket, "keepAlive")?;
    let mut changed = !context.strict_equal_rooted(enable, previous)?;
    if !changed && enabled {
        for (name, value) in [("initialDelay", delay), ("interval", interval), ("count", count)] {
            let previous = field(context, data, socket, name)?;
            if !context.strict_equal_rooted(value, previous)? { changed = true; break; }
        }
    }
    if changed {
        for (name, value) in [("keepAlive", enable), ("initialDelay", delay), ("interval", interval), ("count", count)] {
            set(context, data, socket, name, value)?;
        }
        let handle = get(context, socket, "_handle")?;
        let method = get(context, handle, "setKeepAlive")?;
        context.call_rooted(method, handle, &[enable, delay, interval, count])?;
    }
    Ok(socket)
}


pub(crate) fn initialize_server(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let server = args[0];
    let original = args[1];
    let options = context.object_rooted()?;
    if !context.rooted_value(original).is_some_and(|value| value.is_null() || value.is_undefined()) && !context.is_callable_rooted(original)? {
        for key in context.enumerable_own_keys_rooted(original, true, true)? {
            let value = context.get_property_rooted(original, key)?;
            context.define_data_property_rooted(options, key, value, true, true, true)?;
        }
    }
    let mut delay = get(context, options, "keepAliveInitialDelay")?;
    let mut high_water = get(context, options, "highWaterMark")?;
    for (name, value) in [("options.keepAliveInitialDelay", delay), ("options.highWaterMark", high_water)] {
        if !context.rooted_value(value).is_some_and(|value| value.is_undefined()) && context.rooted_value(value).and_then(|value| value.as_number()).is_none() {
            return Err(super::util_shared_vm::argument_type_error(context, value, name, "number")?);
        }
    }
    if context.rooted_value(delay).and_then(|value| value.as_number()).is_some_and(|value| value < 0.0) { delay = context.number(0.0); }
    let delay = seconds(context, delay)?;
    let object_mode = context.boolean(false);
    let undefined = context.undefined();
    if context.rooted_value(high_water).is_some_and(|value| value.is_undefined() || value.is_null()) || context.rooted_value(high_water).and_then(|value| value.as_number()).is_some_and(|value| value < 0.0) {
        high_water = super::stream_shared_vm::get_default_high_water_mark(context, undefined, &[object_mode])?;
    }
    let allow = get(context, options, "allowHalfOpen")?;
    let allow = if context.truthy_rooted(allow)? { allow } else { context.boolean(false) };
    for (name, value) in [("allowHalfOpen", allow), ("keepAliveInitialDelay", delay), ("highWaterMark", high_water)] {
        let key = context.string_rooted(name);
        context.set_property_rooted(server, key, value, server)?;
    }
    for name in ["pauseOnConnect", "noDelay", "keepAlive"] {
        let value = get(context, options, name)?;
        let value = context.truthy_rooted(value)?;
        let value = context.boolean(value);
        let key = context.string_rooted(name);
        context.set_property_rooted(server, key, value, server)?;
    }
    let key = context.string_rooted("_connections");
    let zero = context.number(0.0);
    context.set_property_rooted(server, key, zero, server)?;
    let block = get(context, options, "blockList")?;
    if context.truthy_rooted(block)? {
        let data = context.host_function_data()?;
        let module = get(context, data, "module")?;
        let constructor = get(context, module, "BlockList")?;
        let predicate = get(context, constructor, "isBlockList")?;
        let valid = context.call_rooted(predicate, constructor, &[block])?;
        if !context.truthy_rooted(valid)? {
            return Err(super::util_shared_vm::argument_type_error(context, block, "options.blockList", "net.BlockList")?);
        }
        let key = context.string_rooted("blockList");
        context.set_property_rooted(server, key, block, server)?;
    }
    Ok(context.undefined())
}

pub(crate) fn accepted_options(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = context.object_rooted()?;
    let server = args[0];
    for (name, source) in [("allowHalfOpen", "allowHalfOpen"), ("pauseOnCreate", "pauseOnConnect"), ("readableHighWaterMark", "highWaterMark"), ("writableHighWaterMark", "highWaterMark")] {
        let value = get(context, server, source)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(options, key, value, true, true, true)?;
    }
    Ok(options)
}

pub(crate) fn apply_accepted(context: &mut NativeContext<'_, NodeHost>, server: RootId, socket: RootId, handle: RootId) -> Result<(), RootedError> {
    let data = context.host_mut().shared_state().borrow().net_socket_option_keys
        .ok_or_else(|| RootedError::host("socket option keys missing"))?;
    let no_delay = get(context, server, "noDelay")?;
    if context.truthy_rooted(no_delay)? {
        let method = get(context, handle, "setNoDelay")?;
        if context.truthy_rooted(method)? {
            let enabled = context.boolean(true);
            set(context, data, socket, "noDelay", enabled)?;
            context.call_rooted(method, handle, &[enabled])?;
        }
    }
    let keep_alive = get(context, server, "keepAlive")?;
    if context.truthy_rooted(keep_alive)? {
        let method = get(context, handle, "setKeepAlive")?;
        if context.truthy_rooted(method)? {
            let enabled = context.boolean(true);
            let delay = get(context, server, "keepAliveInitialDelay")?;
            set(context, data, socket, "keepAlive", enabled)?;
            set(context, data, socket, "initialDelay", delay)?;
            context.call_rooted(method, handle, &[enabled, delay])?;
        }
    }
    Ok(())
}


pub(crate) fn apply_connected(context: &mut NativeContext<'_, NodeHost>, socket: RootId) -> Result<(), RootedError> {
    let data = context.host_mut().shared_state().borrow().net_socket_option_keys
        .ok_or_else(|| RootedError::host("socket option keys missing"))?;
    let no_delay = field(context, data, socket, "noDelay")?;
    if context.truthy_rooted(no_delay)? {
        let handle = get(context, socket, "_handle")?;
        let method = get(context, handle, "setNoDelay")?;
        if context.truthy_rooted(method)? {
            let handle = get(context, socket, "_handle")?;
            let method = get(context, handle, "setNoDelay")?;
            let enabled = context.boolean(true);
            context.call_rooted(method, handle, &[enabled])?;
        }
    }
    let keep_alive = field(context, data, socket, "keepAlive")?;
    if context.truthy_rooted(keep_alive)? {
        let handle = get(context, socket, "_handle")?;
        let method = get(context, handle, "setKeepAlive")?;
        if context.truthy_rooted(method)? {
            let handle = get(context, socket, "_handle")?;
            let method = get(context, handle, "setKeepAlive")?;
            let enabled = context.boolean(true);
            let delay = field(context, data, socket, "initialDelay")?;
            let interval = field(context, data, socket, "interval")?;
            let count = field(context, data, socket, "count")?;
            context.call_rooted(method, handle, &[enabled, delay, interval, count])?;
        }
    }
    Ok(())
}
