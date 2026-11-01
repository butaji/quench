//! Native provider facts consumed by the configured SDK normalization policy.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
#[path = "crypto_provider_facts.rs"]
pub(crate) mod native;
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
        "secureBuffer",
        "secureHeapUsed",
        "randomFillSync",
        "getFipsCrypto",
        "getFipsCryptoGeneration",
        "setFipsCrypto",
        "testFipsCrypto",
        "getCiphers",
        "getHashes",
        "getCachedAliases",
        "getMacs",
        "timingSafeEqual",
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
        "timingSafeEqual" => {
            let left = args.first().copied().unwrap_or_else(|| c.undefined());
            let right = args.get(1).copied().unwrap_or_else(|| c.undefined());
            let mut bytes = Vec::with_capacity(2);
            for (value, name) in [(left, "buf1"), (right, "buf2")] {
                if !c.has_builtin_brand_rooted(
                    value,
                    quench_runtime::BuiltinBrand::ArrayBufferView,
                )? {
                    let error = c.type_error_rooted(&format!(
                        "The \"{name}\" argument must be an instance of Buffer, TypedArray, or DataView"
                    ))?;
                    let key = c.string_rooted("code");
                    let code = c.string_rooted("ERR_INVALID_ARG_TYPE");
                    c.define_data_property_rooted(error, key, code, true, true, true)?;
                    return Err(c.throw(error));
                }
                bytes.push(c.view_bytes_rooted(value).ok_or_else(|| {
                    RootedError::host("ArrayBufferView lost its byte storage")
                })?);
            }
            if bytes[0].len() != bytes[1].len() {
                let error = c.range_error_rooted("Input buffers must have the same byte length")?;
                let key = c.string_rooted("code");
                let code = c.string_rooted("ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH");
                c.define_data_property_rooted(error, key, code, true, true, true)?;
                return Err(c.throw(error));
            }
            let mut difference = 0u8;
            for (left, right) in bytes[0].iter().zip(&bytes[1]) {
                difference |= left ^ right;
            }
            Ok(c.boolean(difference == 0))
        }
        "getHashes" => super::crypto_secret_job_shared_vm::digest_inventory(c, false),
        "getCachedAliases" => super::crypto_secret_job_shared_vm::digest_inventory(c, true),
        "secureHeapUsed" => Ok(c.bigint_rooted(native::secure_heap_used() as i128)),
        "secureBuffer" => {
            if native::secure_heap_initialized() {
                let error = c.error_rooted(
                    "Active OpenSSL secure heap requires native backing-store adoption",
                )?;
                let key = c.string_rooted("code");
                let code = c.string_rooted("ERR_OPERATION_FAILED");
                c.define_data_property_rooted(error, key, code, true, true, true)?;
                return Err(c.throw(error));
            }
            let value = args.first().copied().unwrap_or_else(|| c.undefined());
            let length = c
                .rooted_value(value)
                .and_then(|v| v.as_number())
                .filter(|n| n.is_finite() && *n >= 0.0 && *n <= u32::MAX as f64 && n.fract() == 0.0)
                .ok_or_else(|| RootedError::host("Invalid native secure buffer length"))?
                as usize;
            // With secure heap disabled, ordinary VM-owned backing is the same
            // allowed allocation class as Node's sandbox copy; GC owns lifetime.
            c.uint8_array_rooted(&vec![0; length])
        }
        "randomFillSync" => random_fill(c, args),
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
        "testFipsCrypto" => Ok(c.number(if native::test_fips_provider() {
            1.0
        } else {
            0.0
        })),
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
            Ok(c.number(builder.build().security_level() as f64))
        }
        "getCiphers" | "getMacs" => {
            let kind = match op.as_str() {
                "getCiphers" => native::Kind::Cipher,
                _ => native::Kind::Mac,
            };
            let mut names = native::names(kind);
            if matches!(kind, native::Kind::Mac) {
                // Node's public MAC inventory is normalized to lowercase and
                // deduplicated even when OpenSSL advertises mixed-case aliases.
                names = names
                    .into_iter()
                    .filter(|name| {
                        let mut parts = name.split('.');
                        let first = parts.next().unwrap_or_default();
                        let rest = parts.collect::<Vec<_>>();
                        !(rest.len() >= 1
                            && !first.is_empty()
                            && first.bytes().all(|byte| byte.is_ascii_digit())
                            && rest.iter().all(|part| {
                                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                            }))
                    })
                    .map(|name| name.to_ascii_lowercase())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
            }
            let values = names
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

/// SDK already converted BufferSource ranges to bytes. Use intrinsic slot getters
/// and one bulk typed-array write; never evaluate public .buffer/.byteOffset.
fn random_fill(c: &mut Context<'_>, args: &[RootId]) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let input = args.first().copied().unwrap_or(missing);
    let integer = |c: &mut Context<'_>, index: usize| -> Result<usize, RootedError> {
        let value = args.get(index).copied().unwrap_or(missing);
        let n = c.rooted_value(value).and_then(|v| v.as_number());
        match n {
            Some(n) if n >= 0.0 && n.is_finite() && n.fract() == 0.0 && n <= usize::MAX as f64 => {
                Ok(n as usize)
            }
            _ => Err(RootedError::host("Invalid native random fill byte range")),
        }
    };
    let offset = integer(c, 1)?;
    let size = integer(c, 2)?;
    let (destination, set) = random_destination(c, input, offset, size)?;
    let mut bytes = vec![0; size];
    openssl::rand::rand_bytes(&mut bytes)
        .map_err(|stack| super::crypto_error_shared_vm::decorated(c, stack))?;
    random_write(c, destination, set, &bytes)?;
    Ok(c.undefined())
}

