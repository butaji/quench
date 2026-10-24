//! Configuration values are read once; public native-method hooks retain their effects.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
pub(crate) const METHODS: &[&str] = &[
    "setCiphers",
    "setCipherSuites",
    "setCert",
    "setKey",
    "addCACert",
    "addRootCerts",
    "setAllowPartialTrustChain",
    "addCRL",
    "loadPKCS12",
    "setSigalgs",
    "setECDHCurve",
    "setDHParam",
    "setSessionIdContext",
    "setSessionTimeout",
    "setTicketKeys",
    "getTicketKeys",
    "setClientCertEngine",
    "setEngineKey",
    "setCertificateCompression",
    "setMinProto",
    "setMaxProto",
    "getCertificate",
];

// Immutable trust material is Node product data, not test or host path policy.
fn bundled_roots() -> Result<&'static [openssl::x509::X509], openssl::error::ErrorStack> {
    static ROOTS: std::sync::OnceLock<
        Result<Vec<openssl::x509::X509>, openssl::error::ErrorStack>,
    > = std::sync::OnceLock::new();
    match ROOTS
        .get_or_init(|| openssl::x509::X509::stack_from_pem(include_bytes!("bundled-roots.pem")))
    {
        Ok(roots) => Ok(roots),
        Err(error) => Err(error.clone()),
    }
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn absent(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}
pub(crate) fn configure(
    context: &mut NativeContext<'_, NodeHost>,
    outer: RootId,
    options: RootId,
) -> Result<(), RootedError> {
    let api = super::facade::install(context)?;
    let policy = get(context, api, "standardConfigure")?;
    let inner = get(context, outer, "context")?;
    let undefined = context.undefined();
    context.call_rooted(policy, undefined, &[inner, options])?;
    Ok(())
}

