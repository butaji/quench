//! `internal/net`: the symbols net's public classes and their consumers (tls, http) share,
//! and the receiver adapter that presents a socket operation as a method of its socket.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The private symbols `internal/net` exports, by their exported name and description.
const SYMBOLS: [(&str, &str); 6] = [
    ("normalizedArgsSymbol", "normalizedArgs"),
    ("kSetNoDelay", "kSetNoDelay"),
    ("kSetKeepAlive", "kSetKeepAlive"),
    ("kSetKeepAliveInitialDelay", "kSetKeepAliveInitialDelay"),
    ("kSetKeepAliveInterval", "kSetKeepAliveInterval"),
    ("kSetKeepAliveCount", "kSetKeepAliveCount"),
];

pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0net:internal/net.js", |c| {
        let module = c.object_rooted()?;
        for (name, operation) in [("isIP", "netIsIP"), ("isIPv4", "netIsIPv4"), ("isIPv6", "netIsIPv6")] {
            let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            c.assign_named_property_rooted(module, name, function)?;
        }
        for (name, description) in SYMBOLS {
            let symbol = c.symbol_rooted(Some(description));
            c.assign_named_property_rooted(module, name, symbol)?;
        }
        Ok(module)
    })
}

/// `operation` as a method named `name`: `this` becomes its first argument, and the
/// method's parameters are the operation's remaining ones (a missing one is undefined).
pub(crate) fn method(c: &mut Context<'_>, operation: RootId, name: &str) -> Result<RootId, RootedError> {
    let length = c.get_named_property_rooted(operation, "length")?;
    let data = c.array_rooted(&[operation, length])?;
    c.host_function_named_with_data(crate::host::shared_vm::operation!("netReceiverCall"), name, data)
}

pub(crate) fn receiver_call(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let operation = c.get_named_property_rooted(data, "0")?;
    let length = c.get_named_property_rooted(data, "1")?;
    let length = c.rooted_value(length).and_then(|value| value.as_number()).unwrap_or(0.0) as usize;
    let undefined = c.undefined();
    let mut arguments = Vec::with_capacity(length.max(args.len() + 1));
    arguments.push(this);
    arguments.extend_from_slice(args);
    if arguments.len() < length {
        arguments.resize(length, undefined);
    }
    c.call_rooted(operation, undefined, &arguments)
}
