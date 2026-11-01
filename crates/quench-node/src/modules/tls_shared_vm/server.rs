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
    // Node's TLS Server validates the effective handshake timeout after
    // applying its default (so falsy values keep the default semantics).
    // This facade constructs Server directly rather than using Node's
    // internal/tls/wrap.js constructor, so perform that public validation here.
    let handshake_timeout = get(context, options, "handshakeTimeout")?;
    if context.truthy_rooted(handshake_timeout)?
        && context
            .rooted_value(handshake_timeout)
            .and_then(|value| value.as_number())
            .is_none()
    {
        return Err(super::super::util_shared_vm::argument_type_error(
            context,
            handshake_timeout,
            "options.handshakeTimeout",
            "number",
        )?);
    }
    let sni_callback = get(context, options, "SNICallback")?;
    if context.truthy_rooted(sni_callback)? && !context.is_callable_rooted(sni_callback)? {
        return Err(super::super::util_shared_vm::argument_type_error(
            context,
            sni_callback,
            "options.SNICallback",
            "function",
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
    let api = super::facade::install(context)?;
    let tls_module = get(context, api, "module")?;
    let convert_alpn = get(context, tls_module, "convertALPNProtocols")?;
    let alpn_protocols = get(context, options, "ALPNProtocols")?;
    let converted = context.object_rooted()?;
    // Node keeps the public Server.ALPNProtocols value as the wire-format
    // Buffer produced while normalizing the constructor options. Preserve
    // that projection as well as the encoded value used by TLS sockets.
    context.call_rooted(convert_alpn, undefined, &[alpn_protocols, converted])?;
    let alpn_protocols = get(context, converted, "ALPNProtocols")?;
    set(context, receiver, "ALPNProtocols", alpn_protocols)?;
    let alpn_callback = get(context, options, "ALPNCallback")?;
    set(context, receiver, "ALPNCallback", alpn_callback)?;
    method(context, receiver, "setSecureContext", &[options])?;
    let net = get(context, api, "identityNet")?;
    let base = get(context, net, "Server")?;
    let connection =
        context.host_function(crate::host::shared_vm::operation("tlsServerConnection"))?;
    context.call_rooted(base, receiver, &[options, connection])?;
    // Node exposes TLS ticket rotation on the Server as a thin projection of
    // its shared SecureContext. The SecureContext operation is already backed
    // by OpenSSL; installing the forwarding method here makes that session
    // control reachable through tls.Server/https.Server as expected.
    let ticket_keys_factory = context.evaluate_script_rooted(
        "server => function setTicketKeys(keys) { return server._sharedCreds.context.setTicketKeys(keys); }",
        "node:tls/server-ticket-keys.js",
    )?;
    let ticket_keys = context.call_rooted(ticket_keys_factory, undefined, &[receiver])?;
    set(context, receiver, "setTicketKeys", ticket_keys)?;
    let get_ticket_keys_factory = context.evaluate_script_rooted(
        "server => function getTicketKeys() { return server._sharedCreds.context.getTicketKeys(); }",
        "node:tls/server-get-ticket-keys.js",
    )?;
    let get_ticket_keys = context.call_rooted(get_ticket_keys_factory, undefined, &[receiver])?;
    set(context, receiver, "getTicketKeys", get_ticket_keys)?;
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
