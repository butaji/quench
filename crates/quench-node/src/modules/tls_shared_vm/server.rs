//! TLS Server projects accepted TCP owners into actual SSL sessions.
use super::socket::{get, method, set};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
pub(crate) fn create(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let api = super::facade::install(context)?;
    let constructor = get(context, api, "serverConstructor")?;
    context.construct_rooted(constructor, constructor, args)
}
pub(crate) fn construct(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let receiver = args[0];
    let target = args[1];
    let undefined = context.undefined();
    let mut options = args.get(2).copied().unwrap_or(undefined);
    let mut listener = args.get(3).copied().unwrap_or(undefined);
    if context
        .rooted_value(target)
        .is_some_and(|value| value.is_undefined())
    {
        return create(context, undefined, &[options, listener]);
    }
    if context.is_callable_rooted(options)? {
        listener = options;
        options = undefined;
    }
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        options = context.object_rooted()?;
        let null = context.null();
        context.set_prototype_rooted(options, null)?;
    } else if !context.is_object_rooted(options)? {
        return Err(super::super::util_shared_vm::argument_type_error(
            context, options, "options", "Object",
        )?);
    }
    let contexts = context.array_rooted(&[])?;
    set(context, receiver, "_contexts", contexts)?;
    let request = get(context, options, "requestCert")?;
    let request = context
        .rooted_value(request)
        .is_some_and(|value| value.as_bool() == Some(true));
    let request = context.boolean(request);
    set(context, receiver, "requestCert", request)?;
    let reject = get(context, options, "rejectUnauthorized")?;
    let reject = !context
        .rooted_value(reject)
        .is_some_and(|value| value.as_bool() == Some(false));
    let reject = context.boolean(reject);
    set(context, receiver, "rejectUnauthorized", reject)?;
    method(context, receiver, "setSecureContext", &[options])?;
    let api = super::facade::install(context)?;
    let net = get(context, api, "identityNet")?;
    let base = get(context, net, "Server")?;
    let connection =
        context.host_function(crate::host::shared_vm::operation("tlsServerConnection"))?;
    context.call_rooted(base, receiver, &[options, connection])?;
    if context.truthy_rooted(listener)? {
        let secure = context.string_rooted("secureConnection");
        method(context, receiver, "on", &[secure, listener])?;
    }
    Ok(receiver)
}
pub(crate) fn set_context(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args.first().copied().unwrap_or_else(|| context.undefined());
    let api = super::facade::install(context)?;
    let module = get(context, api, "module")?;
    let secure = method(context, module, "createSecureContext", &[options])?;
    set(context, receiver, "_sharedCreds", secure)?;
    Ok(context.undefined())
}
pub(crate) fn connection(
    context: &mut NativeContext<'_, NodeHost>,
    server: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let api = super::facade::install(context)?;
    let constructor = get(context, api, "socketConstructor")?;
    let options = context.object_rooted()?;
    let secure = get(context, server, "_sharedCreds")?;
    set(context, options, "secureContext", secure)?;
    let yes = context.boolean(true);
    set(context, options, "isServer", yes)?;
    set(context, options, "server", server)?;
    for name in [
        "requestCert",
        "rejectUnauthorized",
        "ALPNProtocols",
        "ALPNCallback",
    ] {
        let value = get(context, server, name)?;
        set(context, options, name, value)?;
    }
    let socket = context.construct_rooted(constructor, constructor, &[args[0], options])?;
    super::sessions::set_purpose(context, socket, super::sessions::Purpose::Server)?;
    let event = context.string_rooted("error");
    let callback =
        context.host_function(crate::host::shared_vm::operation("tlsServerSocketError"))?;
    method(context, socket, "on", &[event, callback])?;
    let undefined = context.undefined();
    super::socket::start(context, undefined, &[socket])?;
    Ok(context.undefined())
}
pub(crate) fn socket_error(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let established = get(context, socket, "_secureEstablished")?;
    if !context.truthy_rooted(established)? {
        let options = get(context, socket, "_tlsOptions")?;
        let server = get(context, options, "server")?;
        let event = context.string_rooted("tlsClientError");
        let error = args.first().copied().unwrap_or_else(|| context.undefined());
        method(context, server, "emit", &[event, error, socket])?;
    }
    Ok(context.undefined())
}

pub(crate) fn set_context_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    set_context(context, args[0], &args[1..])
}
