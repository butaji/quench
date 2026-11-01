//! Cipher discovery follows the active OpenSSL provider and Node TLS policy.
use foreign_types::ForeignType;
use openssl::ssl::{Ssl, SslContextBuilder, SslMethod};
use std::collections::BTreeSet;
use std::ffi::CStr;

// Named Node policy: deps/ncrypto/ncrypto.cc SSLPointer::getCiphers.
const TLS13_CIPHERS: [&str; 5] = [
    "tls_aes_256_gcm_sha384",
    "tls_chacha20_poly1305_sha256",
    "tls_aes_128_gcm_sha256",
    "tls_aes_128_ccm_8_sha256",
    "tls_aes_128_ccm_sha256",
];
unsafe extern "C" {
    fn SSL_get_ciphers(ssl: *const openssl_sys::SSL) -> *mut openssl_sys::OPENSSL_STACK;
}
fn discover() -> Result<Vec<String>, openssl::error::ErrorStack> {
    let ctx = SslContextBuilder::new(SslMethod::tls())?.build();
    let ssl = Ssl::new(&ctx)?;
    let mut names = BTreeSet::new();
    // The SSL owns this borrowed stack and all its cipher descriptions until
    // they have been copied into Rust strings. No foreign pointer is retained.
    unsafe {
        let stack = SSL_get_ciphers(ssl.as_ptr());
        if !stack.is_null() {
            for index in 0..openssl_sys::OPENSSL_sk_num(stack) {
                let cipher =
                    openssl_sys::OPENSSL_sk_value(stack, index).cast::<openssl_sys::SSL_CIPHER>();
                let name = CStr::from_ptr(openssl_sys::SSL_CIPHER_get_name(cipher));
                names.insert(name.to_string_lossy().to_ascii_lowercase());
            }
        }
    }
    names.extend(TLS13_CIPHERS.into_iter().map(str::to_owned));
    Ok(names.into_iter().collect())
}

pub(crate) fn ciphers(
    context: &mut quench_runtime::NativeContext<'_, crate::host::NodeHost>,
    _: quench_runtime::RootId,
    _: &[quench_runtime::RootId],
) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
    let shared = context.host_mut().shared_state();
    if shared.borrow().tls_contexts.ciphers.is_none() {
        let names = discover()
            .map_err(|errors| super::super::crypto_error_shared_vm::decorated(context, errors))?;
        shared.borrow_mut().tls_contexts.ciphers = Some(names);
    }
    let names = shared
        .borrow()
        .tls_contexts
        .ciphers
        .as_ref()
        .unwrap()
        .clone();
    let values = names
        .iter()
        .map(|name| context.string_rooted(name))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn ca_certificates(
    context: &mut quench_runtime::NativeContext<'_, crate::host::NodeHost>,
    _: quench_runtime::RootId,
    args: &[quench_runtime::RootId],
) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
    let is_set_default = if let Some(value) = args.first() {
        context.string_text(*value)?.as_deref() == Some("setDefault")
    } else {
        false
    };
    if is_set_default {
        return set_default_ca_certificates(context, args.get(1).copied());
    }
    let kind = if let Some(value) = args.first() {
        context.string_text(*value)?
    } else {
        Some("default".to_owned())
    };
    let Some(kind) = kind else {
        let error = context.type_error_rooted("The \"type\" argument must be of type string")?;
        super::super::node_errors_shared_vm::decorate_type_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?;
        return Err(context.throw(error));
    };
    if !matches!(kind.as_str(), "default" | "bundled" | "system" | "extra") {
        let error = context.type_error_rooted(&format!(
            "The \"type\" argument must be one of: 'default', 'bundled', 'system', 'extra'. Received '{kind}'"
        ))?;
        super::super::node_errors_shared_vm::decorate_type_error(
            context,
            error,
            "ERR_INVALID_ARG_VALUE",
        )?;
        return Err(context.throw(error));
    }

    let use_system = args
        .get(1)
        .and_then(|value| context.rooted_value(*value))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if kind == "default" {
        let defaults = context
            .host_mut()
            .shared_state()
            .borrow()
            .tls_contexts
            .default_ca_certificates
            .clone();
        if let Some(certificates) = defaults {
            let values = certificates
                .iter()
                .map(|certificate| context.string_rooted(certificate))
                .collect::<Vec<_>>();
            return context.array_rooted(&values);
        }
    }
    let system_certificates = || -> Vec<String> {
        for path in [
            "/etc/ssl/cert.pem",
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/pki/tls/certs/ca-bundle.crt",
        ] {
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            if let Ok(certificates) = openssl::x509::X509::stack_from_pem(&bytes) {
                return certificates
                    .iter()
                    .filter_map(|certificate| certificate.to_pem().ok())
                    .filter_map(|pem| String::from_utf8(pem).ok())
                    .collect();
            }
        }
        Vec::new()
    };
    let mut certificates = Vec::new();
    if matches!(kind.as_str(), "default" | "bundled") {
        certificates.extend(
            include_str!("bundled-roots.pem")
                .split_inclusive("-----END CERTIFICATE-----")
                .map(str::trim)
                .filter(|pem| !pem.is_empty())
                .map(str::to_owned),
        );
    }
    if kind == "system" || (kind == "default" && use_system) {
        certificates.extend(system_certificates());
    }
    if matches!(kind.as_str(), "default" | "extra") {
        if let Some(path) = std::env::var_os("NODE_EXTRA_CA_CERTS") {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(text) = String::from_utf8(bytes) {
                    if !text.is_empty() {
                        certificates.push(text);
                    }
                }
            }
        }
    }
    let values = certificates
        .iter()
        .map(|certificate| context.string_rooted(certificate))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

