//! The public `tls` surface (lib/tls.js, lib/_tls_wrap.js, lib/_tls_common.js): the
//! `SecureContext` facades, `TLSSocket` and `Server` over the native session operations,
//! and the module functions.

use super::socket::{get, method, set};
use crate::host::NodeHost;
use crate::modules::host_members::{accessors, constructor, current, function, members, methods};
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

members!(Surface, "tlsSurface", [
    NativeSecureContext,
    NativeExternal,
    SecureContext,
    CreateSecureContext,
    TlsSocket,
    Server,
    CreateServer,
    Connect,
    GetCiphers,
    ConvertAlpnProtocols,
    GetCaCertificates,
    SetDefaultCaCertificates,
    CheckServerIdentity,
    GetProtocol,
    GetPeerCertificate,
    GetCertificate,
    GetPeerX509Certificate,
    GetX509Certificate,
    GetCipher,
    GetSession,
    GetTlsTicket,
    IsSessionReused,
    DestroySoon,
    Start,
    TlsError,
    ReleaseControl,
    ServerRejection,
    SetSecureContext,
    SetTicketKeys,
    GetTicketKeys,
    AddContext,
    /// `tls.connect`'s `onConnectEnd` and its reset-error projection, on the socket.
    ConnectEnd,
    ConnectError,
]);

/// The largest ALPN protocol name, in bytes.
const MAX_ALPN_PROTOCOL: usize = u8::MAX as usize;
/// Node's renegotiation limits (`CLIENT_RENEG_LIMIT`, `CLIENT_RENEG_WINDOW` seconds).
const CLIENT_RENEG_LIMIT: f64 = 3.0;
const CLIENT_RENEG_WINDOW: f64 = 600.0;
/// The certificate stores `getCACertificates` names.
const BUNDLED_STORE: &str = "bundled";
const DEFAULT_STORE: &str = "default";

fn absent(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_null() || value.is_undefined())
}

/// The context facades (`NativeSecureContext`, `SecureContext`, `createSecureContext`).
pub(crate) fn context_facades(c: &mut Context<'_>) -> Outcome {
    let native = constructor(c, Surface::NativeSecureContext, "SecureContext")?;
    let prototype = get(c, native, "prototype")?;
    accessors(c, prototype, &[("_external", Surface::NativeExternal, None)])?;
    let secure_context = constructor(c, Surface::SecureContext, "SecureContext")?;
    let create = function(c, Surface::CreateSecureContext, "createSecureContext", &[])?;
    let api = c.object_rooted()?;
    set(c, api, "NativeSecureContext", native)?;
    set(c, api, "SecureContext", secure_context)?;
    set(c, api, "createSecureContext", create)?;
    let cache = c.null_object_rooted()?;
    set(c, api, "caCertificates", cache)?;
    Ok(api)
}

