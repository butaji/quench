//! Initial TLS builtin surface for modules that load TLS paths but do not open a TLS socket.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const TLS_FACTORY: &str = quench_js_check::checked_js!(
    r#"(validateContextNative) => {
      const secureContexts = new WeakSet();
      const cipherNames = Object.freeze([
        "aes128-gcm-sha256", "aes256-gcm-sha384", "aes256-sha",
        "ecdhe-ecdsa-aes128-gcm-sha256", "ecdhe-rsa-aes128-gcm-sha256",
        "tls_aes_128_ccm_8_sha256", "tls_aes_128_ccm_sha256",
        "tls_aes_128_gcm_sha256", "tls_aes_256_gcm_sha384",
      ].sort());
      class SecureContext {
        setOptions() {
          if (!secureContexts.has(this)) throw new TypeError("Illegal invocation");
        }
      }
      function createSecureContext(options) {
        validateContextNative(options);
        const context = new SecureContext();
        secureContexts.add(context);
        return { context };
      }
      return {
      getCiphers() { return cipherNames.slice(); },
      SecureContext,
      createSecureContext,
      connect() {
        const error = new Error('TLS sockets are not implemented');
        error.code = 'ERR_METHOD_NOT_IMPLEMENTED';
        throw error;
      },
    };
    }"#
);

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(TLS_FACTORY, "node:tls/shared.js")?;
    let undefined = context.undefined();
    let validate =
        context.host_function(crate::host::shared_vm::operation("tlsValidateContext"))?;
    context.call_rooted(factory, undefined, &[validate])
}

/// Validate material through the cryptographic parser, never through input identity.
/// Socket handshakes and retained SSL_CTX configuration remain separate work.
pub(crate) fn validate_context(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::event_constructor_shared_vm::get;
    let undefined = context.undefined();
    let options = args.first().copied().unwrap_or(undefined);
    if !context.truthy_rooted(options)? {
        return Ok(undefined);
    }
    if !context.is_object_rooted(options)? || context.is_array_rooted(options)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, options, "options", "object",
        )?);
    }
    let crl = get(context, options, "crl")?;
    let passphrase = get(context, options, "passphrase")?;
    let pfx = get(context, options, "pfx")?;
    if !nullish(context, crl) {
        if context.is_array_rooted(crl)? {
            let iterator = context.iterator_rooted(crl)?;
            while let Some(value) = context.iterator_step_rooted(&iterator)? {
                if let Err(error) = validate_crl(context, value) {
                    if let Err(close) = context.iterator_close_rooted(&iterator) {
                        if let Some(exception) = close.exception {
                            context.release_root(exception);
                        }
                    }
                    return Err(error);
                }
            }
        } else {
            validate_crl(context, crl)?;
        }
    }
    if !nullish(context, pfx) {
        if context.is_array_rooted(pfx)? {
            // Node uses primordial Array.prototype.forEach: length is captured,
            // holes are skipped, and each value is read immediately before use.
            let length = get(context, pfx, "length")?;
            let length = context
                .rooted_value(length)
                .and_then(|value| value.as_number())
                .unwrap_or(0.0) as usize;
            for index in 0..length {
                let key = context.string_rooted(&index.to_string());
                if !context.has_property_rooted(pfx, key)? {
                    continue;
                }
                let value = context.get_property_rooted(pfx, key)?;
                let raw = get(context, value, "buf")?;
                let raw = if context.truthy_rooted(raw)? {
                    raw
                } else {
                    value
                };
                let pass = get(context, value, "passphrase")?;
                let pass = if context.truthy_rooted(pass)? {
                    pass
                } else {
                    passphrase
                };
                validate_pfx(context, raw, pass)?;
            }
        } else {
            validate_pfx(context, pfx, passphrase)?;
        }
    }
    Ok(undefined)
}

fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}