fn set_default_ca_certificates(
    context: &mut quench_runtime::NativeContext<'_, crate::host::NodeHost>,
    certs: Option<quench_runtime::RootId>,
) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
    use quench_runtime::RootedError;
    let Some(certs) = certs else {
        return Err(RootedError::host(
            "missing tls.setDefaultCACertificates argument",
        ));
    };
    if !context.is_array_rooted(certs)? {
        let error =
            context.type_error_rooted("The \"certs\" argument must be an instance of Array")?;
        super::super::node_errors_shared_vm::decorate_type_error(
            context,
            error,
            "ERR_INVALID_ARG_TYPE",
        )?;
        return Err(context.throw(error));
    }
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(certs, length_key)?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0)
        .max(0.0) as usize;
    let mut certificates = Vec::<String>::new();
    for index in 0..length {
        let key = context.string_rooted(&index.to_string());
        let value = context.get_property_rooted(certs, key)?;
        let bytes = if let Some(text) = context.string_text(value)? {
            text.into_bytes()
        } else if let Some(bytes) = context.view_bytes_rooted(value) {
            bytes
        } else {
            let error = context.type_error_rooted(&format!(
                "The \"certs[{index}]\" argument must be of type string or an instance of ArrayBufferView"
            ))?;
            super::super::node_errors_shared_vm::decorate_type_error(
                context,
                error,
                "ERR_INVALID_ARG_TYPE",
            )?;
            return Err(context.throw(error));
        };
        let parsed = match openssl::x509::X509::stack_from_pem(&bytes) {
            Ok(parsed) if !parsed.is_empty() => parsed,
            Ok(_) | Err(_) if certificates.is_empty() => {
                return Err(super::operation_error(
                    context,
                    "No valid certificates found in the provided array",
                ));
            }
            Err(error) => {
                return Err(super::super::crypto_error_shared_vm::decorated(
                    context, error,
                ));
            }
            Ok(_) => continue,
        };
        for certificate in parsed {
            let pem = certificate
                .to_pem()
                .map_err(|error| super::super::crypto_error_shared_vm::decorated(context, error))?;
            let pem =
                String::from_utf8(pem).map_err(|error| RootedError::host(error.to_string()))?;
            if !certificates.contains(&pem) {
                certificates.push(pem);
            }
        }
    }
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .tls_contexts
        .default_ca_certificates = Some(certificates);
    Ok(context.undefined())
}