/// The `tls` module: its constants and functions, `TLSSocket` and `Server`.
pub(crate) fn build(c: &mut Context<'_>, api: RootId, net: RootId, defaults: &[(&str, String)]) -> Outcome {
    let socket = constructor(c, Surface::TlsSocket, "TLSSocket")?;
    let prototype = get(c, socket, "prototype")?;
    methods(c, prototype, &[
        ("getProtocol", Surface::GetProtocol),
        ("getPeerCertificate", Surface::GetPeerCertificate),
        ("getCertificate", Surface::GetCertificate),
        ("getPeerX509Certificate", Surface::GetPeerX509Certificate),
        ("getX509Certificate", Surface::GetX509Certificate),
        ("getCipher", Surface::GetCipher),
        ("getSession", Surface::GetSession),
        ("getTLSTicket", Surface::GetTlsTicket),
        ("isSessionReused", Surface::IsSessionReused),
        ("destroySoon", Surface::DestroySoon),
        ("_start", Surface::Start),
        ("_tlsError", Surface::TlsError),
        ("_releaseControl", Surface::ReleaseControl),
    ])?;
    inherit(c, socket, net, "Socket")?;
    let server = constructor(c, Surface::Server, "Server")?;
    let prototype = get(c, server, "prototype")?;
    let rejection = {
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let key = c.string_rooted("nodejs.rejection");
        method(c, symbol, "for", &[key])?
    };
    let handler = function(c, Surface::ServerRejection, "", &[])?;
    c.define_data_property_rooted(prototype, rejection, handler, true, true, true)?;
    methods(c, prototype, &[
        ("setSecureContext", Surface::SetSecureContext),
        ("setTicketKeys", Surface::SetTicketKeys),
        ("getTicketKeys", Surface::GetTicketKeys),
        ("addContext", Surface::AddContext),
    ])?;
    inherit(c, server, net, "Server")?;
    let module = c.object_rooted()?;
    let limit = c.number(CLIENT_RENEG_LIMIT);
    set(c, module, "CLIENT_RENEG_LIMIT", limit)?;
    let window = c.number(CLIENT_RENEG_WINDOW);
    set(c, module, "CLIENT_RENEG_WINDOW", window)?;
    for (name, value) in defaults {
        let value = c.string_rooted(value);
        set(c, module, name, value)?;
    }
    let ciphers = function(c, Surface::GetCiphers, "getCiphers", &[])?;
    set(c, module, "getCiphers", ciphers)?;
    super::bundled::install(c, module)?;
    for (name, member) in [
        ("getCACertificates", Surface::GetCaCertificates),
        ("setDefaultCACertificates", Surface::SetDefaultCaCertificates),
        ("convertALPNProtocols", Surface::ConvertAlpnProtocols),
        ("checkServerIdentity", Surface::CheckServerIdentity),
    ] {
        let value = function(c, member, name, &[])?;
        set(c, module, name, value)?;
    }
    let create = get(c, api, "createSecureContext")?;
    set(c, module, "createSecureContext", create)?;
    let secure_context = get(c, api, "SecureContext")?;
    set(c, module, "SecureContext", secure_context)?;
    set(c, module, "TLSSocket", socket)?;
    set(c, module, "Server", server)?;
    let create_server = function(c, Surface::CreateServer, "createServer", &[])?;
    set(c, module, "createServer", create_server)?;
    let connect = function(c, Surface::Connect, "connect", &[])?;
    set(c, module, "connect", connect)?;
    Ok(module)
}

