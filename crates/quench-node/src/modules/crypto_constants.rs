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
/// `SSL_OP_*` option bits, from OpenSSL 3's openssl/ssl.h (the bits Node reports).
mod ssl_op {
    pub(super) const LEGACY_SERVER_CONNECT: u64 = 1 << 2;
    pub(super) const ALLOW_NO_DHE_KEX: u64 = 1 << 10;
    pub(super) const DONT_INSERT_EMPTY_FRAGMENTS: u64 = 1 << 11;
    pub(super) const NO_QUERY_MTU: u64 = 1 << 12;
    pub(super) const COOKIE_EXCHANGE: u64 = 1 << 13;
    pub(super) const NO_TICKET: u64 = 1 << 14;
    pub(super) const CISCO_ANYCONNECT: u64 = 1 << 15;
    pub(super) const NO_SESSION_RESUMPTION_ON_RENEGOTIATION: u64 = 1 << 16;
    pub(super) const NO_COMPRESSION: u64 = 1 << 17;
    pub(super) const ALLOW_UNSAFE_LEGACY_RENEGOTIATION: u64 = 1 << 18;
    pub(super) const NO_ENCRYPT_THEN_MAC: u64 = 1 << 19;
    pub(super) const PRIORITIZE_CHACHA: u64 = 1 << 21;
    pub(super) const CIPHER_SERVER_PREFERENCE: u64 = 1 << 22;
    pub(super) const TLS_ROLLBACK_BUG: u64 = 1 << 23;
    pub(super) const NO_SSLV3: u64 = 1 << 25;
    pub(super) const NO_TLSV1: u64 = 1 << 26;
    pub(super) const NO_TLSV1_2: u64 = 1 << 27;
    pub(super) const NO_TLSV1_1: u64 = 1 << 28;
    pub(super) const NO_TLSV1_3: u64 = 1 << 29;
    pub(super) const NO_RENEGOTIATION: u64 = 1 << 30;
    pub(super) const CRYPTOPRO_TLSEXT_BUG: u64 = 1 << 31;
    /// SSLv2 support was removed; the option is kept as no bits.
    pub(super) const NO_SSLV2: u64 = 0;
    /// `SSL_OP_ALL`: the bug workarounds OpenSSL enables together.
    pub(super) const ALL: u64 = CRYPTOPRO_TLSEXT_BUG | DONT_INSERT_EMPTY_FRAGMENTS | TLSEXT_PADDING | SAFARI_ECDHE_ECDSA_BUG;
    const TLSEXT_PADDING: u64 = 1 << 4;
    const SAFARI_ECDHE_ECDSA_BUG: u64 = 1 << 6;
}
/// `ENGINE_METHOD_*` selection bits, from openssl/engine.h.
mod engine_method {
    pub(super) const RSA: u32 = 0x1;
    pub(super) const DSA: u32 = 0x2;
    pub(super) const DH: u32 = 0x4;
    pub(super) const RAND: u32 = 0x8;
    pub(super) const CIPHERS: u32 = 0x40;
    pub(super) const DIGESTS: u32 = 0x80;
    pub(super) const PKEY_METHS: u32 = 0x200;
    pub(super) const PKEY_ASN1_METHS: u32 = 0x400;
    pub(super) const EC: u32 = 0x800;
    pub(super) const NONE: u32 = 0;
}
/// `point_conversion_form_t`, from openssl/ec.h.
#[repr(u32)]
enum PointConversion {
    Compressed = 2,
    Uncompressed = 4,
    Hybrid = 6,
}
/// A crypto constant's value: a number, or Node's default cipher list.
enum Constant {
    Number(f64),
    Text(&'static str),
}
pub(crate) fn binding(c: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    if let Some(root) = shared.borrow().builtin_cache.get(CACHE).copied() {
        return Ok(root);
    }
    let object = c.null_object_rooted()?;
    let number = |value: f64| Constant::Number(value);
    // SAFETY: OpenSSL_version_num reads a constant of the linked library.
    let openssl_version = unsafe { openssl_sys::OpenSSL_version_num() };
    // node_constants.cc's DefineCryptoConstants order.
    let constants = [
        ("OPENSSL_VERSION_NUMBER", number(openssl_version as f64)),
        ("SSL_OP_ALL", number(ssl_op::ALL as f64)),
        ("SSL_OP_ALLOW_NO_DHE_KEX", number(ssl_op::ALLOW_NO_DHE_KEX as f64)),
        ("SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION", number(ssl_op::ALLOW_UNSAFE_LEGACY_RENEGOTIATION as f64)),
        ("SSL_OP_CIPHER_SERVER_PREFERENCE", number(ssl_op::CIPHER_SERVER_PREFERENCE as f64)),
        ("SSL_OP_CISCO_ANYCONNECT", number(ssl_op::CISCO_ANYCONNECT as f64)),
        ("SSL_OP_COOKIE_EXCHANGE", number(ssl_op::COOKIE_EXCHANGE as f64)),
        ("SSL_OP_CRYPTOPRO_TLSEXT_BUG", number(ssl_op::CRYPTOPRO_TLSEXT_BUG as f64)),
        ("SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS", number(ssl_op::DONT_INSERT_EMPTY_FRAGMENTS as f64)),
        ("SSL_OP_LEGACY_SERVER_CONNECT", number(ssl_op::LEGACY_SERVER_CONNECT as f64)),
        ("SSL_OP_NO_COMPRESSION", number(ssl_op::NO_COMPRESSION as f64)),
        ("SSL_OP_NO_ENCRYPT_THEN_MAC", number(ssl_op::NO_ENCRYPT_THEN_MAC as f64)),
        ("SSL_OP_NO_QUERY_MTU", number(ssl_op::NO_QUERY_MTU as f64)),
        ("SSL_OP_NO_RENEGOTIATION", number(ssl_op::NO_RENEGOTIATION as f64)),
        ("SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION", number(ssl_op::NO_SESSION_RESUMPTION_ON_RENEGOTIATION as f64)),
        ("SSL_OP_NO_SSLv2", number(ssl_op::NO_SSLV2 as f64)),
        ("SSL_OP_NO_SSLv3", number(ssl_op::NO_SSLV3 as f64)),
        ("SSL_OP_NO_TICKET", number(ssl_op::NO_TICKET as f64)),
        ("SSL_OP_NO_TLSv1", number(ssl_op::NO_TLSV1 as f64)),
        ("SSL_OP_NO_TLSv1_1", number(ssl_op::NO_TLSV1_1 as f64)),
        ("SSL_OP_NO_TLSv1_2", number(ssl_op::NO_TLSV1_2 as f64)),
        ("SSL_OP_NO_TLSv1_3", number(ssl_op::NO_TLSV1_3 as f64)),
        ("SSL_OP_PRIORITIZE_CHACHA", number(ssl_op::PRIORITIZE_CHACHA as f64)),
        ("SSL_OP_TLS_ROLLBACK_BUG", number(ssl_op::TLS_ROLLBACK_BUG as f64)),
        ("ENGINE_METHOD_RSA", number(f64::from(engine_method::RSA))),
        ("ENGINE_METHOD_DSA", number(f64::from(engine_method::DSA))),
        ("ENGINE_METHOD_DH", number(f64::from(engine_method::DH))),
        ("ENGINE_METHOD_RAND", number(f64::from(engine_method::RAND))),
        ("ENGINE_METHOD_EC", number(f64::from(engine_method::EC))),
        ("ENGINE_METHOD_CIPHERS", number(f64::from(engine_method::CIPHERS))),
        ("ENGINE_METHOD_DIGESTS", number(f64::from(engine_method::DIGESTS))),
        ("ENGINE_METHOD_PKEY_METHS", number(f64::from(engine_method::PKEY_METHS))),
        ("ENGINE_METHOD_PKEY_ASN1_METHS", number(f64::from(engine_method::PKEY_ASN1_METHS))),
        ("ENGINE_METHOD_ALL", number(f64::from(super::crypto_engine_shared_vm::ENGINE_METHOD_ALL))),
        ("ENGINE_METHOD_NONE", number(f64::from(engine_method::NONE))),
        ("DH_CHECK_P_NOT_SAFE_PRIME", number(f64::from(DhCheck::NotSafePrime as u32))),
        ("DH_CHECK_P_NOT_PRIME", number(f64::from(DhCheck::NotPrime as u32))),
        ("DH_UNABLE_TO_CHECK_GENERATOR", number(f64::from(DhCheck::UnableToCheckGenerator as u32))),
        ("DH_NOT_SUITABLE_GENERATOR", number(f64::from(DhCheck::NotSuitableGenerator as u32))),
        ("RSA_PKCS1_PADDING", number(f64::from(openssl_sys::RSA_PKCS1_PADDING))),
        ("RSA_SSLV23_PADDING", number(f64::from(RSA_SSLV23_PADDING))),
        ("RSA_NO_PADDING", number(f64::from(openssl_sys::RSA_NO_PADDING))),
        ("RSA_PKCS1_OAEP_PADDING", number(f64::from(openssl_sys::RSA_PKCS1_OAEP_PADDING))),
        ("RSA_X931_PADDING", number(f64::from(openssl_sys::RSA_X931_PADDING))),
        ("RSA_PKCS1_PSS_PADDING", number(f64::from(openssl_sys::RSA_PKCS1_PSS_PADDING))),
        ("RSA_PSS_SALTLEN_DIGEST", number(f64::from(SaltLength::Digest as i32))),
        ("RSA_PSS_SALTLEN_MAX_SIGN", number(f64::from(SaltLength::Auto as i32))),
        ("RSA_PSS_SALTLEN_AUTO", number(f64::from(SaltLength::Auto as i32))),
        ("defaultCoreCipherList", Constant::Text(super::tls_shared_vm::DEFAULT_CIPHER_LIST)),
        ("TLS1_VERSION", number(f64::from(openssl_sys::TLS1_VERSION))),
        ("TLS1_1_VERSION", number(f64::from(openssl_sys::TLS1_1_VERSION))),
        ("TLS1_2_VERSION", number(f64::from(openssl_sys::TLS1_2_VERSION))),
        ("TLS1_3_VERSION", number(f64::from(openssl_sys::TLS1_3_VERSION))),
        ("POINT_CONVERSION_COMPRESSED", number(f64::from(PointConversion::Compressed as u32))),
        ("POINT_CONVERSION_UNCOMPRESSED", number(f64::from(PointConversion::Uncompressed as u32))),
        ("POINT_CONVERSION_HYBRID", number(f64::from(PointConversion::Hybrid as u32))),
    ];
    for (name, value) in constants {
        let property = c.string_rooted(name);
        let value = match value {
            Constant::Number(value) => c.number(value),
            Constant::Text(text) => c.string_rooted(text),
        };
        c.define_data_property_rooted(object, property, value, false, true, false)?;
    }
    let root = c.retain(object)?;
    shared.borrow_mut().builtin_cache.insert(CACHE.into(), root);
    Ok(object)
}
