//! Native TLS configuration, discovery, and certificate name authorization.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) mod bundled;
pub(crate) mod certificate;
pub(crate) mod configuration;
pub(crate) mod contexts;
pub(crate) mod discovery;
pub(crate) mod facade;
pub(crate) mod handle;
pub(crate) mod identity;
pub(crate) mod server;
pub(crate) mod sessions;
pub(crate) mod socket;
pub(crate) mod surface;

/// Build the legacy TLS internal-module facade and emit Node's deprecation
/// warning when the module is first loaded through CommonJS.
pub(crate) fn deprecated_internal_module(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    module_name: &str,
) -> Result<RootId, RootedError> {
    let emit_warning_key = context.string_rooted("emitWarning");
    let emit_warning = context.get_property_rooted(process, emit_warning_key)?;
    let message = context.string_rooted(&format!(
        "The {module_name} module is deprecated. Use `node:tls` instead."
    ));
    let warning_type = context.string_rooted("DeprecationWarning");
    let code = context.string_rooted("DEP0192");
    context.call_rooted(
        emit_warning,
        process,
        &[message, warning_type, code],
    )?;
    context.object_rooted()
}

// Node's default TLS policy, from src/node_constants.h DEFAULT_CIPHER_LIST_CORE.
pub(crate) const DEFAULT_CIPHER_LIST: &str = concat!(
    "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:",
    "ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:",
    "ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:",
    "DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:",
    "ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:",
    "DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"
);


/// The `tls` builtin the module cache keeps, so every class shares its `TLSSocket` and
/// `Server`.
pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::tls_module(context)
}

/// Builds the `tls` builtin (`module`).
pub(crate) fn build_module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let api = facade::install(context)?;
    handle::registry(context)?;
    let net = crate::host::shared_vm::commonjs::net_module(context)?;
    let net_key = context.string_rooted("identityNet");
    context.define_data_property_rooted(api, net_key, net, false, false, false)?;
    let binding = facade::binding(context)?;
    let constructor = socket::get(context, binding, "SecureContext")?;
    let key = context.string_rooted("contextConstructor");
    context.define_data_property_rooted(api, key, constructor, true, true, true)?;
    let defaults = default_versions(context);
    let module = surface::build(context, api, net, &defaults)?;
    for (name, field) in [("TLSSocket", "socketConstructor"), ("Server", "serverConstructor")] {
        let class = socket::get(context, module, name)?;
        let key = context.string_rooted(field);
        context.define_data_property_rooted(api, key, class, false, false, false)?;
    }
    let key = context.string_rooted("module");
    context.set_property_rooted(api, key, module, api)?;
    Ok(module)
}

/// The `DEFAULT_*` policy, with Node's CLI precedence: the lowest enabled minimum and the
/// highest enabled maximum win.
fn default_versions(context: &mut NativeContext<'_, NodeHost>) -> [(&'static str, String); 4] {
    let cli_options = context.host_mut().shared_state().borrow().cli_options.clone();
    let minimum = [
        ("--tls-min-v1.0", "TLSv1"),
        ("--tls-min-v1.1", "TLSv1.1"),
        ("--tls-min-v1.2", "TLSv1.2"),
        ("--tls-min-v1.3", "TLSv1.3"),
    ]
    .into_iter()
    .find_map(|(flag, version)| cli_options.enabled(flag).then_some(version))
    .unwrap_or("TLSv1.2");
    let maximum = [("--tls-max-v1.3", "TLSv1.3"), ("--tls-max-v1.2", "TLSv1.2")]
        .into_iter()
        .find_map(|(flag, version)| cli_options.enabled(flag).then_some(version))
        .unwrap_or("TLSv1.3");
    let ciphers = cli_options
        .string_value("--tls-cipher-list")
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CIPHER_LIST);
    [
        ("DEFAULT_CIPHERS", ciphers.to_owned()),
        ("DEFAULT_ECDH_CURVE", "auto".to_owned()),
        ("DEFAULT_MIN_VERSION", minimum.to_owned()),
        ("DEFAULT_MAX_VERSION", maximum.to_owned()),
    ]
}