fn inherit(c: &mut Context<'_>, class: RootId, net: RootId, base: &str) -> Result<(), RootedError> {
    let base = get(c, net, base)?;
    let prototype = get(c, class, "prototype")?;
    let base_prototype = get(c, base, "prototype")?;
    c.set_prototype_rooted(prototype, base_prototype)?;
    c.set_prototype_rooted(class, base)?;
    Ok(())
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Surface>(c)? {
        Surface::NativeSecureContext => super::contexts::register(c, undefined, &[this]),
        Surface::NativeExternal => {
            let error = c.type_error_rooted("Illegal invocation")?;
            Err(c.throw(error))
        }
        Surface::SecureContext => {
            let class = get_api(c, "SecureContext")?;
            let existing = c.is_object_rooted(this)? && c.instance_of_rooted(this, class)?;
            let existing = c.boolean(existing);
            super::facade::initialize(c, undefined, &[this, existing, at(0), at(1), at(2), at(3)])
        }
        Surface::CreateSecureContext => super::facade::create(c, undefined, &[at(0)]),
        Surface::TlsSocket => {
            let options = c.object_rooted()?;
            let given = at(1);
            if c.is_object_rooted(given)? {
                for key in c.enumerable_own_keys_rooted(given, true, true)? {
                    let value = c.get_property_rooted(given, key)?;
                    c.set_property_rooted(options, key, value, options)?;
                }
            }
            let target = c.new_target_rooted().unwrap_or(undefined);
            super::socket::construct(c, undefined, &[this, target, at(0), options])
        }
        Surface::Server => {
            let class = get_api(c, "serverConstructor")?;
            let initializes = c.is_object_rooted(this)? && c.instance_of_rooted(this, class)?;
            let target = c.new_target_rooted().unwrap_or(if initializes { class } else { undefined });
            super::server::construct(c, undefined, &[this, target, at(0), at(1)])
        }
        Surface::CreateServer => super::server::create(c, undefined, &[at(0), at(1)]),
        Surface::Connect => connect(c, args),
        Surface::GetCiphers => super::discovery::ciphers(c, undefined, &[]),
        Surface::ConvertAlpnProtocols => {
            convert_alpn_protocols(c, at(0), at(1))?;
            Ok(undefined)
        }
        Surface::GetCaCertificates => ca_certificates(c, at(0)),
        Surface::SetDefaultCaCertificates => {
            let operation = c.string_rooted("setDefault");
            super::discovery::ca_certificates(c, undefined, &[operation, at(0)])?;
            let cache = get_api(c, "caCertificates")?;
            let key = c.string_rooted(DEFAULT_STORE);
            c.delete_property_rooted(cache, key)?;
            Ok(undefined)
        }
        Surface::CheckServerIdentity => super::identity::check_server_identity(c, undefined, &[at(0), at(1)]),
        Surface::GetProtocol => info(c, this, "protocol", &[]),
        Surface::GetPeerCertificate => info(c, this, "peerCertificate", &[at(0)]),
        Surface::GetCertificate => info(c, this, "certificate", &[]),
        Surface::GetCipher => info(c, this, "cipher", &[]),
        Surface::GetSession => info(c, this, "session", &[]),
        Surface::GetTlsTicket => info(c, this, "ticket", &[]),
        Surface::IsSessionReused => info(c, this, "sessionReused", &[]),
        Surface::GetPeerX509Certificate => {
            let yes = c.boolean(true);
            let certificate = method(c, this, "getPeerCertificate", &[yes])?;
            x509(c, certificate)
        }
        Surface::GetX509Certificate => {
            let certificate = method(c, this, "getCertificate", &[])?;
            x509(c, certificate)
        }
        Surface::DestroySoon => {
            if truthy(c, this, "writable")? {
                method(c, this, "end", &[])?;
            }
            let finished = get(c, this, "writableFinished")?;
            if c.truthy_rooted(finished)? {
                method(c, this, "destroy", &[])?;
            } else {
                let event = c.string_rooted("finish");
                let destroy = get(c, this, "destroy")?;
                method(c, this, "once", &[event, destroy])?;
            }
            Ok(undefined)
        }
        Surface::Start => super::socket::start(c, undefined, &[this]),
        Surface::TlsError => super::socket::tls_error(c, undefined, &[this, at(0)]),
        Surface::ReleaseControl => super::socket::release_control(c, undefined, &[this]),
        Surface::ServerRejection => {
            let (error, event, socket) = (at(0), at(1), at(2));
            if c.string_text(event)?.as_deref() == Some("secureConnection") {
                method(c, socket, "destroy", &[error])?;
                return Ok(undefined);
            }
            let server = get_api(c, "serverConstructor")?;
            let prototype = get(c, server, "prototype")?;
            let parent = c.prototype_rooted(prototype)?;
            if !absent(c, parent) {
                let global = c.global_root()?;
                let symbol = get(c, global, "Symbol")?;
                let key = c.string_rooted("nodejs.rejection");
                let key = method(c, symbol, "for", &[key])?;
                let inherited = c.get_property_rooted(parent, key)?;
                if c.is_callable_rooted(inherited)? {
                    return c.call_rooted(inherited, this, &[error, event, socket]);
                }
            }
            let name = c.string_rooted("error");
            method(c, this, "emit", &[name, error])
        }
        Surface::SetSecureContext => super::server::set_context(c, this, &[at(0)]),
        Surface::SetTicketKeys | Surface::GetTicketKeys => {
            let credentials = get(c, this, "_sharedCreds")?;
            let inner = if absent(c, credentials) { undefined } else { get(c, credentials, "context")? };
            if absent(c, inner) {
                let error = c.error_rooted("TLS server context is unavailable")?;
                crate::modules::node_errors_shared_vm::decorate_error(c, error, "ERR_TLS_INVALID_STATE")?;
                return Err(c.throw(error));
            }
            if current::<Surface>(c)? == Surface::SetTicketKeys {
                method(c, inner, "setTicketKeys", &[at(0)])
            } else {
                method(c, inner, "getTicketKeys", &[])
            }
        }
        Surface::AddContext => {
            let (servername, secure) = (at(0), at(1));
            let Some(name) = c.string_text(servername)? else {
                let error = c.type_error_rooted("\"servername\" must be a string")?;
                return Err(c.throw(error));
            };
            let class = get_api(c, "SecureContext")?;
            let secure = if c.is_object_rooted(secure)? && c.instance_of_rooted(secure, class)? {
                secure
            } else {
                super::facade::create(c, undefined, &[secure])?
            };
            let name = c.string_rooted(&name.to_lowercase());
            let entry = c.array_rooted(&[name, secure])?;
            let contexts = get(c, this, "_contexts")?;
            method(c, contexts, "push", &[entry])?;
            Ok(undefined)
        }
        Surface::ConnectEnd => {
            if truthy(c, this, "_hadError")? || truthy(c, this, "_secureEstablished")? {
                return Ok(undefined);
            }
            let yes = c.boolean(true);
            set(c, this, "_hadError", yes)?;
            let error = c.error_rooted("Client network socket disconnected before secure TLS connection was established")?;
            let code = c.string_rooted("ECONNRESET");
            set(c, error, "code", code)?;
            connect_fields(c, this, error)?;
            method(c, this, "destroy", &[error])
        }
        Surface::ConnectError => {
            let error = at(0);
            let reset = !absent(c, error) && {
                let code = get(c, error, "code")?;
                c.string_text(code)?.as_deref() == Some("ECONNRESET")
            };
            if reset && !truthy(c, this, "_secureEstablished")? && !truthy(c, this, "_hadError")? {
                let yes = c.boolean(true);
                set(c, this, "_hadError", yes)?;
                connect_fields(c, this, error)?;
            }
            Ok(undefined)
        }
    }
}

