//! Native provider facts consumed by the configured SDK normalization policy.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
#[path = "crypto_provider_facts.rs"]
mod native;
type Context<'a> = NativeContext<'a, NodeHost>;
fn get(c: &mut Context<'_>, o: RootId, name: &str) -> Result<RootId, RootedError> {
    let name = c.string_rooted(name);
    c.get_property_rooted(o, name)
}
fn put(c: &mut Context<'_>, o: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let name = c.string_rooted(name);
    c.define_data_property_rooted(o, name, value, true, true, true)?;
    Ok(())
}
pub(crate) fn install(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    openssl::init();
    // The same constructor/constant values must be available at both SDK ABIs.
    let keys = super::crypto_key_shared_vm::binding(c)?;
    for key in c.own_keys_rooted(keys)? {
        let value = c.get_property_rooted(keys, key)?;
        c.define_data_property_rooted(binding, key, value, true, true, true)?;
    }
    for name in [
        "getFipsCrypto",
        "getFipsCryptoGeneration",
        "setFipsCrypto",
        "getCiphers",
        "getHashes",
        "getMacs",
        "isKeyAlgorithmAvailable",
        "getOpenSSLSecLevelCrypto",
    ] {
        let data = c.string_rooted(name);
        let function = c.host_function_named_with_data(
            crate::host::shared_vm::operation("cryptoProviderFacts"),
            name,
            data,
        )?;
        put(c, binding, name, function)?;
    }
    Ok(())
}
pub(crate) fn operation(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let op = c.host_function_data()?;
    let op = c.to_string(op)?;
    match op.as_str() {
        "getFipsCrypto" => Ok(c.number(if native::fips_enabled() { 1.0 } else { 0.0 })),
        "getFipsCryptoGeneration" => Ok(c.bigint_rooted(i128::from(native::fips_generation()))),
        "setFipsCrypto" => {
            let arg = args.first().copied().unwrap_or_else(|| c.undefined());
            let enable = c.truthy_rooted(arg)?;
            if !native::set_fips(enable) {
                return Err(super::crypto_error_shared_vm::decorated(
                    c,
                    openssl::error::ErrorStack::get(),
                ));
            }
            Ok(c.undefined())
        }
        "isKeyAlgorithmAvailable" => {
            let arg = args.first().copied().unwrap_or_else(|| c.undefined());
            let Some(name) = c.string_text(arg)? else {
                return Err(RootedError::host("Key algorithm name must be a string"));
            };
            Ok(c.boolean(native::key_available(&name)))
        }
        "getOpenSSLSecLevelCrypto" => {
            let builder = openssl::ssl::SslContextBuilder::new(openssl::ssl::SslMethod::tls())
                .map_err(|stack| super::crypto_error_shared_vm::decorated(c, stack))?;
            Ok(c.number(builder.security_level() as f64))
        }
        "getCiphers" | "getHashes" | "getMacs" => {
            let kind = match op.as_str() {
                "getCiphers" => native::Kind::Cipher,
                "getHashes" => native::Kind::Digest,
                _ => native::Kind::Mac,
            };
            let values = native::names(kind)
                .into_iter()
                .map(|name| c.string_rooted(&name))
                .collect::<Vec<_>>();
            c.array_rooted(&values)
        }
        _ => Err(RootedError::host("Unknown native crypto provider fact")),
    }
}
pub(crate) fn webcrypto(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_export(
        c,
        "crypto.js",
        "internal/crypto/webcrypto.js",
        "crypto",
    )
}
pub(crate) fn install_classes(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let source = crate::host::shared_vm::commonjs::node_library_module(
        c,
        "crypto.js",
        "internal/crypto/webcrypto.js",
    )?;
    let global = c.global_root()?;
    for name in ["Crypto", "SubtleCrypto", "CryptoKey"] {
        let value = get(c, source, name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(global, key, value, true, false, true)?;
    }
    Ok(c.undefined())
}
