//! Duplex transport between two ordinary streams, with callback-driven demand.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

// Only Core class/field syntax and argument forwarding live in the facade.
const SIDE: &str = r#"(function(Duplex, initialize, link, read, write, final, destroy, initKey) {
  class DuplexSide extends Duplex {
    #otherSide = null;
    constructor(options) { super(options); initialize(this); this.#otherSide = null; }
    [initKey](other) { this.#otherSide = link(this.#otherSide, other); }
    _read() { return read(this); }
    _write(chunk, encoding, callback) { return write(this.#otherSide, chunk, encoding, callback); }
    _final(callback) { return final(this.#otherSide, callback); }
    _destroy(error, callback) { return destroy(this.#otherSide, error, callback); }
  }
  return { DuplexSide, assign(target, key, value) { 'use strict'; target[key] = value; } };
})"#;
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}
fn call(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = get(c, object, name)?;
    c.call_rooted(function, object, args)
}
fn is_null(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null())
}
fn assign_callback(
    c: &mut Context<'_>,
    data: RootId,
    stream: RootId,
    value: RootId,
) -> Result<(), RootedError> {
    let setter = get(c, data, "assign")?;
    let key = get(c, data, "callbackKey")?;
    let undefined = c.undefined();
    c.call_rooted(setter, undefined, &[stream, key, value])?;
    Ok(())
}
fn callback(c: &mut Context<'_>, data: RootId, stream: RootId) -> Result<RootId, RootedError> {
    let key = get(c, data, "callbackKey")?;
    c.get_property_rooted(stream, key)
}
fn assertion(c: &mut Context<'_>, condition: bool) -> Result<(), RootedError> {
    if condition {
        return Ok(());
    }
    let error=c.error_rooted("This is caused by either a bug in Node.js or incorrect usage of Node.js internals.\nPlease open an issue with this stack trace at https://github.com/nodejs/node/issues\n")?;
    super::node_errors_shared_vm::decorate_error(c, error, "ERR_INTERNAL_ASSERTION")?;
    Err(c.throw(error))
}
fn next_tick(c: &mut Context<'_>, data: RootId, callback: RootId) -> Result<(), RootedError> {
    let process = get(c, data, "process")?;
    call(c, process, "nextTick", &[callback])?;
    Ok(())
}

pub(crate) fn install(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let data = c.null_object_rooted()?;
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    put(c, data, "process", process)?;
    let callback_key = c.symbol_rooted(Some("Callback"));
    put(c, data, "callbackKey", callback_key)?;
    let init_key = c.symbol_rooted(Some("InitOtherSide"));
    put(c, data, "initKey", init_key)?;
    let duplex = get(c, module, "Duplex")?;
    let mut args = vec![duplex];
    for operation in [
        "duplexPairInitialize",
        "duplexPairLink",
        "duplexPairRead",
        "duplexPairWrite",
        "duplexPairFinal",
        "duplexPairDestroy",
    ] {
        args.push(c.host_function_with_data(crate::host::shared_vm::operation(operation), data)?);
    }
    args.push(init_key);
    let factory = c.evaluate_script_rooted(SIDE, "node:stream/duplex-pair.js")?;
    let undefined = c.undefined();
    let side = c.call_rooted(factory, undefined, &args)?;
    for name in ["DuplexSide", "assign"] {
        let value = get(c, side, name)?;
        put(c, data, name, value)?;
    }
    let function =
        c.host_constructor_with_data(crate::host::shared_vm::operation("duplexPair"), data)?;
    put(c, module, "duplexPair", function)
}

pub(crate) fn pair(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, "DuplexSide")?;
    let options = arg(c, args, 0);
    let left = c.construct_rooted(constructor, constructor, &[options])?;
    let right = c.construct_rooted(constructor, constructor, &[options])?;
    let key = get(c, data, "initKey")?;
    let init = c.get_property_rooted(left, key)?;
    c.call_rooted(init, left, &[right])?;
    let init = c.get_property_rooted(right, key)?;
    c.call_rooted(init, right, &[left])?;
    c.array_rooted(&[left, right])
}
pub(crate) fn initialize(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let stream = arg(c, args, 0);
    let null = c.null();
    assign_callback(c, data, stream, null)?;
    Ok(c.undefined())
}
pub(crate) fn link(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let current = arg(c, args, 0);
    let allowed = is_null(c, current);
    assertion(c, allowed)?;
    Ok(arg(c, args, 1))
}
pub(crate) fn read(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let stream = arg(c, args, 0);
    let callback = callback(c, data, stream)?;
    if c.truthy_rooted(callback)? {
        let null = c.null();
        assign_callback(c, data, stream, null)?;
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[])?;
    }
    Ok(c.undefined())
}
pub(crate) fn write(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let peer = arg(c, args, 0);
    let chunk = arg(c, args, 1);
    let listener = arg(c, args, 3);
    let linked = !is_null(c, peer);
    assertion(c, linked)?;
    let pending = callback(c, data, peer)?;
    let available = is_null(c, pending);
    assertion(c, available)?;
    let length = get(c, chunk, "length")?;
    if c.rooted_value(length).and_then(|value| value.as_number()) == Some(0.0) {
        next_tick(c, data, listener)?;
    } else {
        call(c, peer, "push", &[chunk])?;
        assign_callback(c, data, peer, listener)?;
    }
    Ok(c.undefined())
}
pub(crate) fn final_(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let peer = arg(c, args, 0);
    let callback = arg(c, args, 1);
    let end = c.string_rooted("end");
    call(c, peer, "on", &[end, callback])?;
    let null = c.null();
    call(c, peer, "push", &[null])?;
    Ok(c.undefined())
}
pub(crate) fn destroy(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let peer = arg(c, args, 0);
    let error = arg(c, args, 1);
    let callback = arg(c, args, 2);
    if !is_null(c, peer) {
        let destroyed = get(c, peer, "destroyed")?;
        if !c.truthy_rooted(destroyed)? {
            let task = c.array_rooted(&[peer, error])?;
            let function = c.host_function_with_data(
                crate::host::shared_vm::operation("duplexPairDestroyPeer"),
                task,
            )?;
            next_tick(c, data, function)?;
        }
    }
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])?;
    Ok(c.undefined())
}
pub(crate) fn destroy_peer(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let zero = c.number(0.0);
    let peer = c.get_property_rooted(data, zero)?;
    let one = c.number(1.0);
    let error = c.get_property_rooted(data, one)?;
    let destroyed = get(c, peer, "destroyed")?;
    if !c.truthy_rooted(destroyed)? {
        if c.truthy_rooted(error)? {
            call(c, peer, "destroy", &[])?;
        } else {
            let null = c.null();
            call(c, peer, "push", &[null])?;
        }
    }
    Ok(c.undefined())
}