pub(crate) fn method(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use foreign_types::{ForeignType, ForeignTypeRef};
    let action = context.host_function_data()?;
    let action = context.to_string(action)?;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let index = super::contexts::owner(context, receiver)?;
    let shared = context.host_mut().shared_state();
    match action.as_str() {
        "setClientCertEngine" | "setEngineKey" => {
            let name = context
                .string_text(value)?
                .ok_or_else(|| RootedError::host("Engine binding requires a string"))?;
            let engine = args.get(1).copied().unwrap_or_else(|| context.undefined());
            let engine = if action == "setEngineKey" {
                Some(
                    context
                        .string_text(engine)?
                        .ok_or_else(|| RootedError::host("Engine binding requires a string"))?,
                )
            } else {
                None
            };
            let result = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                if let Some(engine) = engine {
                    super::super::crypto_engine_shared_vm::set_key(configuration, &name, &engine)
                } else {
                    super::super::crypto_engine_shared_vm::set_client(configuration, &name)
                }
            };
            result.map_err(|failure| {
                super::super::crypto_engine_shared_vm::error(context, failure)
            })?;
        }
        "setCiphers" | "setCipherSuites" | "setSigalgs" | "setECDHCurve" => {
            let value = context
                .string_text(value)?
                .ok_or_else(|| RootedError::host("TLS native configuration requires a string"))?;
            let value = super::super::crypto_strings::null_terminated_prefix(&value);
            let result = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                match action.as_str() {
                    "setCiphers" => configuration.set_cipher_list(value),
                    "setCipherSuites" => configuration.set_ciphersuites(value),
                    "setSigalgs" => configuration.set_sigalgs_list(value),
                    "setECDHCurve" if value == "auto" => Ok(()),
                    _ => configuration.set_groups_list(value),
                }
            };
            if let Err(error) = result {
                // OpenSSL sslerr.h: deliberately clearing TLS 1.2 ciphers is
                // the sole ignored setter error in Node's native binding.
                const SSL_R_NO_CIPHER_MATCH: i32 = 185;
                let cleared = action == "setCiphers"
                    && value.is_empty()
                    && error
                        .errors()
                        .first()
                        .is_some_and(|error| error.reason_code() == SSL_R_NO_CIPHER_MATCH);
                if !cleared {
                    return Err(if action == "setECDHCurve" {
                        super::operation_error(context, "Failed to set ECDH curve")
                    } else {
                        super::super::crypto_error_shared_vm::decorated(context, error)
                    });
                }
            }
        }
        "setMinProto" | "setMaxProto" => {
            let version = context
                .rooted_value(value)
                .and_then(|value| value.as_number())
                .ok_or_else(|| RootedError::host("TLS version requires an integer"))?
                as i32;
            let success = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                unsafe {
                    if action == "setMinProto" {
                        openssl_sys::SSL_CTX_set_min_proto_version(configuration.as_ptr(), version)
                    } else {
                        openssl_sys::SSL_CTX_set_max_proto_version(configuration.as_ptr(), version)
                    }
                }
            };
            if success != 1 {
                return Err(super::parser_error(
                    context,
                    openssl::error::ErrorStack::get(),
                ));
            }
        }
        "getCertificate" => {
            let bytes = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                let cert = unsafe { openssl_sys::SSL_CTX_get0_certificate(configuration.as_ptr()) };
                if cert.is_null() {
                    None
                } else {
                    // SSL_CTX retains this X509 while the configuration borrow is held.
                    Some(unsafe { openssl::x509::X509Ref::from_ptr(cert) }.to_der())
                }
            };
            let Some(bytes) = bytes else {
                return Ok(context.null());
            };
            let bytes = bytes
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
            return super::super::buffer_shared_vm::from_bytes(context, &bytes);
        }
        "setSessionIdContext" => {
            let bytes = context
                .string_text(value)?
                .ok_or_else(|| RootedError::host("TLS session identifier requires a string"))?
                .into_bytes();
            let result = shared
                .borrow_mut()
                .tls_contexts
                .configuration(index)?
                .set_session_id_context(&bytes);
            result
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
        "setSessionTimeout" => {
            let timeout = context
                .rooted_value(value)
                .and_then(|value| value.as_number())
                .ok_or_else(|| RootedError::host("TLS timeout requires an integer"))?
                as std::ffi::c_long;
            let mut state = shared.borrow_mut();
            let configuration = state.tls_contexts.configuration(index)?;
            unsafe {
                SSL_CTX_set_timeout(configuration.as_ptr(), timeout);
            }
        }
        "setTicketKeys" => {
            let Some(bytes) = context.view_bytes_rooted(value).map(|bytes| bytes.to_vec()) else {
                return Err(super::super::util_shared_vm::argument_type_error(
                    context, value, "keys", "Buffer",
                )?);
            };
            if bytes.len() != 48 {
                // Node validates this with an assertion after accepting any
                // ArrayBuffer view (Buffer, TypedArray, or DataView).
                let error = context.error_rooted("Session ticket keys must be a 48-byte buffer")?;
                return Err(context.throw(error));
            }
            // Node exposes ticket keys as 16-byte name + 16-byte HMAC key +
            // 16-byte AES key. OpenSSL 3.5 expanded both internal keys to 32
            // bytes, so its SSL_CTX control expects 80 bytes. Zero-extend the
            // Node key material while preserving Node's 48-byte public value.
            const SSL_CTRL_SET_TLSEXT_TICKET_KEYS: i32 = 59;
            const SSL_CTRL_GET_TLSEXT_TICKET_KEYS: i32 = 58;
            let key_len = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                unsafe {
                    openssl_sys::SSL_CTX_ctrl(
                        configuration.as_ptr(),
                        SSL_CTRL_GET_TLSEXT_TICKET_KEYS,
                        0,
                        std::ptr::null_mut(),
                    ) as usize
                }
            };
            if !matches!(key_len, 48 | 80) {
                return Err(RootedError::host(
                    "unsupported OpenSSL ticket key layout",
                ));
            }
            let mut openssl_keys = vec![0u8; key_len];
            if key_len == 48 {
                openssl_keys.copy_from_slice(&bytes);
            } else {
                openssl_keys[..16].copy_from_slice(&bytes[..16]);
                openssl_keys[16..32].copy_from_slice(&bytes[16..32]);
                openssl_keys[48..64].copy_from_slice(&bytes[32..48]);
            }
            let result = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                unsafe {
                    openssl_sys::SSL_CTX_ctrl(
                        configuration.as_ptr(),
                        SSL_CTRL_SET_TLSEXT_TICKET_KEYS,
                        openssl_keys.len() as _,
                        openssl_keys.as_mut_ptr().cast(),
                    )
                }
            };
            if result != 1 {
                return Err(super::parser_error(
                    context,
                    openssl::error::ErrorStack::get(),
                ));
            }
        }
        "getTicketKeys" => {
            // OpenSSL 3.5 returns an 80-byte layout (16-byte name, 32-byte
            // HMAC key, 32-byte AES key), while Node's API is 48 bytes. Drop
            // the zero-extended tails when projecting its public key format.
            const SSL_CTRL_GET_TLSEXT_TICKET_KEYS: i32 = 58;
            let key_len = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                unsafe {
                    openssl_sys::SSL_CTX_ctrl(
                        configuration.as_ptr(),
                        SSL_CTRL_GET_TLSEXT_TICKET_KEYS,
                        0,
                        std::ptr::null_mut(),
                    ) as usize
                }
            };
            if !matches!(key_len, 48 | 80) {
                return Err(RootedError::host(
                    "unsupported OpenSSL ticket key layout",
                ));
            }
            let mut openssl_keys = vec![0u8; key_len];
            let result = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                unsafe {
                    openssl_sys::SSL_CTX_ctrl(
                        configuration.as_ptr(),
                        SSL_CTRL_GET_TLSEXT_TICKET_KEYS,
                        openssl_keys.len() as _,
                        openssl_keys.as_mut_ptr().cast(),
                    )
                }
            };
            if result != 1 {
                return Err(super::parser_error(
                    context,
                    openssl::error::ErrorStack::get(),
                ));
            }
            let public_keys: Vec<u8> = if key_len == 48 {
                openssl_keys
            } else {
                openssl_keys[..16]
                    .iter()
                    .chain(&openssl_keys[16..32])
                    .chain(&openssl_keys[48..64])
                    .copied()
                    .collect()
            };
            let values: Vec<_> = public_keys
                .iter()
                .map(|byte| context.number(*byte as f64))
                .collect();
            let array = context.array_rooted(&values)?;
            let global = context.global_root()?;
            let buffer = get(context, global, "Buffer")?;
            let from = get(context, buffer, "from")?;
            return context.call_rooted(from, buffer, &[array]);
        }
        "setDHParam" => {
            if context
                .rooted_value(value)
                .and_then(|value| value.as_bool())
                == Some(true)
            {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                let success =
                    unsafe { openssl_sys::SSL_CTX_set_dh_auto(configuration.as_ptr(), 1) };
                if success != 1 {
                    return Err(RootedError::host("automatic DH configuration failed"));
                }
            } else {
                let bytes = super::material_bytes(context, value, "options.dhparam")?;
                // Node deliberately discards malformed/non-DH parameter PEMs.
                let Ok(dh) = openssl::dh::Dh::params_from_pem(&bytes) else {
                    return Ok(context.undefined());
                };
                let bits = dh.prime_p().num_bits();
                const MINIMUM_DH_BITS: i32 = 1024;
                const RECOMMENDED_DH_BITS: i32 = 2048;
                if bits < MINIMUM_DH_BITS {
                    let error = context.type_error_rooted("DH parameter is less than 1024 bits")?;
                    super::super::node_errors_shared_vm::decorate_type_error(
                        context,
                        error,
                        "ERR_INVALID_ARG_VALUE",
                    )?;
                    return Err(context.throw(error));
                }
                let result = shared
                    .borrow_mut()
                    .tls_contexts
                    .configuration(index)?
                    .set_tmp_dh(&dh);
                result.map_err(|_| {
                    super::operation_error(context, "Error setting temp DH parameter")
                })?;
                if bits < RECOMMENDED_DH_BITS {
                    return Ok(context.string_rooted("DH parameter is less than 2048 bits"));
                }
            }
        }
        "setCert" => {
            let bytes = super::material_bytes(context, value, "options.cert")?;
            let chain = openssl::x509::X509::stack_from_pem(&bytes)
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
            let mut chain = chain.into_iter();
            let cert = chain
                .next()
                .ok_or_else(|| super::operation_error(context, "Failed to parse certificate"))?;
            let result = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                (|| {
                    configuration.set_certificate(&cert)?;
                    for cert in chain {
                        configuration.add_extra_chain_cert(cert)?;
                    }
                    Ok::<_, openssl::error::ErrorStack>(())
                })()
            };
            result
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
        "setKey" => {
            let bytes = super::material_bytes(context, value, "options.key")?;
            let pass = args.get(1).copied().unwrap_or_else(|| context.undefined());
            let key = if absent(context, pass) {
                openssl::pkey::PKey::private_key_from_pem(&bytes)
            } else {
                let password = context
                    .string_text(pass)?
                    .ok_or_else(|| RootedError::host("TLS key passphrase requires a string"))?;
                openssl::pkey::PKey::private_key_from_pem_passphrase(&bytes, password.as_bytes())
            }
            .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
            let result = shared
                .borrow_mut()
                .tls_contexts
                .configuration(index)?
                .set_private_key(&key);
            result
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
        "addCACert" => {
            let bytes = super::material_bytes(context, value, "options.ca")?;
            let certs = openssl::x509::X509::stack_from_pem(&bytes)
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
            for cert in certs {
                let result = shared
                    .borrow_mut()
                    .tls_contexts
                    .configuration(index)?
                    .cert_store_mut()
                    .add_cert(cert);
                result.map_err(|error| {
                    super::super::crypto_error_shared_vm::decorated(context, error)
                })?;
            }
        }
        "addRootCerts" => {
            let defaults = shared.borrow().tls_contexts.default_ca_certificates.clone();
            let roots: Vec<openssl::x509::X509> = if let Some(defaults) = defaults {
                if defaults.is_empty() {
                    Vec::new()
                } else {
                    let pem = defaults.concat();
                    openssl::x509::X509::stack_from_pem(pem.as_bytes()).map_err(|error| {
                        super::super::crypto_error_shared_vm::decorated(context, error)
                    })?
                }
            } else {
                bundled_roots()
                    .map_err(|error| {
                        super::super::crypto_error_shared_vm::decorated(context, error)
                    })?
                    .iter()
                    .map(|root| root.to_owned())
                    .collect()
            };
            for root in roots {
                let result = shared
                    .borrow_mut()
                    .tls_contexts
                    .configuration(index)?
                    .cert_store_mut()
                    .add_cert(root);
                result.map_err(|error| {
                    super::super::crypto_error_shared_vm::decorated(context, error)
                })?;
            }
        }
        "setAllowPartialTrustChain" => {
            let result = shared
                .borrow_mut()
                .tls_contexts
                .configuration(index)?
                .cert_store_mut()
                .set_flags(openssl::x509::verify::X509VerifyFlags::PARTIAL_CHAIN);
            result
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
        "addCRL" => {
            let bytes = super::material_bytes(context, value, "options.crl")?;
            let crl = openssl::x509::X509Crl::from_pem(&bytes)
                .map_err(|_| super::operation_error(context, "Failed to parse CRL"))?;
            let result = {
                let mut state = shared.borrow_mut();
                let store = state.tls_contexts.configuration(index)?.cert_store_mut();
                let success = unsafe { X509_STORE_add_crl(store.as_ptr(), crl.as_ptr()) };
                if success == 1 {
                    store.set_flags(
                        openssl::x509::verify::X509VerifyFlags::CRL_CHECK
                            | openssl::x509::verify::X509VerifyFlags::CRL_CHECK_ALL,
                    )
                } else {
                    Err(openssl::error::ErrorStack::get())
                }
            };
            result
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
        "loadPKCS12" => {
            let pass = args.get(1).copied().unwrap_or_else(|| context.undefined());
            let parsed = super::parsed_pfx(context, value, pass)?;
            let result = {
                let mut state = shared.borrow_mut();
                let configuration = state.tls_contexts.configuration(index)?;
                (|| {
                    configuration
                        .set_certificate(parsed.cert.as_ref().expect("validated certificate"))?;
                    configuration.set_private_key(parsed.pkey.as_ref().expect("validated key"))?;
                    if let Some(ca) = parsed.ca {
                        for cert in ca {
                            configuration.add_extra_chain_cert(cert.to_owned())?;
                            configuration.add_client_ca(&cert)?;
                            // Node ignores duplicate-store insertion errors.
                            let _ = configuration.cert_store_mut().add_cert(cert);
                        }
                    }
                    Ok::<_, openssl::error::ErrorStack>(())
                })()
            };
            result
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
        }
        _ => {
            return Err(RootedError::host(format!(
                "TLS configuration operation {action} is not implemented"
            )));
        }
    }
    Ok(context.undefined())
}

unsafe extern "C" {
    fn X509_STORE_add_crl(
        store: *mut openssl_sys::X509_STORE,
        crl: *mut openssl_sys::X509_CRL,
    ) -> std::ffi::c_int;
    fn SSL_CTX_set_timeout(
        context: *mut openssl_sys::SSL_CTX,
        timeout: std::ffi::c_long,
    ) -> std::ffi::c_long;
}