fn parsed_pfx(
    context: &mut NativeContext<'_, NodeHost>,
    raw: RootId,
    pass: RootId,
) -> Result<openssl::pkcs12::ParsedPkcs12_2, RootedError> {
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
    Ok(parsed)
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

pub(crate) fn operation_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> RootedError {
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

/// The certificates of a PEM trust list, as Node's `addCACert` reads them
/// (`PEM_read_bio_X509_AUX`): `CERTIFICATE`, `X509 CERTIFICATE` and `TRUSTED CERTIFICATE`
/// blocks alike.
pub(crate) fn trusted_certificates_from_pem(pem: &[u8]) -> Result<Vec<openssl::x509::X509>, openssl::error::ErrorStack> {
    use foreign_types::ForeignType;
    openssl::init();
    let length = i32::try_from(pem.len()).map_err(|_| openssl::error::ErrorStack::get())?;
    let mut certificates = Vec::new();
    // The BIO borrows `pem` for this call only and is freed on every exit; each
    // certificate's ownership transfers once to its Rust wrapper.
    unsafe {
        let input = openssl_sys::BIO_new_mem_buf(pem.as_ptr().cast(), length);
        if input.is_null() {
            return Err(openssl::error::ErrorStack::get());
        }
        let outcome = loop {
            let certificate = PEM_read_bio_X509_AUX(input, std::ptr::null_mut(), None, std::ptr::null_mut());
            if certificate.is_null() {
                let errors = openssl::error::ErrorStack::get();
                let exhausted = errors.errors().last().is_some_and(|error| {
                    error.library_code() == openssl_sys::ERR_LIB_PEM && error.reason_code() == openssl_sys::PEM_R_NO_START_LINE
                });
                break if exhausted { Ok(()) } else { Err(errors) };
            }
            certificates.push(openssl::x509::X509::from_ptr(certificate));
        };
        openssl_sys::BIO_free_all(input);
        outcome.map(|()| certificates)
    }
}

// Node uses the BIO decoder, whose bounded-input errors differ from d2i's
// slice decoder. Keep the same parser entry point rather than rewriting errors.
unsafe extern "C" {
    fn PEM_read_bio_X509_AUX(
        input: *mut openssl_sys::BIO,
        output: *mut *mut openssl_sys::X509,
        callback: Option<unsafe extern "C" fn(*mut std::ffi::c_char, std::ffi::c_int, std::ffi::c_int, *mut std::ffi::c_void) -> std::ffi::c_int>,
        data: *mut std::ffi::c_void,
    ) -> *mut openssl_sys::X509;
    #[cfg(quench_openssl_provider)]
    fn BIO_s_secmem() -> *const openssl_sys::BIO_METHOD;
    fn d2i_PKCS12_bio(
        input: *mut openssl_sys::BIO,
        output: *mut *mut openssl_sys::PKCS12,
    ) -> *mut openssl_sys::PKCS12;
}

fn archive_from_bio(bytes: &[u8]) -> Result<openssl::pkcs12::Pkcs12, openssl::error::ErrorStack> {
    use foreign_types::ForeignType;
    openssl::init();
    let length = i32::try_from(bytes.len()).map_err(|_| openssl::error::ErrorStack::get())?;
    // Keep the BIO and its backing bytes alive until decoding; free on every exit.
    unsafe {
        // Node LoadBIO owns a writable secure-memory BIO and writes the
        // supplied bytes into it. A borrowed empty BIO has different EOF/error
        // behavior on OpenSSL3.5, so preserve the actual input BIO state too.
        #[cfg(quench_openssl_provider)]
        let input = openssl_sys::BIO_new(BIO_s_secmem());
        #[cfg(not(quench_openssl_provider))]
        let input = openssl_sys::BIO_new_mem_buf(bytes.as_ptr().cast(), length);
        if input.is_null() {
            return Err(openssl::error::ErrorStack::get());
        }
        #[cfg(quench_openssl_provider)]
        if openssl_sys::BIO_write(input, bytes.as_ptr().cast(), length) != length {
            openssl_sys::BIO_free_all(input);
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
