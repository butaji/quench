//! SSL_CTX ownership is weakly tied to its VM object; SSL streams retain OpenSSL refs.
use crate::host::NodeHost;
use foreign_types::ForeignTypeRef;
use openssl::ssl::{NameType, SniError, Ssl, SslContextBuilder};
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
use std::sync::OnceLock;

type ServerNames = Vec<(String, usize)>;
type AlpnProtocols = Vec<u8>;

pub(crate) fn alpn_protocols_index(
) -> Result<&'static openssl::ex_data::Index<Ssl, AlpnProtocols>, openssl::error::ErrorStack> {
    static INDEX: OnceLock<
        Result<openssl::ex_data::Index<Ssl, AlpnProtocols>, openssl::error::ErrorStack>,
    > = OnceLock::new();
    INDEX
        .get_or_init(Ssl::new_ex_index::<AlpnProtocols>)
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn server_names_index(
) -> Result<&'static openssl::ex_data::Index<Ssl, ServerNames>, openssl::error::ErrorStack> {
    static INDEX: OnceLock<
        Result<openssl::ex_data::Index<Ssl, ServerNames>, openssl::error::ErrorStack>,
    > = OnceLock::new();
    INDEX
        .get_or_init(Ssl::new_ex_index::<ServerNames>)
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn peer_servername_index(
) -> Result<&'static openssl::ex_data::Index<Ssl, String>, openssl::error::ErrorStack> {
    static INDEX: OnceLock<
        Result<openssl::ex_data::Index<Ssl, String>, openssl::error::ErrorStack>,
    > = OnceLock::new();
    INDEX
        .get_or_init(Ssl::new_ex_index::<String>)
        .as_ref()
        .map_err(Clone::clone)
}

fn hostname_matches(pattern: &str, hostname: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix("*.") {
        hostname
            .to_ascii_lowercase()
            .strip_suffix(suffix)
            .and_then(|prefix| prefix.strip_suffix('.'))
            .is_some_and(|label| !label.is_empty() && !label.contains('.'))
    } else {
        pattern.eq_ignore_ascii_case(hostname)
    }
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) api: Option<RootId>,
    pub(crate) ciphers: Option<Vec<String>>,
    pub(crate) default_ca_certificates: Option<Vec<String>>,
    pub(crate) transport_errors: std::collections::VecDeque<(u64, openssl::error::ErrorStack)>,
    pub(crate) sessions: super::sessions::State,
    entries: Vec<Entry>,
}
struct Entry {
    identity: HostWeakRoot,
    phase: Phase,
}
enum Phase {
    Allocated,
    Initialized(SslContextBuilder),
}
impl State {
    pub(crate) fn register(&mut self, identity: HostWeakRoot) {
        self.entries.push(Entry {
            identity,
            phase: Phase::Allocated,
        });
    }
    pub(crate) fn initialize(&mut self, index: usize, configuration: SslContextBuilder) {
        self.entries[index].phase = Phase::Initialized(configuration);
    }
    pub(crate) fn configuration(
        &mut self,
        index: usize,
    ) -> Result<&mut SslContextBuilder, RootedError> {
        match &mut self.entries[index].phase {
            Phase::Initialized(configuration) => Ok(configuration),
            Phase::Allocated => Err(RootedError::host("TLS context is not initialized")),
        }
    }
}
pub(crate) fn owner(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
) -> Result<usize, RootedError> {
    let shared = context.host_mut().shared_state();
    let identities = shared
        .borrow()
        .tls_contexts
        .entries
        .iter()
        .map(|entry| entry.identity)
        .collect::<Vec<_>>();
    for (index, identity) in identities.into_iter().enumerate() {
        if let Some(object) = context.resolve_host_weak_root(identity) {
            if context.strict_equal_rooted(object, receiver)? {
                return Ok(index);
            }
        }
    }
    let error = context.type_error_rooted("Illegal invocation")?;
    Err(context.throw(error))
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    super::sessions::prune(runtime);
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .tls_contexts
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (!runtime.host_weak_root_is_live(entry.identity)).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let entry = shared.borrow_mut().tls_contexts.entries.remove(index);
        runtime.release_host_weak_root(entry.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    super::sessions::cleanup(runtime);
    let shared = runtime.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().tls_contexts.entries);
    for entry in entries {
        runtime.release_host_weak_root(entry.identity);
    }
    shared.borrow_mut().tls_contexts.transport_errors.clear();
    let api = shared.borrow_mut().tls_contexts.api.take();
    if let Some(api) = api {
        runtime.release_root(api);
    }
}

