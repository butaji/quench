//! Native ABI constants shared by the public and internal crypto namespaces.
//! Padding values come from the linked OpenSSL headers; the salt/check flags
//! below are the literal ABI definitions in openssl/rsa.h and openssl/dh.h.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
// node_constants.cc retains this historical ABI when OpenSSL removes its macro.
const RSA_SSLV23_PADDING: i32 = 2;
const CACHE: &str = "\0binding:crypto/constants";
#[repr(i32)]
enum SaltLength {
    Digest = -1,
    Auto = -2,
}
#[repr(u32)]
enum DhCheck {
    NotPrime = 1,
    NotSafePrime = 2,
    UnableToCheckGenerator = 4,
    NotSuitableGenerator = 8,
}
pub(crate) fn binding(c: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    if let Some(root) = shared.borrow().builtin_cache.get(CACHE).copied() {
        return Ok(root);
    }
    let object = c.null_object_rooted()?;
    for (name, value) in [
        (
            "ENGINE_METHOD_ALL",
            super::crypto_engine_shared_vm::ENGINE_METHOD_ALL as i32,
        ),
        ("DH_CHECK_P_NOT_PRIME", DhCheck::NotPrime as i32),
        ("DH_CHECK_P_NOT_SAFE_PRIME", DhCheck::NotSafePrime as i32),
        (
            "DH_UNABLE_TO_CHECK_GENERATOR",
            DhCheck::UnableToCheckGenerator as i32,
        ),
        (
            "DH_NOT_SUITABLE_GENERATOR",
            DhCheck::NotSuitableGenerator as i32,
        ),
        ("RSA_PKCS1_PADDING", openssl_sys::RSA_PKCS1_PADDING),
        ("RSA_SSLV23_PADDING", RSA_SSLV23_PADDING),
        ("RSA_NO_PADDING", openssl_sys::RSA_NO_PADDING),
        (
            "RSA_PKCS1_OAEP_PADDING",
            openssl_sys::RSA_PKCS1_OAEP_PADDING,
        ),
        ("RSA_X931_PADDING", openssl_sys::RSA_X931_PADDING),
        ("RSA_PKCS1_PSS_PADDING", openssl_sys::RSA_PKCS1_PSS_PADDING),
        ("RSA_PSS_SALTLEN_DIGEST", SaltLength::Digest as i32),
        ("RSA_PSS_SALTLEN_MAX_SIGN", SaltLength::Auto as i32),
        ("RSA_PSS_SALTLEN_AUTO", SaltLength::Auto as i32),
    ] {
        let property = c.string_rooted(name);
        let value = c.number(value as f64);
        c.define_data_property_rooted(object, property, value, false, true, false)?;
    }
    let cli_options = shared.borrow().cli_options.clone();
    for (name, value) in [
        (
            "defaultCipherList",
            cli_options
                .string_value("--tls-cipher-list")
                .filter(|value| !value.is_empty())
                .unwrap_or(super::tls_shared_vm::DEFAULT_CIPHER_LIST),
        ),
        (
            "defaultCoreCipherList",
            super::tls_shared_vm::DEFAULT_CIPHER_LIST,
        ),
    ] {
        let property = c.string_rooted(name);
        let value = c.string_rooted(value);
        c.define_data_property_rooted(object, property, value, false, true, false)?;
    }
    let root = c.retain(object)?;
    shared.borrow_mut().builtin_cache.insert(CACHE.into(), root);
    Ok(object)
}
