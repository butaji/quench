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
fn call(
    context: &mut NativeContext<'_, NodeHost>,
    inner: RootId,
    name: &str,
    args: &[RootId],
) -> Result<(), RootedError> {
    let method = get(context, inner, name)?;
    context.call_rooted(method, inner, args)?;
    Ok(())
}
fn text(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<String, RootedError> {
    if let Some(text) = context.string_text(value)? {
        Ok(text)
    } else {
        Err(super::super::util_shared_vm::argument_type_error(
            context, value, name, "string",
        )?)
    }
}
fn each(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    mut visit: impl FnMut(&mut NativeContext<'_, NodeHost>, RootId) -> Result<(), RootedError>,
) -> Result<(), RootedError> {
    if !context.is_array_rooted(value)? {
        return visit(context, value);
    }
    let length = get(context, value, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0) as usize;
    for index in 0..length {
        let key = context.string_rooted(&index.to_string());
        if context.has_property_rooted(value, key)? {
            let item = context.get_property_rooted(value, key)?;
            visit(context, item)?;
        }
    }
    Ok(())
}
fn each_key(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    mut visit: impl FnMut(&mut NativeContext<'_, NodeHost>, RootId) -> Result<(), RootedError>,
) -> Result<(), RootedError> {
    if !context.is_array_rooted(value)? {
        return visit(context, value);
    }
    let mut index = 0;
    loop {
        let length = get(context, value, "length")?;
        let length = context
            .rooted_value(length)
            .and_then(|value| value.as_number())
            .ok_or_else(|| RootedError::host("key array length is not numeric"))?
            as usize;
        if index >= length {
            break;
        }
        let item = get(context, value, &index.to_string())?;
        visit(context, item)?;
        index += 1;
    }
    Ok(())
}

pub(crate) fn configure(
    context: &mut NativeContext<'_, NodeHost>,
    outer: RootId,
    options: RootId,
) -> Result<(), RootedError> {
    if !context.is_object_rooted(options)? || context.is_array_rooted(options)? {
        return Err(super::super::util_shared_vm::argument_type_error(
            context, options, "options", "Object",
        )?);
    }
    let api = super::facade::install(context)?;
    let module = get(context, api, "module")?;
    let snapshot = context.null_object_rooted()?;
    for name in [
        "allowPartialTrustChain",
        "ca",
        "cert",
        "certificateCompression",
        "ciphers",
        "clientCertEngine",
        "crl",
        "dhparam",
        "ecdhCurve",
        "key",
        "passphrase",
        "pfx",
        "privateKeyIdentifier",
        "privateKeyEngine",
        "sessionIdContext",
        "sessionTimeout",
        "sigalgs",
        "ticketKeys",
    ] {
        let mut value = get(context, options, name)?;
        if context
            .rooted_value(value)
            .is_some_and(|value| value.is_undefined())
        {
            if name == "ciphers" {
                value = get(context, module, "DEFAULT_CIPHERS")?;
            }
            if name == "ecdhCurve" {
                value = get(context, module, "DEFAULT_ECDH_CURVE")?;
                if !context.truthy_rooted(value)? {
                    value = context.string_rooted("auto");
                }
            }
        }
        let key = context.string_rooted(name);
        context.define_data_property_rooted(snapshot, key, value, true, true, true)?;
    }
    let inner = get(context, outer, "context")?;
    let mut ciphers = get(context, snapshot, "ciphers")?;
    if !absent(context, ciphers) {
        text(context, ciphers, "options.ciphers")?;
    }
    if !context.truthy_rooted(ciphers)? {
        ciphers = get(context, module, "DEFAULT_CIPHERS")?;
    }
    let ciphers = text(context, ciphers, "options.ciphers")?;
    let (suites, legacy): (Vec<_>, Vec<_>) = ciphers
        .split(':')
        .filter(|value| !value.is_empty())
        .partition(|value| value.starts_with("TLS_") || value.starts_with("!TLS_"));
    if suites.is_empty() && legacy.is_empty() {
        let value = context.array_rooted(&[])?;
        return Err(super::super::util_shared_vm::argument_value_error(
            context,
            value,
            "options.ciphers",
            "is invalid",
        )?);
    }
    if !suites.is_empty() {
        let suites = context.string_rooted(&suites.join(":"));
        call(context, inner, "setCipherSuites", &[suites])?;
    }
    let only_modern = legacy.is_empty();
    let legacy = context.string_rooted(&legacy.join(":"));
    call(context, inner, "setCiphers", &[legacy])?;
    if only_modern {
        let getter = get(context, inner, "getMinProto")?;
        let minimum = context.call_rooted(getter, inner, &[])?;
        if context
            .rooted_value(minimum)
            .and_then(|value| value.as_number())
            .is_some_and(|value| value < f64::from(openssl_sys::TLS1_3_VERSION))
        {
            let getter = get(context, inner, "getMaxProto")?;
            let maximum = context.call_rooted(getter, inner, &[])?;
            if context
                .rooted_value(maximum)
                .and_then(|value| value.as_number())
                .is_some_and(|value| value > f64::from(openssl_sys::TLS1_2_VERSION))
            {
                let version = context.number(f64::from(openssl_sys::TLS1_3_VERSION));
                call(context, inner, "setMinProto", &[version])?;
            }
        }
    }
    let ca = get(context, snapshot, "ca")?;
    if context.truthy_rooted(ca)? {
        each(context, ca, |context, item| {
            super::material_bytes(context, item, "options.ca")?;
            call(context, inner, "addCACert", &[item])
        })?;
    } else {
        call(context, inner, "addRootCerts", &[])?;
    }
    let partial = get(context, snapshot, "allowPartialTrustChain")?;
    if context.truthy_rooted(partial)? {
        call(context, inner, "setAllowPartialTrustChain", &[])?;
    }
    let cert = get(context, snapshot, "cert")?;
    if context.truthy_rooted(cert)? {
        each(context, cert, |context, item| {
            super::material_bytes(context, item, "options.cert")?;
            call(context, inner, "setCert", &[item])
        })?;
    }
    let key = get(context, snapshot, "key")?;
    let passphrase = get(context, snapshot, "passphrase")?;
    if context.truthy_rooted(key)? {
        let array = context.is_array_rooted(key)?;
        each_key(context, key, |context, item| {
            let mut pem = item;
            let mut pass = passphrase;
            if array && !absent(context, item) {
                let property = get(context, item, "pem")?;
                if !context
                    .rooted_value(property)
                    .is_some_and(|value| value.is_undefined())
                {
                    pem = get(context, item, "pem")?;
                }
                let property = get(context, item, "passphrase")?;
                if !context
                    .rooted_value(property)
                    .is_some_and(|value| value.is_undefined())
                {
                    pass = get(context, item, "passphrase")?;
                }
            }
            super::material_bytes(context, pem, "options.key")?;
            if !absent(context, pass) {
                text(context, pass, "options.passphrase")?;
            }
            call(context, inner, "setKey", &[pem, pass])
        })?;
    }
    let compression = get(context, snapshot, "certificateCompression")?;
    if !absent(context, compression) {
        if !context.is_array_rooted(compression)? {
            return Err(super::super::util_shared_vm::argument_type_error(
                context,
                compression,
                "options.certificateCompression",
                "Array",
            )?);
        }
        let length = get(context, compression, "length")?;
        let length = context
            .rooted_value(length)
            .and_then(|value| value.as_number())
            .unwrap_or(0.0) as usize;
        // IANA TLS CertificateCompressionAlgorithm IDs, also used by OpenSSL.
        const CERT_COMPRESSION_ZLIB: u32 = 1;
        const CERT_COMPRESSION_BROTLI: u32 = 2;
        const CERT_COMPRESSION_ZSTD: u32 = 3;
        const MAX_COMPRESSION_ALGORITHMS: usize = 3;
        const COMPRESSION_FIELD_BITS: usize = u8::BITS as usize;
        if length > MAX_COMPRESSION_ALGORITHMS {
            return Err(super::super::util_shared_vm::argument_value_error(
                context,
                compression,
                "options.certificateCompression",
                "can specify at most 3 algorithms",
            )?);
        }
        if length != 0 {
            let mut packed = length as u32;
            for index in 0..length {
                let algorithm = get(context, compression, &index.to_string())?;
                let id = match context.string_text(algorithm)?.as_deref() {
                    Some("zlib") => CERT_COMPRESSION_ZLIB,
                    Some("brotli") => CERT_COMPRESSION_BROTLI,
                    Some("zstd") => CERT_COMPRESSION_ZSTD,
                    _ => {
                        return Err(super::super::util_shared_vm::argument_value_error(
                            context,
                            algorithm,
                            &format!("options.certificateCompression[{index}]"),
                            "must be 'zlib', 'brotli', or 'zstd'",
                        )?);
                    }
                };
                packed |= id << (COMPRESSION_FIELD_BITS * (index + 1));
            }
            let packed = context.number(f64::from(packed));
            call(context, inner, "setCertificateCompression", &[packed])?;
        }
    }
    let sigalgs = get(context, snapshot, "sigalgs")?;
    if !absent(context, sigalgs) {
        let sigalgs_text = text(context, sigalgs, "options.sigalgs")?;
        if sigalgs_text.is_empty() {
            return Err(super::super::util_shared_vm::argument_value_error(
                context,
                sigalgs,
                "options.sigalgs",
                "is invalid",
            )?);
        }
        call(context, inner, "setSigalgs", &[sigalgs])?;
    }
    let identifier = get(context, snapshot, "privateKeyIdentifier")?;
    let engine = get(context, snapshot, "privateKeyEngine")?;
    if !absent(context, identifier) {
        if absent(context, engine) {
            return Err(super::super::util_shared_vm::argument_value_error(
                context,
                engine,
                "options.privateKeyEngine",
                "is invalid",
            )?);
        }
        if context.truthy_rooted(key)? {
            return Err(super::super::util_shared_vm::argument_value_error(
                context,
                identifier,
                "options.privateKeyIdentifier",
                "is invalid",
            )?);
        }
        if context.string_text(identifier)?.is_none() {
            return Err(
                super::super::util_shared_vm::argument_type_error_with_expected(
                    context,
                    identifier,
                    "options.privateKeyIdentifier",
                    super::super::crypto_engine_shared_vm::EXPECTED_ENGINE_TYPES,
                )?,
            );
        }
        if context.string_text(engine)?.is_none() {
            return Err(
                super::super::util_shared_vm::argument_type_error_with_expected(
                    context,
                    engine,
                    "options.privateKeyEngine",
                    super::super::crypto_engine_shared_vm::EXPECTED_ENGINE_TYPES,
                )?,
            );
        }
        let method = get(context, inner, "setEngineKey")?;
        if !context.truthy_rooted(method)? {
            return Err(super::super::crypto_engine_shared_vm::error(
                context,
                super::super::crypto_engine_shared_vm::Failure::Unsupported,
            ));
        }
        call(context, inner, "setEngineKey", &[identifier, engine])?;
    }
    let curve = get(context, snapshot, "ecdhCurve")?;
    text(context, curve, "options.ecdhCurve")?;
    call(context, inner, "setECDHCurve", &[curve])?;
    let dh = get(context, snapshot, "dhparam")?;
    if !absent(context, dh) {
        super::material_bytes(context, dh, "options.dhparam")?;
        let native = if context.string_text(dh)?.as_deref() == Some("auto") {
            context.boolean(true)
        } else {
            dh
        };
        let method = get(context, inner, "setDHParam")?;
        let warning = context.call_rooted(method, inner, &[native])?;
        if context.truthy_rooted(warning)? {
            let global = context.global_root()?;
            let process = get(context, global, "process")?;
            let emit = get(context, process, "emitWarning")?;
            let kind = context.string_rooted("SecurityWarning");
            context.call_rooted(emit, process, &[warning, kind])?;
        }
    }
    let crl = get(context, snapshot, "crl")?;
    if !absent(context, crl) {
        if context.is_array_rooted(crl)? {
            let iterator = context.iterator_rooted(crl)?;
            while let Some(item) = context.iterator_step_rooted(&iterator)? {
                let result = (|| {
                    super::material_bytes(context, item, "options.crl")?;
                    call(context, inner, "addCRL", &[item])
                })();
                if let Err(error) = result {
                    if let Err(close) = context.iterator_close_rooted(&iterator) {
                        if let Some(exception) = close.exception {
                            context.release_root(exception);
                        }
                    }
                    return Err(error);
                }
            }
        } else {
            super::material_bytes(context, crl, "options.crl")?;
            call(context, inner, "addCRL", &[crl])?;
        }
    }
    let session_id = get(context, snapshot, "sessionIdContext")?;
    if !absent(context, session_id) {
        text(context, session_id, "options.sessionIdContext")?;
        call(context, inner, "setSessionIdContext", &[session_id])?;
    }
    let pfx = get(context, snapshot, "pfx")?;
    if !absent(context, pfx) {
        let array = context.is_array_rooted(pfx)?;
        each(context, pfx, |context, item| {
            let (raw, pass) = if array {
                let raw = get(context, item, "buf")?;
                let raw = if context.truthy_rooted(raw)? {
                    raw
                } else {
                    item
                };
                let pass = get(context, item, "passphrase")?;
                let pass = if context.truthy_rooted(pass)? {
                    pass
                } else {
                    passphrase
                };
                (raw, pass)
            } else {
                (item, passphrase)
            };
            call(context, inner, "loadPKCS12", &[raw, pass])
        })?;
    }
    let engine = get(context, snapshot, "clientCertEngine")?;
    if !absent(context, engine) {
        if context.string_text(engine)?.is_none() {
            return Err(
                super::super::util_shared_vm::argument_type_error_with_expected(
                    context,
                    engine,
                    "options.clientCertEngine",
                    super::super::crypto_engine_shared_vm::EXPECTED_ENGINE_TYPES,
                )?,
            );
        }
        let method = get(context, inner, "setClientCertEngine")?;
        if !context.is_callable_rooted(method)? {
            return Err(super::super::crypto_engine_shared_vm::error(
                context,
                super::super::crypto_engine_shared_vm::Failure::Unsupported,
            ));
        }
        call(context, inner, "setClientCertEngine", &[engine])?;
    }
    let ticket = get(context, snapshot, "ticketKeys")?;
    if !absent(context, ticket) {
        let Some(bytes) = context.view_bytes_rooted(ticket) else {
            return Err(
                super::super::util_shared_vm::argument_type_error_with_expected(
                    context,
                    ticket,
                    "options.ticketKeys",
                    "an instance of Buffer, TypedArray, or DataView",
                )?,
            );
        };
        const TICKET_KEY_BYTES: usize = 3 * 16; // Node ticket name, HMAC key and AES key layout.
        if bytes.len() != TICKET_KEY_BYTES {
            let length = context.number(bytes.len() as f64);
            return Err(super::super::util_shared_vm::argument_value_error(
                context,
                length,
                "options.ticketKeys",
                "must be exactly 48 bytes",
            )?);
        }
        call(context, inner, "setTicketKeys", &[ticket])?;
    }
    let timeout = get(context, snapshot, "sessionTimeout")?;
    if !absent(context, timeout) {
        let Some(number) = context
            .rooted_value(timeout)
            .and_then(|value| value.as_number())
        else {
            return Err(super::super::util_shared_vm::argument_type_error(
                context,
                timeout,
                "options.sessionTimeout",
                "number",
            )?);
        };
        if !number.is_finite()
            || number.fract() != 0.0
            || number < 0.0
            || number > f64::from(i32::MAX)
        {
            let received = super::super::util_shared_vm::range_error_received(context, timeout)?;
            let rule = if number.fract() != 0.0 || !number.is_finite() {
                "an integer".to_owned()
            } else {
                format!(">= 0 && <= {}", i32::MAX)
            };
            let error = context.range_error_rooted(&format!("The value of \"options.sessionTimeout\" is out of range. It must be {rule}. Received {received}"))?;
            super::super::node_errors_shared_vm::decorate_range_error(
                context,
                error,
                "ERR_OUT_OF_RANGE",
            )?;
            super::super::node_errors_shared_vm::hide_range_error(
                context,
                error,
                "ERR_OUT_OF_RANGE",
            )?;
            return Err(context.throw(error));
        }
        call(context, inner, "setSessionTimeout", &[timeout])?;
    }
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
            let roots = bundled_roots()
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
            for root in roots {
                let result = shared
                    .borrow_mut()
                    .tls_contexts
                    .configuration(index)?
                    .cert_store_mut()
                    .add_cert(root.to_owned());
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