pub(crate) fn register(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let object = args.first().copied().unwrap_or_else(|| context.undefined());
    let identity = context.host_weak_root(object)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .tls_contexts
        .register(identity);
    Ok(context.undefined())
}
fn raw_version(context: &NativeContext<'_, NodeHost>, value: RootId) -> Result<i32, RootedError> {
    context
        .rooted_value(value)
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && value.fract() == 0.0)
        .map(|value| value as i32)
        .ok_or_else(|| RootedError::host("TLS protocol binding requires an integer"))
}
pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use openssl::ssl::{SslMethod, SslOptions, SslSessionCacheMode};
    let index = owner(context, receiver)?;
    let protocol = args.first().copied().unwrap_or_else(|| context.undefined());
    let min = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let max = args.get(2).copied().unwrap_or_else(|| context.undefined());
    let mut min = raw_version(context, min)?;
    let mut max = raw_version(context, max)?;
    if max == 0 {
        max = openssl_sys::TLS1_3_VERSION;
    }
    let name = context.string_text(protocol)?;
    let mut method = SslMethod::tls();
    if let Some(name) = name {
        let name = super::super::crypto_strings::null_terminated_prefix(&name);
        let (family, role) = if let Some(family) = name.strip_suffix("_server_method") {
            (family, Some(false))
        } else if let Some(family) = name.strip_suffix("_client_method") {
            (family, Some(true))
        } else if let Some(family) = name.strip_suffix("_method") {
            (family, None)
        } else {
            let error = context.type_error_rooted(&format!("Unknown method: {name}"))?;
            super::super::node_errors_shared_vm::decorate_type_error(
                context,
                error,
                "ERR_TLS_INVALID_PROTOCOL_METHOD",
            )?;
            return Err(context.throw(error));
        };
        method = match role {
            Some(true) => SslMethod::tls_client(),
            Some(false) => SslMethod::tls_server(),
            None => SslMethod::tls(),
        };
        let version = match family {
            "SSLv23" => {
                max = openssl_sys::TLS1_2_VERSION;
                None
            }
            "TLS" => {
                min = 0;
                max = openssl_sys::TLS1_3_VERSION;
                None
            }
            "TLSv1" => Some(openssl_sys::TLS1_VERSION),
            "TLSv1_1" => Some(openssl_sys::TLS1_1_VERSION),
            "TLSv1_2" => Some(openssl_sys::TLS1_2_VERSION),
            "SSLv2" | "SSLv3" => {
                let error = context.type_error_rooted(&format!("{family} methods disabled"))?;
                super::super::node_errors_shared_vm::decorate_type_error(
                    context,
                    error,
                    "ERR_TLS_INVALID_PROTOCOL_METHOD",
                )?;
                return Err(context.throw(error));
            }
            _ => {
                let error = context.type_error_rooted(&format!("Unknown method: {name}"))?;
                super::super::node_errors_shared_vm::decorate_type_error(
                    context,
                    error,
                    "ERR_TLS_INVALID_PROTOCOL_METHOD",
                )?;
                return Err(context.throw(error));
            }
        };
        if let Some(version) = version {
            min = version;
            max = version;
        }
    }
    let mut configuration =
        SslContextBuilder::new(method).map_err(|error| super::parser_error(context, error))?;
    let server_names =
        *server_names_index().map_err(|error| super::parser_error(context, error))?;
    let peer_servername =
        *peer_servername_index().map_err(|error| super::parser_error(context, error))?;
    let alpn_protocols =
        *alpn_protocols_index().map_err(|error| super::parser_error(context, error))?;
    configuration.set_alpn_select_callback(move |ssl, client| {
        let Some(server) = ssl.ex_data(alpn_protocols) else {
            return Err(openssl::ssl::AlpnError::NOACK);
        };
        select_alpn_protocol(server, client)
    });
    configuration.set_servername_callback(move |ssl, _alert| {
        let Some(hostname) = ssl.servername(NameType::HOST_NAME).map(str::to_owned) else {
            return Ok(());
        };
        if let Some(contexts) = ssl.ex_data(server_names) {
            for (pattern, pointer) in contexts.iter().rev() {
                if hostname_matches(pattern, &hostname) {
                    // Server._contexts strongly retains each SecureContext;
                    // the registry-owned builder therefore keeps this SSL_CTX
                    // alive for the complete SNI callback and SSL_set_SSL_CTX
                    // takes its own native reference.
                    let target = unsafe {
                        openssl::ssl::SslContextRef::from_ptr(*pointer as *mut openssl_sys::SSL_CTX)
                    };
                    ssl.set_ssl_context(target)
                        .map_err(|_| SniError::ALERT_FATAL)?;
                    // SSL_set_SSL_CTX switches certificate settings but an
                    // already-created SSL retains its verification store.
                    // Node's per-SNI SecureContext also supplies the trust
                    // store for a requested client certificate.
                    let store = unsafe { openssl_sys::SSL_CTX_get_cert_store(target.as_ptr()) };
                    if !store.is_null() {
                        let retained = unsafe { X509_STORE_up_ref(store) };
                        if retained != 1
                            || unsafe {
                                openssl_sys::SSL_set0_verify_cert_store(ssl.as_ptr(), store) != 1
                            }
                        {
                            return Err(SniError::ALERT_FATAL);
                        }
                    }
                    break;
                }
            }
        }
        ssl.set_ex_data(peer_servername, hostname);
        Ok(())
    });
    configuration.set_options(SslOptions::NO_SSLV2 | SslOptions::NO_SSLV3);
    configuration.set_session_cache_mode(
        SslSessionCacheMode::CLIENT
            | SslSessionCacheMode::SERVER
            | SslSessionCacheMode::NO_INTERNAL
            | SslSessionCacheMode::NO_AUTO_CLEAR,
    );
    // Protocol numbers and the SSL_CTX controls are owned by OpenSSL's layout.
    let success = unsafe {
        openssl_sys::SSL_CTX_set_min_proto_version(configuration.as_ptr(), min) == 1
            && openssl_sys::SSL_CTX_set_max_proto_version(configuration.as_ptr(), max) == 1
    };
    if !success {
        return Err(super::parser_error(
            context,
            openssl::error::ErrorStack::get(),
        ));
    }
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .tls_contexts
        .initialize(index, configuration);
    Ok(context.undefined())
}

