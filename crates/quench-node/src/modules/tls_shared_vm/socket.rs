//! TLS session operations share the existing physical net socket owner.
use crate::host::NodeHost;
use foreign_types::ForeignTypeRef;
use openssl::ssl::{Ssl, SslContextRef, SslSession, SslVerifyMode};
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn owner(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<u64, RootedError> {
    if let Some(id) = physical_owner(context, socket)? {
        return Ok(id);
    }
    let error = context.type_error_rooted("TLS socket has no native transport")?;
    Err(context.throw(error))
}
pub(crate) fn physical_owner(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<Option<u64>, RootedError> {
    let shared = context.host_mut().shared_state();
    let sockets = shared
        .borrow()
        .net_sockets
        .iter()
        .map(|(id, socket)| (*id, socket.root))
        .collect::<Vec<_>>();
    for (id, candidate) in sockets {
        if context.strict_equal_rooted(candidate, socket)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}
fn create_session(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    configuration: RootId,
    is_server: RootId,
    options: RootId,
) -> Result<(), RootedError> {
    let index = super::contexts::owner(context, configuration)?;
    let server = context.truthy_rooted(is_server)?;
    let mut server_names = Vec::new();
    if server {
        let server_object = get(context, options, "server")?;
        if context.truthy_rooted(server_object)? {
            let contexts = get(context, server_object, "_contexts")?;
            if context.truthy_rooted(contexts)? {
                let length = get(context, contexts, "length")?;
                let length = context
                    .rooted_value(length)
                    .and_then(|value| value.as_number())
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                for position in 0..length {
                    let entry_key = context.string_rooted(&position.to_string());
                    let entry = context.get_property_rooted(contexts, entry_key)?;
                    let name_key = context.string_rooted("0");
                    let name = context.get_property_rooted(entry, name_key)?;
                    let secure_key = context.string_rooted("1");
                    let secure = context.get_property_rooted(entry, secure_key)?;
                    let Some(name) = context.string_text(name)? else {
                        continue;
                    };
                    let secure = get(context, secure, "context")?;
                    let context_index = super::contexts::owner(context, secure)?;
                    let pointer = context
                        .host_mut()
                        .shared_state()
                        .borrow_mut()
                        .tls_contexts
                        .configuration(context_index)?
                        .as_ptr() as usize;
                    server_names.push((name, pointer));
                }
            }
        }
    }
    let shared = context.host_mut().shared_state();
    let ssl = {
        let mut state = shared.borrow_mut();
        let configuration = state.tls_contexts.configuration(index)?;
        // SSL_new retains SSL_CTX independently of the weak VM identity.
        let configuration = unsafe { SslContextRef::from_ptr(configuration.as_ptr()) };
        Ssl::new(configuration)
    }
    .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
    let mut ssl = ssl;
    if let Some(protocols) = encode_alpn_protocols(context, options)? {
        if server {
            let index = super::contexts::alpn_protocols_index()
                .map_err(|error| super::parser_error(context, error))?;
            ssl.set_ex_data(*index, protocols);
        } else {
            ssl.set_alpn_protos(&protocols)
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
    }
    if !server {
        let session = get(context, options, "session")?;
        if let Some(bytes) = context
            .view_bytes_rooted(session)
            .map(|bytes| bytes.to_vec())
        {
            let session = SslSession::from_der(&bytes)
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
            // The cached session is owned by the TLS Agent and may be shared
            // across connections, while each SSL takes its own native ref.
            unsafe { ssl.set_session(&session) }
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
    }
    if server {
        let index = super::contexts::server_names_index()
            .map_err(|error| RootedError::host(error.to_string()))?;
        ssl.set_ex_data(*index, server_names);
    }
    {
        let servername = get(context, options, "servername")?;
        if context.truthy_rooted(servername)? {
            let servername = context.to_string(servername)?;
            ssl.set_hostname(super::super::crypto_strings::null_terminated_prefix(
                &servername,
            ))
            .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
    }
    // Node records certificate verification failures and authorizes after secure.
    let mode = if server {
        let request = get(context, options, "requestCert")?;
        if context.truthy_rooted(request)? {
            let reject = get(context, options, "rejectUnauthorized")?;
            let mut mode = SslVerifyMode::PEER;
            if context.truthy_rooted(reject)? {
                mode |= SslVerifyMode::FAIL_IF_NO_PEER_CERT;
            }
            mode
        } else {
            SslVerifyMode::NONE
        }
    } else {
        // Preserve OpenSSL's verification result so Node can emit its normal
        // secure event and apply rejectUnauthorized in the JS layer.
        SslVerifyMode::PEER
    };
    ssl.set_verify_callback(mode, |_preverify, _store| true);
    let role = if server {
        super::super::net_shared_vm::encrypted::Role::Server
    } else {
        super::super::net_shared_vm::encrypted::Role::Client
    };
    super::sessions::register(context, socket, ssl, role)
}

fn encode_alpn_protocols(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
) -> Result<Option<Vec<u8>>, RootedError> {
    let value = get(context, options, "ALPNProtocols")?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(None);
    }
    if let Some(bytes) = context.view_bytes_rooted(value) {
        let bytes = bytes.to_vec();
        validate_alpn_wire(context, &bytes)?;
        return Ok(Some(bytes));
    }
    if !context.is_array_rooted(value)? {
        return Err(super::super::util_shared_vm::argument_type_error(
            context,
            value,
            "options.ALPNProtocols",
            "Array or ArrayBufferView",
        )?);
    }

    let length = get(context, value, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0)
        .max(0.0) as usize;
    let mut wire = Vec::new();
    for index in 0..length {
        let key = context.string_rooted(&index.to_string());
        let item = context.get_property_rooted(value, key)?;
        let Some(protocol) = context.string_text(item)? else {
            return Err(super::super::util_shared_vm::argument_type_error(
                context,
                item,
                "ALPNProtocols element",
                "string",
            )?);
        };
        let bytes = protocol.as_bytes();
        if bytes.is_empty() || bytes.len() > u8::MAX as usize {
            let error = context.type_error_rooted("Invalid ALPN protocol length")?;
            super::super::node_errors_shared_vm::decorate_type_error(
                context,
                error,
                "ERR_INVALID_ARG_VALUE",
            )?;
            return Err(context.throw(error));
        }
        wire.push(bytes.len() as u8);
        wire.extend_from_slice(bytes);
    }
    Ok(Some(wire))
}

fn validate_alpn_wire(
    context: &mut NativeContext<'_, NodeHost>,
    protocols: &[u8],
) -> Result<(), RootedError> {
    let mut offset = 0;
    while offset < protocols.len() {
        let length = usize::from(protocols[offset]);
        if length == 0 || offset + 1 + length > protocols.len() {
            let error = context.type_error_rooted("Invalid ALPN protocol list")?;
            super::super::node_errors_shared_vm::decorate_type_error(
                context,
                error,
                "ERR_INVALID_ARG_VALUE",
            )?;
            return Err(context.throw(error));
        }
        offset += 1 + length;
    }
    Ok(())
}

pub(crate) fn upgrade(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let root = args.first().copied().unwrap_or(receiver);
    let Some(socket) = physical_owner(context, root)? else {
        return Ok(context.undefined());
    };
    let index = super::sessions::owner(context, root)?;
    let shared = context.host_mut().shared_state();
    if !super::super::net_shared_vm::is_connected(&shared.borrow().tcp, socket) {
        return Ok(context.undefined());
    }
    let pending = {
        let mut state = shared.borrow_mut();
        let session = &mut state.tls_contexts.sessions.entries[index];
        session
            .pending
            .take()
            .map(|pending| (pending.ssl, pending.role, pending.start_requested))
    };
    let Some((ssl, role, started)) = pending else {
        return Ok(context.undefined());
    };
    let result = super::super::net_shared_vm::encrypt(
        &mut shared.borrow_mut().tcp,
        socket,
        ssl,
        role,
        started,
    );
    result.map_err(|error| match error {
        super::super::net_shared_vm::EncryptError::OpenSsl(error) => {
            super::super::crypto_error_shared_vm::decorated(context, error)
        }
        super::super::net_shared_vm::EncryptError::NotConnected => {
            RootedError::host("TLS transport is not connected")
        }
        super::super::net_shared_vm::EncryptError::AlreadyEncrypted => {
            RootedError::host("Nested TLS transport is not implemented")
        }
    })?;
    Ok(context.undefined())
}
pub(crate) fn authorize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = owner(context, args[0])?;
    let shared = context.host_mut().shared_state();
    super::super::net_shared_vm::authorize(&mut shared.borrow_mut().tcp, socket);
    Ok(context.undefined())
}
pub(crate) fn info(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = physical_owner(context, args[0])?;
    let session = super::sessions::owner(context, args[0])?;
    let name = context.to_string(args[1])?;
    let shared = context.host_mut().shared_state();
    enum Info {
        Text(String),
        Bool(bool),
        Cipher(Option<(String, String, String)>),
        Certificate(Option<openssl::x509::X509>),
        PeerCertificate(
            Option<openssl::x509::X509>,
            Option<Vec<openssl::x509::X509>>,
        ),
        Session(Option<Vec<u8>>),
        Verification(i32, String),
    }
    let detailed = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let value = {
        let state = shared.borrow();
        let ssl = socket
            .and_then(|id| super::super::net_shared_vm::tls_info(&state.tcp, id))
            .or_else(|| {
                state.tls_contexts.sessions.entries[session]
                    .pending
                    .as_ref()
                    .map(|pending| pending.ssl.as_ref())
            });
        let Some(ssl) = ssl else {
            return Ok(context.null());
        };
        match name.as_str() {
            "protocol" => Info::Text(ssl.version_str().to_owned()),
            "sessionReused" => Info::Bool(ssl.session_reused()),
            "cipher" => Info::Cipher(ssl.current_cipher().map(|cipher| {
                (
                    cipher.name().to_owned(),
                    cipher
                        .standard_name()
                        .unwrap_or_else(|| cipher.name())
                        .to_owned(),
                    cipher.version().to_owned(),
                )
            })),
            "session" => Info::Session(
                ssl.session()
                    .map(|session| session.to_der())
                    .transpose()
                    .map_err(|error| {
                        RootedError::host(format!("serialize TLS session: {error}"))
                    })?,
            ),
            "verifyError" => Info::Verification(
                ssl.verify_result().as_raw(),
                ssl.verify_result().error_string().to_owned(),
            ),
            "peerCertificate" => Info::PeerCertificate(
                ssl.peer_certificate(),
                detailed.then(|| {
                    let mut chain: Vec<openssl::x509::X509> = ssl
                        .peer_cert_chain()
                        .map(|chain| chain.iter().map(|cert| cert.to_owned()).collect())
                        .unwrap_or_default();
                    // The peer-supplied chain omits certificates completed
                    // from the local trust store. Node exposes those too in
                    // the detailed legacy certificate projection, so append
                    // OpenSSL's verified path; the projection below removes
                    // duplicates by DER identity.
                    if let Some(verified) = ssl.verified_chain() {
                        chain.extend(verified.iter().map(|cert| cert.to_owned()));
                    }
                    chain
                }),
            ),
            "certificate" => {
                Info::Certificate(ssl.certificate().map(|certificate| certificate.to_owned()))
            }
            _ => return Err(RootedError::host("TLS session query is not implemented")),
        }
    };
    match value {
        Info::Text(text) => Ok(context.string_rooted(&text)),
        Info::Bool(value) => Ok(context.boolean(value)),
        Info::Cipher(None) => Ok(context.null()),
        Info::Cipher(Some((name, standard_name, version))) => {
            let object = context.object_rooted()?;
            let name_value = context.string_rooted(&name);
            set(context, object, "name", name_value)?;
            let standard_value = context.string_rooted(&standard_name);
            set(context, object, "standardName", standard_value)?;
            let version_value = context.string_rooted(&version);
            set(context, object, "version", version_value)?;
            Ok(object)
        }
        Info::Certificate(None) => context.object_rooted(),
        Info::Certificate(Some(cert)) => super::certificate::legacy(context, &cert),
        Info::PeerCertificate(None, _) => context.object_rooted(),
        Info::PeerCertificate(Some(cert), None) => super::certificate::legacy(context, &cert),
        Info::PeerCertificate(Some(cert), Some(chain)) => {
            let leaf_der = cert.to_der().map_err(|error| {
                crate::modules::crypto_error_shared_vm::decorated(context, error)
            })?;
            let leaf = super::certificate::legacy(context, &cert)?;
            let mut child = leaf;
            let mut seen = vec![leaf_der];
            for issuer in chain {
                let der = issuer.to_der().map_err(|error| {
                    crate::modules::crypto_error_shared_vm::decorated(context, error)
                })?;
                if seen.contains(&der) {
                    continue;
                }
                seen.push(der);
                let issuer_object = super::certificate::legacy(context, &issuer)?;
                let key = context.string_rooted("issuerCertificate");
                if !context.define_data_property_rooted(
                    child,
                    key,
                    issuer_object,
                    true,
                    true,
                    true,
                )? {
                    return Err(RootedError::host(
                        "Cannot define TLS issuer certificate property",
                    ));
                }
                child = issuer_object;
            }
            // Node closes a self-signed peer chain with a circular link to
            // the leaf, which makes detailed certificate inspection
            // consistent with its toLegacyObject() projection.
            if super::certificate::multiline_name(cert.subject_name()).ok()
                == super::certificate::multiline_name(cert.issuer_name()).ok()
                && !context.has_own_property_rooted(leaf, "issuerCertificate")?
            {
                let key = context.string_rooted("issuerCertificate");
                if !context.define_data_property_rooted(leaf, key, leaf, true, true, true)? {
                    return Err(RootedError::host(
                        "Cannot define TLS issuer certificate property",
                    ));
                }
            }
            Ok(leaf)
        }
        Info::Session(None) => Ok(context.null()),
        Info::Session(Some(bytes)) => {
            let typed_array = context.uint8_array_rooted(&bytes)?;
            let global = context.global_root()?;
            let key = context.string_rooted("Buffer");
            let buffer = context.get_property_rooted(global, key)?;
            let key = context.string_rooted("from");
            let from = context.get_property_rooted(buffer, key)?;
            context.call_rooted(from, buffer, &[typed_array])
        }
        Info::Verification(code, _) if code == openssl_sys::X509_V_OK => Ok(context.null()),
        Info::Verification(code, reason) => verify_error(context, code, &reason),
    }
}

pub(super) fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
pub(super) fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}
pub(super) fn method(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = get(context, object, name)?;
    context.call_rooted(function, object, args)
}
fn copy_options(
    context: &mut NativeContext<'_, NodeHost>,
    target: RootId,
    source: RootId,
) -> Result<(), RootedError> {
    if context
        .rooted_value(source)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(());
    }
    for key in context.enumerable_own_keys_rooted(source, true, true)? {
        let value = context.get_property_rooted(source, key)?;
        context.define_data_property_rooted(target, key, value, true, true, true)?;
    }
    Ok(())
}
pub(crate) fn construct(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let receiver = args[0];
    let wrapped = args[2];
    let options = args[3];
    let is_server = get(context, options, "isServer")?;
    if context.truthy_rooted(is_server)? {
        let sni_callback = get(context, options, "SNICallback")?;
        if context.truthy_rooted(sni_callback)? && !context.is_callable_rooted(sni_callback)? {
            return Err(super::super::util_shared_vm::argument_type_error(
                context,
                sni_callback,
                "options.SNICallback",
                "function",
            )?);
        }
    }
    // Use the actual `new.target` prototype for TLS wrappers. In particular,
    // server-side accepted sockets are created by the TLS Server path rather
    // than by tls.connect(), and still need the same TLSSocket identity while
    // retaining TLSSocket.prototype's net.Socket prototype chain.
    let new_target = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let prototype = if context
        .rooted_value(new_target)
        .is_some_and(|value| value.is_undefined())
    {
        context.prototype_rooted(receiver)?
    } else {
        get(context, new_target, "prototype")?
    };
    let api = super::facade::install(context)?;
    let net = get(context, api, "identityNet")?;
    let base = get(context, net, "Socket")?;
    let net_options = context.object_rooted()?;
    for name in [
        "allowHalfOpen",
        "highWaterMark",
        "onread",
        "signal",
        "noDelay",
        "keepAlive",
        "keepAliveInitialDelay",
    ] {
        let mut value = get(context, options, name)?;
        if name == "allowHalfOpen"
            && !context
                .rooted_value(wrapped)
                .is_some_and(|value| value.is_undefined() || value.is_null())
        {
            // When a stream is supplied, Node inherits its half-open policy
            // from that stream and ignores the constructor option.
            value = get(context, wrapped, "allowHalfOpen")?;
        }
        // Node's net.Socket constructor initializes its internal option
        // symbols even when the TLS caller omits these settings. Project the
        // defaults through the same setter path so tls.connect() exposes
        // false/0 consistently on the wrapped socket.
        if context
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined())
        {
            value = match name {
                "noDelay" | "keepAlive" => context.boolean(false),
                "keepAliveInitialDelay" => context.number(0.0),
                _ => value,
            };
        }
        set(context, net_options, name, value)?;
    }
    let socket = context.construct_rooted(base, base, &[net_options])?;
    context.set_prototype_rooted(socket, prototype)?;
    let true_value = context.boolean(true);
    let false_value = context.boolean(false);
    let null = context.null();
    for (name, value) in [
        ("encrypted", true_value),
        ("secureConnecting", true_value),
        ("authorized", false_value),
        ("authorizationError", null),
        ("servername", null),
        ("alpnProtocol", null),
        ("_secureEstablished", false_value),
    ] {
        set(context, socket, name, value)?;
    }
    set(context, socket, "_tlsOptions", options)?;
    set(context, socket, "_controlReleased", false_value)?;
    let error = context.string_rooted("error");
    let listener = get(context, socket, "_tlsError")?;
    method(context, socket, "on", &[error, listener])?;
    let secure_context = get(context, options, "secureContext")?;
    let secure_context = if context.truthy_rooted(secure_context)? {
        secure_context
    } else {
        let module = get(context, api, "module")?;
        method(context, module, "createSecureContext", &[options])?
    };
    set(context, options, "secureContext", secure_context)?;
    let inner = get(context, secure_context, "context")?;
    create_session(context, socket, inner, is_server, options)?;
    // A directly constructed server-side TLSSocket is already in its final
    // role.  Only tls.Server's connection factory assigns Purpose::Server
    // after construction, so preserve the public `isServer` option here too;
    // secure() uses the purpose to publish ALPN and server-side authorization
    // state once the handshake completes.
    if context.truthy_rooted(is_server)? {
        super::sessions::set_purpose(context, socket, super::sessions::Purpose::Server)?;
    }
    if context.truthy_rooted(wrapped)? {
        set(context, socket, "_parent", wrapped)?;
        // Node permits TLSSocket to wrap streams before they acquire a native
        // transport (including a fresh net.Socket and generic Duplex). Keep
        // the parent relationship in that case; once a native socket exists,
        // transfer its identity and read handle to the TLS wrapper as usual.
        if let Some(id) = physical_owner(context, wrapped)? {
            let shared = context.host_mut().shared_state();
            let retained = context.retain(socket)?;
            let old = shared
                .borrow_mut()
                .net_sockets
                .get_mut(&id)
                .ok_or_else(|| RootedError::host("wrapped socket disappeared"))?
                .root;
            shared.borrow_mut().net_sockets.get_mut(&id).unwrap().root = retained;
            context.release_root(old);
            let id_root = context.number(id as f64);
            set(context, socket, "__quenchNetSocketId", id_root)?;
            let parent_handle = get(context, wrapped, "_handle")?;
            let reading = get(context, parent_handle, "reading")?;
            super::super::net_reference_shared_vm::install_socket_handle(context, id, socket)?;
            let handle = get(context, socket, "_handle")?;
            set(context, handle, "reading", reading)?;
            super::super::net_socket_read::redirect_parent_reading(context, parent_handle, handle)?;
            // Wrapping an in-flight net.Socket must retain its connect state.
            // TLS initialization schedules a read immediately; clearing these
            // flags makes it call readStart before the transport owns a connected
            // stream, which surfaces EINVAL instead of waiting for `connect`.
            let connecting = get(context, wrapped, "connecting")?;
            let pending = get(context, wrapped, "pending")?;
            set(context, socket, "connecting", connecting)?;
            set(context, socket, "pending", pending)?;
        }
    }
    super::handle::install(context, socket)?;
    let secure = context.string_rooted("secure");
    let callback = context.host_function(crate::host::shared_vm::operation("tlsSocketSecure"))?;
    method(context, socket, "on", &[secure, callback])?;
    let connect = context.string_rooted("connect");
    let callback = context.host_function(crate::host::shared_vm::operation("tlsSocketUpgrade"))?;
    method(context, socket, "once", &[connect, callback])?;
    if context.truthy_rooted(wrapped)? {
        // Node allows construction around an unconnected net.Socket or a
        // generic Duplex. Such objects have no physical TCP owner yet, so
        // there is no native session to upgrade at construction time.
        if let Some(id) = physical_owner(context, socket)? {
            let connected = super::super::net_shared_vm::is_connected(
                &context.host_mut().shared_state().borrow().tcp,
                id,
            );
            if connected {
                let undefined = context.undefined();
                upgrade(context, undefined, &[socket])?;
            }
        }
    }
    if context.truthy_rooted(is_server)? {
        let undefined = context.undefined();
        start(context, undefined, &[socket])?;
    }
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let init = context.host_function(crate::host::shared_vm::operation("tlsSocketInitRead"))?;
    method(context, process, "nextTick", &[init, socket, wrapped])?;
    Ok(socket)
}
pub(crate) fn init_read(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    if physical_owner(context, socket)?.is_none() {
        return Ok(context.undefined());
    }
    let handle = get(context, socket, "_handle")?;
    if !context.truthy_rooted(handle)? {
        return Ok(context.undefined());
    }
    let wrapped = args[1];
    if context.truthy_rooted(wrapped)? {
        let length = get(context, wrapped, "readableLength")?;
        if context.truthy_rooted(length)? {
            loop {
                let chunk = method(context, wrapped, "read", &[])?;
                if context
                    .rooted_value(chunk)
                    .is_some_and(|value| value.is_null())
                {
                    break;
                }
                let bytes = context
                    .view_bytes_rooted(chunk)
                    .ok_or_else(|| RootedError::host("TLS buffered input is not a byte view"))?;
                let id = owner(context, socket)?;
                super::super::net_shared_vm::receive_tls(
                    &mut context.host_mut().shared_state().borrow_mut().tcp,
                    id,
                    &bytes,
                );
            }
        }
    }
    let zero = context.number(0.0);
    method(context, socket, "read", &[zero])?;
    Ok(context.undefined())
}
pub(crate) fn connect(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let api = super::facade::install(context)?;
    let module = get(context, api, "module")?;
    let options = context.object_rooted()?;
    let undefined = context.undefined();
    let default_min_dh_size = context.number(1024.0);
    set(context, options, "minDHSize", default_min_dh_size)?;
    let first = args.first().copied().unwrap_or(undefined);
    if context.is_object_rooted(first)? {
        copy_options(context, options, first)?
    } else {
        // Match net.connect's overload: a non-numeric string is a local
        // socket path, while a numeric string is a TCP port.
        let is_path = if context.string_text(first)?.is_some() {
            let zero = context.number(0.0);
            let numeric = context.subtract_rooted(first, zero)?;
            !context
                .rooted_value(numeric)
                .and_then(|value| value.as_number())
                .is_some_and(|number| number >= 0.0)
        } else {
            false
        };
        set(context, options, if is_path { "path" } else { "port" }, first)?;
    }
    let second = args.get(1).copied().unwrap_or(undefined);
    // Node's `normalizeConnectArgs()` interprets a second string as the host
    // only for the `(port, host)` overload. When the first argument is an
    // options object, a trailing string is ignored.
    if context.string_text(second)?.is_some() && !context.is_object_rooted(first)? {
        set(context, options, "host", second)?;
    } else if context.is_object_rooted(second)? && !context.is_callable_rooted(second)? {
        copy_options(context, options, second)?;
    }
    if let Some(third) = args.get(2).copied() {
        if context.is_object_rooted(third)? && !context.is_callable_rooted(third)? {
            copy_options(context, options, third)?;
        }
    }
    // `tls.connect()` passes the effective default cipher list through to
    // createSecureContext(). Node exposes this in the options object, and
    // callers that wrap createSecureContext rely on seeing the same default
    // as `tls.DEFAULT_CIPHERS`.
    let ciphers = get(context, options, "ciphers")?;
    if context
        .rooted_value(ciphers)
        .is_some_and(|value| value.is_undefined())
    {
        let default_ciphers = get(context, module, "DEFAULT_CIPHERS")?;
        set(context, options, "ciphers", default_ciphers)?;
    }
    let check_was_provided = context.has_own_property_rooted(options, "checkServerIdentity")?;
    if !check_was_provided {
        let check = get(context, module, "checkServerIdentity")?;
        set(context, options, "checkServerIdentity", check)?;
    }
    let check = get(context, options, "checkServerIdentity")?;
    if !context.is_callable_rooted(check)? {
        return Err(super::super::util_shared_vm::argument_type_error(
            context,
            check,
            "options.checkServerIdentity",
            "function",
        )?);
    }
    // Validate TLS-only connect options before delegating endpoint validation
    // to net.Socket.connect(). Node reports an invalid minDHSize even when
    // the caller also omitted the port, so the TLS option error takes
    // precedence over the later missing-endpoint error.
    let min_dh_size = get(context, options, "minDHSize")?;
    let Some(number) = context
        .rooted_value(min_dh_size)
        .and_then(|value| value.as_number())
    else {
        return Err(super::super::util_shared_vm::argument_type_error(
            context,
            min_dh_size,
            "options.minDHSize",
            "number",
        )?);
    };
    // `validateNumber()` accepts positive fractional values and +Infinity;
    // it rejects values below one and NaN, but applies no upper bound.
    if number < 1.0 || number.is_nan() {
        let received = super::super::util_shared_vm::range_error_received(context, min_dh_size)?;
        let error = context.range_error_rooted(&format!(
            "The value of \"options.minDHSize\" is out of range. It must be >= 1. Received {received}"
        ))?;
        let key = context.string_rooted("code");
        let code = context.string_rooted("ERR_OUT_OF_RANGE");
        context.set_property_rooted(error, key, code, error)?;
        return Err(context.throw(error));
    }
    let servername = get(context, options, "servername")?;
    if context.truthy_rooted(servername)? {
        let net = get(context, api, "identityNet")?;
        let is_ip = get(context, net, "isIP")?;
        let is_ip = context.call_rooted(is_ip, net, &[servername])?;
        if context.truthy_rooted(is_ip)? {
            return Err(super::super::util_shared_vm::argument_value_error(
                context,
                servername,
                "options.servername",
                "Setting the TLS ServerName to an IP address is not permitted.",
            )?);
        }
    }
    let reject = get(context, options, "rejectUnauthorized")?;
    let reject = !context
        .rooted_value(reject)
        .is_some_and(|value| value.as_bool() == Some(false));
    let reject = context.boolean(reject);
    set(context, options, "rejectUnauthorized", reject)?;
    let constructor = get(context, api, "socketConstructor")?;
    let wrapped = get(context, options, "socket")?;
    let socket = context.construct_rooted(constructor, constructor, &[wrapped, options])?;
    super::sessions::set_purpose(context, socket, super::sessions::Purpose::ClientConnect)?;
    method(context, socket, "_releaseControl", &[])?;
    // Node's tls.connect() applies the timeout option to a TLS socket that it
    // creates itself before starting the underlying connection. The generic
    // TLSSocket constructor intentionally forwards only its documented socket
    // options, so project this connect-only setting here as well.
    let timeout = get(context, options, "timeout")?;
    if context.truthy_rooted(timeout)? {
        method(context, socket, "setTimeout", &[timeout])?;
    }
    if let Some(callback) = args.last().copied() {
        if context.is_callable_rooted(callback)? {
            let event = context.string_rooted("secureConnect");
            method(context, socket, "once", &[event, callback])?;
        }
    }
    if context.truthy_rooted(wrapped)? {
        start(context, undefined, &[socket])?;
    } else {
        method(context, socket, "connect", &[options])?;
        start(context, undefined, &[socket])?;
    }
    Ok(socket)
}
pub(crate) fn start(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args.first().copied().unwrap_or(receiver);
    let index = super::sessions::owner(context, socket)?;
    if let Some(pending) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .tls_contexts
        .sessions
        .entries[index]
        .pending
        .as_mut()
    {
        pending.start_requested = true;
    }
    let undefined = context.undefined();
    upgrade(context, undefined, &[socket])?;
    if let Some(id) = physical_owner(context, socket)? {
        super::super::net_shared_vm::start_tls(
            &mut context.host_mut().shared_state().borrow_mut().tcp,
            id,
        );
    }
    Ok(context.undefined())
}
pub(crate) fn start_for_write(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<(), RootedError> {
    let Some(index) = super::sessions::find_owner(context, socket)? else {
        return Ok(());
    };
    let id = physical_owner(context, socket)?;
    let shared = context.host_mut().shared_state();
    let needed = {
        let state = shared.borrow();
        state.tls_contexts.sessions.entries[index]
            .pending
            .as_ref()
            .is_some_and(|pending| !pending.start_requested)
            || id.is_some_and(|id| super::super::net_shared_vm::tls_is_dormant(&state.tcp, id))
    };
    if needed {
        let undefined = context.undefined();
        start(context, undefined, &[socket])?;
    }
    Ok(())
}
pub(super) fn verify_error(
    context: &mut NativeContext<'_, NodeHost>,
    result: i32,
    reason: &str,
) -> Result<RootId, RootedError> {
    let code = match result {
        openssl_sys::X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT => "UNABLE_TO_GET_ISSUER_CERT",
        openssl_sys::X509_V_ERR_UNABLE_TO_GET_CRL => "UNABLE_TO_GET_CRL",
        openssl_sys::X509_V_ERR_UNABLE_TO_DECRYPT_CERT_SIGNATURE => {
            "UNABLE_TO_DECRYPT_CERT_SIGNATURE"
        }
        openssl_sys::X509_V_ERR_UNABLE_TO_DECRYPT_CRL_SIGNATURE => {
            "UNABLE_TO_DECRYPT_CRL_SIGNATURE"
        }
        openssl_sys::X509_V_ERR_UNABLE_TO_DECODE_ISSUER_PUBLIC_KEY => {
            "UNABLE_TO_DECODE_ISSUER_PUBLIC_KEY"
        }
        openssl_sys::X509_V_ERR_CERT_SIGNATURE_FAILURE => "CERT_SIGNATURE_FAILURE",
        openssl_sys::X509_V_ERR_CRL_SIGNATURE_FAILURE => "CRL_SIGNATURE_FAILURE",
        openssl_sys::X509_V_ERR_CERT_NOT_YET_VALID => "CERT_NOT_YET_VALID",
        openssl_sys::X509_V_ERR_CERT_HAS_EXPIRED => "CERT_HAS_EXPIRED",
        openssl_sys::X509_V_ERR_CRL_NOT_YET_VALID => "CRL_NOT_YET_VALID",
        openssl_sys::X509_V_ERR_CRL_HAS_EXPIRED => "CRL_HAS_EXPIRED",
        openssl_sys::X509_V_ERR_ERROR_IN_CERT_NOT_BEFORE_FIELD => "ERROR_IN_CERT_NOT_BEFORE_FIELD",
        openssl_sys::X509_V_ERR_ERROR_IN_CERT_NOT_AFTER_FIELD => "ERROR_IN_CERT_NOT_AFTER_FIELD",
        openssl_sys::X509_V_ERR_ERROR_IN_CRL_LAST_UPDATE_FIELD => "ERROR_IN_CRL_LAST_UPDATE_FIELD",
        openssl_sys::X509_V_ERR_ERROR_IN_CRL_NEXT_UPDATE_FIELD => "ERROR_IN_CRL_NEXT_UPDATE_FIELD",
        openssl_sys::X509_V_ERR_OUT_OF_MEM => "OUT_OF_MEM",
        openssl_sys::X509_V_ERR_DEPTH_ZERO_SELF_SIGNED_CERT => "DEPTH_ZERO_SELF_SIGNED_CERT",
        openssl_sys::X509_V_ERR_SELF_SIGNED_CERT_IN_CHAIN => "SELF_SIGNED_CERT_IN_CHAIN",
        openssl_sys::X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT_LOCALLY => {
            "UNABLE_TO_GET_ISSUER_CERT_LOCALLY"
        }
        openssl_sys::X509_V_ERR_UNABLE_TO_VERIFY_LEAF_SIGNATURE => {
            "UNABLE_TO_VERIFY_LEAF_SIGNATURE"
        }
        openssl_sys::X509_V_ERR_CERT_CHAIN_TOO_LONG => "CERT_CHAIN_TOO_LONG",
        openssl_sys::X509_V_ERR_CERT_REVOKED => "CERT_REVOKED",
        openssl_sys::X509_V_ERR_INVALID_CA => "INVALID_CA",
        openssl_sys::X509_V_ERR_PATH_LENGTH_EXCEEDED => "PATH_LENGTH_EXCEEDED",
        openssl_sys::X509_V_ERR_INVALID_PURPOSE => "INVALID_PURPOSE",
        openssl_sys::X509_V_ERR_CERT_UNTRUSTED => "CERT_UNTRUSTED",
        openssl_sys::X509_V_ERR_CERT_REJECTED => "CERT_REJECTED",
        openssl_sys::X509_V_ERR_HOSTNAME_MISMATCH => "HOSTNAME_MISMATCH",
        _ => "UNSPECIFIED",
    };

    // Node crypto_common.cc adds this advice for the native chain result.
    let use_system_ca = context
        .host_mut()
        .shared_state()
        .borrow()
        .exec_argv
        .iter()
        .any(|arg| arg == "--use-system-ca");
    let suggest_system_ca = matches!(
        result,
        openssl_sys::X509_V_ERR_UNABLE_TO_VERIFY_LEAF_SIGNATURE
            | openssl_sys::X509_V_ERR_DEPTH_ZERO_SELF_SIGNED_CERT
    ) || result == openssl_sys::X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT
        && !use_system_ca;
    let reason = if suggest_system_ca {
        format!(
            "{reason}; if the root CA is installed locally, try running Node.js with --use-system-ca"
        )
    } else {
        reason.to_owned()
    };
    let error = context.error_rooted(&reason)?;
    let code = context.string_rooted(code);
    set(context, error, "code", code)?;
    Ok(error)
}
pub(crate) fn secure(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = owner(context, socket)?;
    let session = super::sessions::owner(context, socket)?;
    let shared = context.host_mut().shared_state();
    let purpose = shared.borrow().tls_contexts.sessions.entries[session].purpose;
    let (result, reason, peer, reused, servername, alpn) = {
        let state = shared.borrow();
        let ssl = super::super::net_shared_vm::tls_info(&state.tcp, id)
            .ok_or_else(|| RootedError::host("TLS session disappeared"))?;
        (
            ssl.verify_result().as_raw(),
            ssl.verify_result().error_string().to_owned(),
            ssl.peer_certificate(),
            ssl.session_reused(),
            super::contexts::peer_servername_index()
                .ok()
                .and_then(|index| ssl.ex_data(*index).cloned())
                .or_else(|| {
                    ssl.servername(openssl::ssl::NameType::HOST_NAME)
                        .map(str::to_owned)
                }),
            ssl.selected_alpn_protocol().map(|bytes| {
                bytes
                    .iter()
                    .map(|byte| char::from(*byte))
                    .collect::<String>()
            }),
        )
    };
    let options = get(context, socket, "_tlsOptions")?;
    if matches!(purpose, super::sessions::Purpose::Direct) {
        let yes = context.boolean(true);
        set(context, socket, "_secureEstablished", yes)?;
        super::super::net_shared_vm::authorize(&mut shared.borrow_mut().tcp, id);
        return Ok(context.undefined());
    }
    if matches!(purpose, super::sessions::Purpose::Server) {
        let protocol = alpn
            .map(|protocol| context.string_rooted(&protocol))
            .unwrap_or_else(|| context.boolean(false));
        set(context, socket, "alpnProtocol", protocol)?;
        // Node represents an absent server-side SNI name as false; null
        // makes checks such as servername !== false behave differently and
        // can leak a literal "null" into response headers.
        let servername = servername
            .map(|text| context.string_rooted(&text))
            .unwrap_or_else(|| context.boolean(false));
        set(context, socket, "servername", servername)?;
        let no = context.boolean(false);
        set(context, socket, "authorized", no)?;
        let request = get(context, options, "requestCert")?;
        if context.truthy_rooted(request)? {
            let failure = if peer.is_none() {
                Some((
                    openssl_sys::X509_V_ERR_UNABLE_TO_GET_ISSUER_CERT,
                    "unable to get issuer certificate".to_owned(),
                ))
            } else if result != openssl_sys::X509_V_OK {
                Some((result, reason.clone()))
            } else {
                None
            };
            if let Some((code, reason)) = failure {
                let error = verify_error(context, code, &reason)?;
                let code = get(context, error, "code")?;
                set(context, socket, "authorizationError", code)?;
                let reject = get(context, options, "rejectUnauthorized")?;
                if context.truthy_rooted(reject)? {
                    method(context, socket, "destroy", &[])?;
                    return Ok(context.undefined());
                }
            } else {
                let yes = context.boolean(true);
                set(context, socket, "authorized", yes)?;
            }
        }
        let no = context.boolean(false);
        let yes = context.boolean(true);
        set(context, socket, "secureConnecting", no)?;
        set(context, socket, "_secureEstablished", yes)?;
        super::super::net_shared_vm::authorize(&mut shared.borrow_mut().tcp, id);
        let server = get(context, options, "server")?;
        let event = context.string_rooted("secureConnection");
        let released = method(context, socket, "_releaseControl", &[])?;
        if context.truthy_rooted(released)? {
            method(context, server, "emit", &[event, socket])?;
        }
        return Ok(context.undefined());
    }
    let error = if result != openssl_sys::X509_V_OK {
        Some(verify_error(context, result, &reason)?)
    } else if !reused {
        let peer = peer.ok_or_else(|| RootedError::host("TLS peer certificate missing"))?;
        let cert = super::certificate::legacy(context, &peer)?;
        if super::certificate::multiline_name(peer.subject_name()).ok()
            == super::certificate::multiline_name(peer.issuer_name()).ok()
        {
            let key = context.string_rooted("issuerCertificate");
            if !context.define_data_property_rooted(cert, key, cert, true, true, true)? {
                return Err(RootedError::host(
                    "Cannot define TLS issuer certificate property",
                ));
            }
        }
        let check = get(context, options, "checkServerIdentity")?;
        let servername = get(context, options, "servername")?;
        let hostname = if context.truthy_rooted(servername)? {
            servername
        } else {
            get(context, options, "host")?
        };
        let hostname = if context.truthy_rooted(hostname)? {
            hostname
        } else {
            context.string_rooted("localhost")
        };
        let result = context.call_rooted(check, options, &[hostname, cert])?;
        if context.truthy_rooted(result)? {
            Some(result)
        } else {
            None
        }
    } else {
        None
    };
    let authorized = context.boolean(error.is_none());
    set(context, socket, "authorized", authorized)?;
    if let Some(error) = error {
        let code = get(context, error, "code")?;
        let code = if context.truthy_rooted(code)? {
            code
        } else {
            get(context, error, "message")?
        };
        set(context, socket, "authorizationError", code)?;
        let reject = get(context, options, "rejectUnauthorized")?;
        if !context
            .rooted_value(reject)
            .is_some_and(|value| value.as_bool() == Some(false))
        {
            method(context, socket, "destroy", &[error])?;
            return Ok(context.undefined());
        }
    }
    let false_value = context.boolean(false);
    let true_value = context.boolean(true);
    set(context, socket, "secureConnecting", false_value)?;
    set(context, socket, "_secureEstablished", true_value)?;
    let servername = servername
        .map(|text| context.string_rooted(&text))
        .unwrap_or_else(|| context.null());
    set(context, socket, "servername", servername)?;
    let alpn = alpn
        .map(|text| context.string_rooted(&text))
        .unwrap_or_else(|| context.boolean(false));
    set(context, socket, "alpnProtocol", alpn)?;
    super::super::net_shared_vm::authorize(&mut shared.borrow_mut().tcp, id);
    let event = context.string_rooted("secureConnect");
    method(context, socket, "emit", &[event])?;
    Ok(context.undefined())
}

pub(crate) fn tls_error(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let error = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let event = context.string_rooted("_tlsError");
    method(context, socket, "emit", &[event, error])?;
    let released = get(context, socket, "_controlReleased")?;
    Ok(if context.truthy_rooted(released)? {
        error
    } else {
        context.null()
    })
}
pub(crate) fn release_control(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = args[0];
    let released = get(context, socket, "_controlReleased")?;
    if context.truthy_rooted(released)? {
        return Ok(context.boolean(false));
    }
    let yes = context.boolean(true);
    set(context, socket, "_controlReleased", yes)?;
    let error = context.string_rooted("error");
    let callback = get(context, socket, "_tlsError")?;
    method(context, socket, "removeListener", &[error, callback])?;
    Ok(yes)
}
pub(crate) fn transport_error(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = context
        .rooted_value(args[0])
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("TLS error owner missing"))? as u64;
    let shared = context.host_mut().shared_state();
    let stack = {
        let mut state = shared.borrow_mut();
        let index = state
            .tls_contexts
            .transport_errors
            .iter()
            .position(|(owner, _)| *owner == id)
            .ok_or_else(|| RootedError::host("TLS error unavailable"))?;
        state.tls_contexts.transport_errors.remove(index).unwrap().1
    };
    let error = super::super::crypto_error_shared_vm::create(context, &stack, true)?;
    method(context, socket, "destroy", &[error])?;
    Ok(context.undefined())
}