fn truthy(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    c.truthy_rooted(value)
}

fn get_api(c: &mut Context<'_>, name: &str) -> Outcome {
    let api = super::facade::install(c)?;
    get(c, api, name)
}

fn info(c: &mut Context<'_>, socket: RootId, name: &str, rest: &[RootId]) -> Outcome {
    let name = c.string_rooted(name);
    let detailed = rest.first().copied().unwrap_or_else(|| c.undefined());
    let undefined = c.undefined();
    super::socket::info(c, undefined, &[socket, name, detailed])
}

/// The endpoint of a failed `tls.connect` on its error (`onConnectEnd`).
fn connect_fields(c: &mut Context<'_>, socket: RootId, error: RootId) -> Result<(), RootedError> {
    let options = get(c, socket, "_tlsOptions")?;
    for name in ["path", "host", "port", "localAddress"] {
        let value = get(c, options, name)?;
        set(c, error, name, value)?;
    }
    Ok(())
}

fn connect(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let socket = super::socket::connect(c, undefined, args)?;
    for (event, member, name) in [("end", Surface::ConnectEnd, "onConnectEnd"), ("error", Surface::ConnectError, "onConnectError")] {
        let listener = function(c, member, name, &[])?;
        let event = c.string_rooted(event);
        method(c, socket, "prependListener", &[event, listener])?;
    }
    Ok(socket)
}

/// `toX509Certificate(certificate)`: the legacy certificate object as an
/// `X509Certificate`, with its issuer chain.
fn x509(c: &mut Context<'_>, certificate: RootId) -> Outcome {
    if !c.truthy_rooted(certificate)? {
        return Ok(c.undefined());
    }
    let raw = get(c, certificate, "raw")?;
    if !c.truthy_rooted(raw)? {
        return Ok(c.undefined());
    }
    let crypto = crate::host::shared_vm::commonjs::crypto_module(c)?;
    let class = get(c, crypto, "X509Certificate")?;
    let result = c.construct_rooted(class, class, &[raw])?;
    let issuer = get(c, certificate, "issuerCertificate")?;
    if c.truthy_rooted(issuer)? && !c.strict_equal_rooted(issuer, certificate)? {
        let issuer = x509(c, issuer)?;
        set(c, result, "issuerCertificate", issuer)?;
    }
    Ok(result)
}

