//! Crypto module's WebCrypto namespace builder.
use quench_runtime::host_api;
use quench_runtime::value::Value;

pub(crate) fn subtle_object() -> Value {
    host_api::object([
        ("digest", crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_DIGEST)),
        ("encrypt", crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_ENCRYPT)),
        ("decrypt", crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_DECRYPT)),
        ("sign", crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_SIGN)),
        ("verify", crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_VERIFY)),
        ("generateKey", crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_GENERATE_KEY)),
        ("importKey", crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_IMPORT_KEY)),
        ("exportKey", crate::host::capability(crate::registry::SPEC_CRYPTO_SUBTLE_EXPORT_KEY)),
    ].into_iter().map(|(name, value)| (name.to_string(), value)).collect())
}