/// One generation owner for actual FIPS transitions and every SDK digest cache.
pub(crate) fn fips_generation() -> u64 {
    native::fips_generation()
}

pub(crate) fn random_destination(
    c: &mut Context<'_>,
    input: RootId,
    offset: usize,
    size: usize,
) -> Result<(RootId, RootId), RootedError> {
    use quench_runtime::BuiltinBrand;
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let constructor = get(c, global, "Uint8Array")?;
        let byte_proto = get(c, constructor, "prototype")?;
        let typed_proto = c.prototype_rooted(byte_proto)?;
        let setter = get(c, typed_proto, "set")?;
        let (backing, base, length) =
            if c.has_builtin_brand_rooted(input, BuiltinBrand::AnyArrayBuffer)? {
                let array = get(c, global, "ArrayBuffer")?;
                let proto = get(c, array, "prototype")?;
                let proto = if c.has_builtin_brand_rooted(input, BuiltinBrand::SharedArrayBuffer)? {
                    let ctor = get(c, global, "SharedArrayBuffer")?;
                    get(c, ctor, "prototype")?
                } else {
                    proto
                };
                let key = c.string_rooted("byteLength");
                let descriptor = c.own_property_descriptor_rooted(proto, key)?;
                let getter = get(c, descriptor, "get")?;
                let length = c.call_rooted(getter, input, &[])?;
                (input, 0, c.to_number(length)? as usize)
            } else {
                let proto = if c.has_builtin_brand_rooted(input, BuiltinBrand::DataView)? {
                    let ctor = get(c, global, "DataView")?;
                    get(c, ctor, "prototype")?
                } else {
                    typed_proto
                };
                let mut values = Vec::new();
                for name in ["buffer", "byteOffset", "byteLength"] {
                    let key = c.string_rooted(name);
                    let desc = c.own_property_descriptor_rooted(proto, key)?;
                    let getter = get(c, desc, "get")?;
                    values.push(c.call_rooted(getter, input, &[])?);
                }
                (
                    values[0],
                    c.to_number(values[1])? as usize,
                    c.to_number(values[2])? as usize,
                )
            };
        if offset.checked_add(size).is_none_or(|end| end > length) {
            return Err(RootedError::host("Native random fill range exceeds buffer"));
        }
        let offset = c.number((base + offset) as f64);
        let length = c.number(size as f64);
        let destination =
            c.construct_rooted(constructor, constructor, &[backing, offset, length])?;
        Ok((destination, setter))
    })
}
pub(crate) fn random_write(
    c: &mut Context<'_>,
    destination: RootId,
    set: RootId,
    bytes: &[u8],
) -> Result<(), RootedError> {
    let source = c.uint8_array_rooted(bytes)?;
    c.call_rooted(set, destination, &[source])?;
    Ok(())
}