fn select_alpn_protocol<'a>(
    server: &[u8],
    client: &'a [u8],
) -> Result<&'a [u8], openssl::ssl::AlpnError> {
    if server.is_empty() {
        return Err(openssl::ssl::AlpnError::NOACK);
    }

    let mut server_offset = 0;
    while server_offset < server.len() {
        let server_len = usize::from(server[server_offset]);
        let server_end = server_offset + 1 + server_len;
        if server_end > server.len() {
            return Err(openssl::ssl::AlpnError::ALERT_FATAL);
        }
        let server_protocol = &server[server_offset + 1..server_end];

        let mut client_offset = 0;
        while client_offset < client.len() {
            let client_len = usize::from(client[client_offset]);
            let client_end = client_offset + 1 + client_len;
            if client_end > client.len() {
                return Err(openssl::ssl::AlpnError::ALERT_FATAL);
            }
            if server_protocol == &client[client_offset + 1..client_end] {
                return Ok(&client[client_offset + 1..client_end]);
            }
            client_offset = client_end;
        }
        server_offset = server_end;
    }

    // ALPN is optional only when the peer sent no ALPN extension. If it did
    // offer protocols but none are supported, reject the handshake with the
    // no-application-protocol alert; otherwise incompatible clients remain
    // connected and can hang waiting for an error that TLS never sends.
    if client.is_empty() {
        Err(openssl::ssl::AlpnError::NOACK)
    } else {
        Err(openssl::ssl::AlpnError::ALERT_FATAL)
    }
}

#[cfg(test)]
mod tests {
    use super::select_alpn_protocol;
    use openssl::ssl::AlpnError;

    #[test]
    fn empty_server_alpn_list_does_not_acknowledge_alpn() {
        assert_eq!(select_alpn_protocol(&[], b"\x01a"), Err(AlpnError::NOACK));
    }

    #[test]
    fn static_server_alpn_prefers_its_first_supported_protocol() {
        assert_eq!(
            select_alpn_protocol(b"\x01a\x01b", b"\x01b\x01a"),
            Ok(&b"a"[..])
        );
    }

    #[test]
    fn configured_server_alpn_rejects_when_protocols_do_not_overlap() {
        assert_eq!(
            select_alpn_protocol(b"\x01a", b"\x01b"),
            Err(AlpnError::ALERT_FATAL)
        );
    }
}

unsafe extern "C" {
    fn X509_STORE_up_ref(store: *mut openssl_sys::X509_STORE) -> std::ffi::c_int;
}
pub(crate) fn set_options(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let index = owner(context, receiver)?;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(value) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        return Err(RootedError::host("TLS option binding requires a number"));
    };
    let shared = context.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    let configuration = state.tls_contexts.configuration(index)?;
    unsafe {
        openssl_sys::SSL_CTX_set_options(configuration.as_ptr(), (value as i64) as _);
    }
    Ok(context.undefined())
}
pub(crate) fn minimum(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    protocol_version(context, receiver, true)
}
pub(crate) fn maximum(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    protocol_version(context, receiver, false)
}
fn protocol_version(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    minimum: bool,
) -> Result<RootId, RootedError> {
    let index = owner(context, receiver)?;
    let shared = context.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    let configuration = state.tls_contexts.configuration(index)?;
    let version = unsafe {
        if minimum {
            openssl_sys::SSL_CTX_get_min_proto_version(configuration.as_ptr())
        } else {
            openssl_sys::SSL_CTX_get_max_proto_version(configuration.as_ptr())
        }
    };
    Ok(context.number(f64::from(version)))
}
