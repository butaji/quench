//! OpenSSL error presentation shared by Node crypto and TLS operations.
use quench_runtime::{Host, NativeContext, RootId, RootedError};

// OpenSSL err.h library IDs and Node crypto_util.cc's public code prefixes.
const LIBRARY_CODES: &[(i32, &str)] = &[
    (2, "SYS"),
    (3, "BN"),
    (4, "RSA"),
    (5, "DH"),
    (6, "EVP"),
    (7, "BUF"),
    (8, "OBJ"),
    (9, "PEM"),
    (10, "DSA"),
    (11, "X509"),
    (13, "ASN1"),
    (14, "CONF"),
    (15, "CRYPTO"),
    (16, "EC"),
    (20, "SSL"),
    (32, "BIO"),
    (33, "PKCS7"),
    (34, "X509V3"),
    (35, "PKCS12"),
    (36, "RAND"),
    (37, "DSO"),
    (38, "ENGINE"),
    (39, "OCSP"),
    (40, "UI"),
    (41, "COMP"),
    (42, "ECDSA"),
    (43, "ECDH"),
    (44, "OSSL_STORE"),
    (45, "FIPS"),
    (46, "CMS"),
    (47, "TS"),
    (48, "HMAC"),
    (50, "CT"),
    (51, "ASYNC"),
    (52, "KDF"),
    (53, "SM2"),
    (128, "USER"),
];
const SETTER_MESSAGE_BYTES: usize = 128;
const STACK_MESSAGE_BYTES: usize = 256;

fn message(code: std::ffi::c_ulong, capacity: usize) -> String {
    let mut bytes = vec![0; capacity];
    unsafe {
        ERR_error_string_n(code, bytes.as_mut_ptr().cast(), bytes.len());
    }
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn put<H: Host>(
    context: &mut NativeContext<'_, H>,
    error: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(error, key, value, true, true, true)?;
    Ok(())
}

pub(crate) fn create<H: Host>(
    context: &mut NativeContext<'_, H>,
    stack: &openssl::error::ErrorStack,
    decorate: bool,
) -> Result<RootId, RootedError> {
    let first = stack.errors().first();
    let text = first
        .map(|error| {
            message(
                error.code(),
                if decorate {
                    SETTER_MESSAGE_BYTES
                } else {
                    STACK_MESSAGE_BYTES
                },
            )
        })
        .unwrap_or_else(|| {
            if decorate {
                message(0, SETTER_MESSAGE_BYTES)
            } else {
                "ok".to_owned()
            }
        });
    let error = context.error_rooted(&text)?;
    if stack.errors().len() > 1 {
        let entries = stack.errors()[1..]
            .iter()
            .rev()
            .map(|entry| context.string_rooted(&message(entry.code(), STACK_MESSAGE_BYTES)))
            .collect::<Vec<_>>();
        let entries = context.array_rooted(&entries)?;
        put(context, error, "opensslErrorStack", entries)?;
    }
    if decorate {
        if let Some(first) = first {
            if let Some(library) = first.library() {
                let library = context.string_rooted(library);
                put(context, error, "library", library)?;
            }
            if let Some(reason) = first.reason() {
                let value = context.string_rooted(reason);
                put(context, error, "reason", value)?;
                let library = LIBRARY_CODES
                    .iter()
                    .find(|(code, _)| *code == first.library_code())
                    .map(|(_, name)| *name);
                let prefix = match library {
                    Some("SSL") => "ERR_SSL_".to_owned(),
                    Some(library) => format!("ERR_OSSL_{library}_"),
                    None => "ERR_OSSL_".to_owned(),
                };
                let value = context.string_rooted(&format!(
                    "{prefix}{}",
                    reason.to_ascii_uppercase().replace(' ', "_")
                ));
                put(context, error, "code", value)?;
            }
        }
    }
    Ok(error)
}

pub(crate) fn decorated<H: Host>(
    context: &mut NativeContext<'_, H>,
    stack: openssl::error::ErrorStack,
) -> RootedError {
    match create(context, &stack, true) {
        Ok(error) => context.throw(error),
        Err(error) => error,
    }
}

pub(crate) fn list<H: Host>(
    context: &mut NativeContext<'_, H>,
    stack: openssl::error::ErrorStack,
) -> RootedError {
    match create(context, &stack, false) {
        Ok(error) => context.throw(error),
        Err(error) => error,
    }
}

unsafe extern "C" {
    fn ERR_error_string_n(code: std::ffi::c_ulong, output: *mut std::ffi::c_char, length: usize);
}
