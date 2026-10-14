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