fn validate_crl(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<(), RootedError> {
    let bytes = material_bytes(context, value, "options.crl")?;
    openssl::x509::X509Crl::from_pem(&bytes)
        .map_err(|_| operation_error(context, "Failed to parse CRL"))?;
    Ok(())
}

fn validate_pfx(
    context: &mut NativeContext<'_, NodeHost>,
    raw: RootId,
    pass: RootId,
) -> Result<(), RootedError> {
    let bytes = pfx_bytes(context, raw)?;
    let pass = if context.truthy_rooted(pass)? {
        let bytes = if let Some(text) = context.string_text(pass)? {
            text.into_bytes()
        } else if let Some(bytes) = context.view_bytes_rooted(pass) {
            bytes
        } else {
            let error = context.type_error_rooted("Pass phrase must be a buffer")?;
            let key = context.string_rooted("code");
            let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
            context.define_data_property_rooted(error, key, code, true, true, true)?;
            return Err(context.throw(error));
        };
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        bytes[..end].to_vec()
    } else {
        Vec::new()
    };
    let archive = archive_from_bio(&bytes).map_err(|error| parser_error(context, error))?;
    let parsed = parse_archive(&archive, &pass).map_err(|error| parser_error(context, error))?;
    if parsed.pkey.is_none() {
        return Err(operation_error(
            context,
            "Unable to load private key from PFX data",
        ));
    }
    if parsed.cert.is_none() {
        return Err(operation_error(
            context,
            "Unable to load certificate from PFX data",
        ));
    }
    Ok(())
}

fn material_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<Vec<u8>, RootedError> {
    if let Some(text) = context.string_text(value)? {
        return Ok(text.into_bytes());
    }
    if let Some(bytes) = context.view_bytes_rooted(value) {
        return Ok(bytes);
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        context,
        value,
        name,
        "of type string or an instance of Buffer, TypedArray, or DataView",
    )?)
}

fn pfx_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Vec<u8>, RootedError> {
    if let Some(text) = context.string_text(value)? {
        return Ok(text.into_bytes());
    }
    context
        .view_bytes_rooted(value)
        .ok_or_else(|| operation_error(context, "Unable to load PFX certificate"))
}

fn operation_error(context: &mut NativeContext<'_, NodeHost>, message: &str) -> RootedError {
    match context.error_rooted(message) {
        Ok(error) => {
            let key = context.string_rooted("code");
            let code = context.string_rooted("ERR_CRYPTO_OPERATION_FAILED");
            match context.define_data_property_rooted(error, key, code, true, true, true) {
                Ok(_) => context.throw(error),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
}

fn parser_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: openssl::error::ErrorStack,
) -> RootedError {
    let reason = error
        .errors()
        .first()
        .and_then(|error| error.reason())
        .unwrap_or("Unable to load PFX certificate");
    match context.error_rooted(reason) {
        Ok(error) => context.throw(error),
        Err(error) => error,
    }
}

// Node uses the BIO decoder, whose bounded-input errors differ from d2i's
// slice decoder. Keep the same parser entry point rather than rewriting errors.
unsafe extern "C" {
    fn d2i_PKCS12_bio(
        input: *mut openssl_sys::BIO,
        output: *mut *mut openssl_sys::PKCS12,
    ) -> *mut openssl_sys::PKCS12;
}

fn archive_from_bio(bytes: &[u8]) -> Result<openssl::pkcs12::Pkcs12, openssl::error::ErrorStack> {
    use foreign_types::ForeignType;
    openssl::init();
    let length = i32::try_from(bytes.len()).map_err(|_| openssl::error::ErrorStack::get())?;
    // BIO borrows the input until decoding completes; all early exits free it.
    unsafe {
        let input = openssl_sys::BIO_new_mem_buf(bytes.as_ptr().cast(), length);
        if input.is_null() {
            return Err(openssl::error::ErrorStack::get());
        }
        let archive = d2i_PKCS12_bio(input, std::ptr::null_mut());
        openssl_sys::BIO_free_all(input);
        if archive.is_null() {
            Err(openssl::error::ErrorStack::get())
        } else {
            Ok(openssl::pkcs12::Pkcs12::from_ptr(archive))
        }
    }
}

fn parse_archive(
    archive: &openssl::pkcs12::Pkcs12,
    password: &[u8],
) -> Result<openssl::pkcs12::ParsedPkcs12_2, openssl::error::ErrorStack> {
    use foreign_types::ForeignType;
    let mut password = password.to_vec();
    password.push(0);
    let mut key = std::ptr::null_mut();
    let mut cert = std::ptr::null_mut();
    let mut ca = std::ptr::null_mut();
    // PKCS12_parse takes a C password; ownership of each nonnull result
    // transfers once to its Rust RAII wrapper, including failure cleanup.
    let success = unsafe {
        openssl_sys::PKCS12_parse(
            archive.as_ptr(),
            password.as_ptr().cast(),
            &mut key,
            &mut cert,
            &mut ca,
        )
    };
    let parsed = unsafe {
        openssl::pkcs12::ParsedPkcs12_2 {
            pkey: if key.is_null() {
                None
            } else {
                Some(openssl::pkey::PKey::from_ptr(key))
            },
            cert: if cert.is_null() {
                None
            } else {
                Some(openssl::x509::X509::from_ptr(cert))
            },
            ca: if ca.is_null() {
                None
            } else {
                Some(openssl::stack::Stack::from_ptr(ca))
            },
        }
    };
    if success == 1 {
        Ok(parsed)
    } else {
        Err(openssl::error::ErrorStack::get())
    }
}
