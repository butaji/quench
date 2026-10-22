//! TLS session operations share the existing physical net socket owner.
use crate::host::NodeHost;
use foreign_types::ForeignTypeRef;
use openssl::ssl::{Ssl, SslContextRef, SslVerifyMode};
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
        SslVerifyMode::NONE
    };
    ssl.set_verify_callback(mode, |_preverify, _store| true);
    let role = if server {
        super::super::net_shared_vm::encrypted::Role::Server
    } else {
        super::super::net_shared_vm::encrypted::Role::Client
    };
    super::sessions::register(context, socket, ssl, role)
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
        Certificate(Option<openssl::x509::X509>),
        Verification(i32, String),
    }
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
            "verifyError" => {
                Info::Verification(ssl.verify_result().as_raw(), ssl.verify_result().error_string().to_owned())
            }
            "peerCertificate" => Info::Certificate(ssl.peer_certificate()),
            _ => return Err(RootedError::host("TLS session query is not implemented")),
        }
    };
    match value {
        Info::Text(text) => Ok(context.string_rooted(&text)),
        Info::Bool(value) => Ok(context.boolean(value)),
        Info::Certificate(None) => context.object_rooted(),
        Info::Certificate(Some(cert)) => super::certificate::legacy(context, &cert),
        Info::Verification(code, _) if code == openssl_sys::X509_V_OK => Ok(context.null()),
        Info::Verification(code, reason) => verify_error(context, code, &reason),
    }
}

pub(super) fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
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
    let prototype = context.prototype_rooted(receiver)?;
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
        let value = get(context, options, name)?;
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
    let is_server = get(context, options, "isServer")?;
    create_session(context, socket, inner, is_server, options)?;
    if context.truthy_rooted(wrapped)? {
        let id = owner(context, wrapped)?;
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
        set(context, socket, "_parent", wrapped)?;
        let false_value = context.boolean(false);
        set(context, socket, "connecting", false_value)?;
        set(context, socket, "pending", false_value)?;
    }
    super::handle::install(context, socket)?;
    let secure = context.string_rooted("secure");
    let callback = context.host_function(crate::host::shared_vm::operation("tlsSocketSecure"))?;
    method(context, socket, "on", &[secure, callback])?;
    let connect = context.string_rooted("connect");
    let callback = context.host_function(crate::host::shared_vm::operation("tlsSocketUpgrade"))?;
    method(context, socket, "once", &[connect, callback])?;
    if context.truthy_rooted(wrapped)? {
        let id = owner(context, socket)?;
        let connected = super::super::net_shared_vm::is_connected(
            &context.host_mut().shared_state().borrow().tcp,
            id,
        );
        if connected {
            let undefined = context.undefined();
            upgrade(context, undefined, &[socket])?;
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
    if physical_owner(context, socket)?.is_none() { return Ok(context.undefined()); }
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
    let first = args.first().copied().unwrap_or(undefined);
    if context.is_object_rooted(first)? {
        copy_options(context, options, first)?
    } else {
        set(context, options, "port", first)?;
    }
    let second = args.get(1).copied().unwrap_or(undefined);
    if context.string_text(second)?.is_some() {
        set(context, options, "host", second)?;
    } else if context.is_object_rooted(second)? && !context.is_callable_rooted(second)? {
        copy_options(context, options, second)?;
    }
    if let Some(third) = args.get(2).copied() {
        if context.is_object_rooted(third)? && !context.is_callable_rooted(third)? {
            copy_options(context, options, third)?;
        }
    }
    let check = get(context, options, "checkServerIdentity")?;
    if context
        .rooted_value(check)
        .is_some_and(|value| value.is_undefined())
    {
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
pub(crate) fn start_for_write(context: &mut NativeContext<'_, NodeHost>, socket: RootId) -> Result<(), RootedError> {
    let Some(index) = super::sessions::find_owner(context, socket)? else { return Ok(()); };
    let id = physical_owner(context, socket)?;
    let shared = context.host_mut().shared_state();
    let needed = {
        let state = shared.borrow();
        state.tls_contexts.sessions.entries[index].pending.as_ref().is_some_and(|pending| !pending.start_requested)
            || id.is_some_and(|id| super::super::net_shared_vm::tls_is_dormant(&state.tcp, id))
    };
    if needed { let undefined = context.undefined(); start(context, undefined, &[socket])?; }
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
            ssl.servername(openssl::ssl::NameType::HOST_NAME)
                .map(str::to_owned),
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