/// `tls.getCACertificates(type)`, cached per store.
fn ca_certificates(c: &mut Context<'_>, kind: RootId) -> Outcome {
    let kind = if c.rooted_value(kind).is_some_and(|value| value.is_undefined()) { c.string_rooted(DEFAULT_STORE) } else { kind };
    if c.string_text(kind)?.as_deref() == Some(BUNDLED_STORE) {
        let module = get_api(c, "module")?;
        return get(c, module, "rootCertificates");
    }
    let cache = get_api(c, "caCertificates")?;
    let cached = if let Some(name) = c.string_text(kind)? { Some((name.clone(), get(c, cache, &name)?)) } else { None };
    if let Some((_, value)) = cached.filter(|(_, value)| !absent(c, *value)) {
        return Ok(value);
    }
    let use_system = {
        let shared = c.host_mut().shared_state();
        let state = shared.borrow();
        let options = &state.cli_options;
        !options.enabled("--no-use-system-ca")
            && (options.enabled("--use-system-ca")
                || options.enabled("--use-openssl-ca")
                || std::env::var("NODE_USE_SYSTEM_CA").is_ok_and(|value| value == "1"))
    };
    let use_system = c.boolean(use_system);
    let undefined = c.undefined();
    let certificates = super::discovery::ca_certificates(c, undefined, &[kind, use_system])?;
    if let Some(name) = c.string_text(kind)? {
        set(c, cache, &name, certificates)?;
    }
    Ok(certificates)
}

fn alpn_value_error(c: &mut Context<'_>, value: RootId, reason: &str) -> RootedError {
    let shown = match crate::modules::util_shared_vm::internal_inspect(c)
        .and_then(|inspect| {
            let undefined = c.undefined();
            c.call_rooted(inspect, undefined, &[value])
        })
        .and_then(|shown| c.to_string(shown))
    {
        Ok(shown) => shown,
        Err(error) => return error,
    };
    match c.type_error_rooted(&format!("The argument 'ALPNProtocols' {reason}. Received {shown}")) {
        Ok(error) => match crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_ARG_VALUE") {
            Ok(()) => c.throw(error),
            Err(failure) => failure,
        },
        Err(failure) => failure,
    }
}

/// `convertALPNProtocols(protocols, out)`: the wire form, as `out.ALPNProtocols`.
fn convert_alpn_protocols(c: &mut Context<'_>, protocols: RootId, out: RootId) -> Result<(), RootedError> {
    if c.is_array_rooted(protocols)? {
        let mut wire = Vec::new();
        for (index, protocol) in c.array_like_values_rooted(protocols)?.into_iter().enumerate() {
            let text = c.to_string(protocol)?;
            let length = text.len();
            if length == 0 {
                return Err(alpn_value_error(c, protocol, &format!("at index {index} must be a non-empty string")));
            }
            if length > MAX_ALPN_PROTOCOL {
                let error = c.range_error_rooted(&format!(
                    "The byte length of the protocol at index {index} exceeds the maximum length. It must be <= {MAX_ALPN_PROTOCOL}. Received {length}"
                ))?;
                crate::modules::node_errors_shared_vm::decorate_range_error(c, error, "ERR_OUT_OF_RANGE")?;
                return Err(c.throw(error));
            }
            wire.push(length as u8);
            wire.extend_from_slice(text.as_bytes());
        }
        let buffer = crate::modules::buffer_shared_vm::from_vec(c, wire)?;
        return set(c, out, "ALPNProtocols", buffer);
    }
    if let Some(bytes) = c.view_bytes_rooted(protocols) {
        let buffer = crate::modules::buffer_shared_vm::from_vec(c, bytes.clone())?;
        let mut offset = 0;
        while offset < bytes.len() {
            let length = usize::from(bytes[offset]);
            if length == 0 {
                return Err(alpn_value_error(c, buffer, "must not contain zero-length protocol"));
            }
            if offset + 1 + length > bytes.len() {
                return Err(alpn_value_error(c, buffer, "contains truncated protocol"));
            }
            offset += 1 + length;
        }
        return set(c, out, "ALPNProtocols", buffer);
    }
    Ok(())
}
