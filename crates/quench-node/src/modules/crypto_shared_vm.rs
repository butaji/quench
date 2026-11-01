//! The hash surface used by the pinned framework package paths.
//!
//! `etag` calls `createHash("sha1").update(string, "utf8").digest("base64")`.
//! Keep that Node-facing wrapper in the guest realm and the digest primitive
//! in Rust; it shares the SHA-1 implementation with the legacy Node module.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::sync::Arc;


/// Node exposes crypto before imports, but initializes its implementation lazily.
pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let global = context.global_root()?;

    #[cfg(quench_openssl_provider)]
    {
        let exceptions = context.host_function(crate::host::shared_vm::operation(
            "webDomExceptionInitialize",
        ))?;
        context.define_lazy_data_properties_rooted(
            global,
            &["DOMException", "QuotaExceededError"],
            false,
            exceptions,
        )?;
        let initialize =
            context.host_function(crate::host::shared_vm::operation("cryptoWebClasses"))?;
        context.define_lazy_data_properties_rooted(
            global,
            &["Crypto", "SubtleCrypto", "CryptoKey"],
            false,
            initialize,
        )?;
    }
    let getter = context.host_function(crate::host::shared_vm::operation("get crypto"))?;
    let descriptor = context.null_object_rooted()?;
    set(context, descriptor, "get", getter)?;
    let yes = context.boolean(true);
    set(context, descriptor, "enumerable", yes)?;
    set(context, descriptor, "configurable", yes)?;
    let key = context.string_rooted("crypto");
    context.define_property_descriptor_rooted(global, key, descriptor)?;
    Ok(())
}

pub(crate) fn global_crypto(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = crate::host::shared_vm::commonjs::crypto_module(context)?;
    get(context, module, "webcrypto")
}

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    transform: RootId,
) -> Result<RootId, RootedError> {
    #[cfg(quench_openssl_provider)]
    let (webcrypto, random_values) = {
        let getter =
            context.host_function(crate::host::shared_vm::operation("cryptoWebNamespace"))?;
        let random = crate::host::shared_vm::commonjs::node_library_export(
            context,
            "crypto.js",
            "internal/crypto/random.js",
            "getRandomValues",
        )?;
        (getter, random)
    };
    #[cfg(not(quench_openssl_provider))]
    let (webcrypto, random_values) = (context.undefined(), context.undefined());
    let factory = crate::host::removed_javascript("node:crypto/shared.js")?;
    let hash = context.host_function(crate::host::shared_vm::operation("cryptoHash"))?;
    let hmac = context.host_function(crate::host::shared_vm::operation("cryptoHmac"))?;
    let mac_digest = context.host_function(crate::host::shared_vm::operation("cryptoMacDigest"))?;
    let sign = context.host_function(crate::host::shared_vm::operation("cryptoSign"))?;
    let verify = context.host_function(crate::host::shared_vm::operation("cryptoVerify"))?;
    let random_bytes =
        context.host_function(crate::host::shared_vm::operation("cryptoRandomBytes"))?;
    let pbkdf2 = context.host_function(crate::host::shared_vm::operation("cryptoPbkdf2"))?;
    let scrypt = context.host_function(crate::host::shared_vm::operation("cryptoScrypt"))?;
    let cipher_process =
        context.host_function(crate::host::shared_vm::operation("cryptoCipherProcess"))?;
    let generate_rsa_key_pair = context.host_function(crate::host::shared_vm::operation(
        "cryptoGenerateRsaKeyPair",
    ))?;
    let rsa_crypt = context.host_function(crate::host::shared_vm::operation("cryptoRsaCrypt"))?;
    let set_engine = context.host_function(crate::host::shared_vm::operation("cryptoSetEngine"))?;
    let secret_material =
        context.host_function(crate::host::shared_vm::operation("cryptoSecretMaterial"))?;
    let native_constants = super::crypto_constants::binding(context)?;
    #[cfg(quench_openssl_provider)]
    let provider_binding = super::tls_shared_vm::facade::binding(context)?;
    #[cfg(quench_openssl_provider)]
    let get_macs_native = get(context, provider_binding, "getMacs")?;
    #[cfg(not(quench_openssl_provider))]
    let get_macs_native = context.undefined();
    #[cfg(quench_openssl_provider)]
    let argon2_module = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "crypto.js",
        "internal/crypto/argon2.js",
    )?;
    #[cfg(quench_openssl_provider)]
    let argon2 = get(context, argon2_module, "argon2")?;
    #[cfg(quench_openssl_provider)]
    let argon2_sync = get(context, argon2_module, "argon2Sync")?;
    #[cfg(quench_openssl_provider)]
    let kem_module = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "crypto.js",
        "internal/crypto/kem.js",
    )?;
    #[cfg(quench_openssl_provider)]
    let kem_encapsulate = get(context, kem_module, "encapsulate")?;
    #[cfg(quench_openssl_provider)]
    let kem_decapsulate = get(context, kem_module, "decapsulate")?;
    #[cfg(not(quench_openssl_provider))]
    let (argon2, argon2_sync) = (context.undefined(), context.undefined());
    let capture_brands =
        context.host_function(crate::host::shared_vm::operation("cryptoCaptureKeyBrands"))?;
    let ecdh = context.host_function(crate::host::shared_vm::operation("cryptoEcdh"))?;
    #[cfg(quench_openssl_provider)]
    let diffie_hellman =
        context.host_function(crate::host::shared_vm::operation("cryptoDiffieHellman"))?;
    #[cfg(not(quench_openssl_provider))]
    let diffie_hellman = context.undefined();
    let handle_symbol =
        context.host_function(crate::host::shared_vm::operation("cryptoHandleSymbol"))?;
    let x509_metadata =
        context.host_function(crate::host::shared_vm::operation("cryptoX509Metadata"))?;
    let x509_check_host =
        context.host_function(crate::host::shared_vm::operation("cryptoX509CheckHost"))?;
    let global = context.global_root()?;
    let buffer = get(context, global, "Buffer")?;
    let undefined = context.undefined();
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            hash,
            hmac,
            mac_digest,
            sign,
            verify,
            buffer,
            random_bytes,
            pbkdf2,
            scrypt,
            transform,
            cipher_process,
            generate_rsa_key_pair,
            rsa_crypt,
            ecdh,
            diffie_hellman,
            set_engine,
            capture_brands,
            handle_symbol,
            webcrypto,
            random_values,
            secret_material,
            native_constants,
            get_macs_native,
            argon2,
            argon2_sync,
            x509_metadata,
            x509_check_host,
        ],
    )?;
    #[cfg(quench_openssl_provider)]
    {
        let get_fips = get(context, provider_binding, "getFipsCrypto")?;
        set(context, module, "getFips", get_fips)?;
        let set_fips = get(context, provider_binding, "setFipsCrypto")?;
        set(context, module, "setFips", set_fips)?;
        set(context, module, "encapsulate", kem_encapsulate)?;
        set(context, module, "decapsulate", kem_decapsulate)?;
        super::crypto_hash_source_shared_vm::install(context, module, transform)?;
        let keygen = crate::host::shared_vm::commonjs::node_library_module(
            context,
            "crypto.js",
            "internal/crypto/keygen.js",
        )?;
        for name in ["generateKey", "generateKeySync"] {
            let function = get(context, keygen, name)?;
            set(context, module, name, function)?;
        }
    }
    Ok(module)
}

/// Parse the X509 fields used by Node's TLS helpers. The certificate remains
/// in OpenSSL's representation until its immutable JavaScript projection is
/// constructed, so Node's PEM and DER inputs share the same parser.
pub(crate) fn x509_metadata(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(bytes) = context.bytes_prefix_rooted(input, usize::MAX) else {
        return Err(type_error(context, "X509 input must be a BufferSource")?);
    };
    let cert = openssl::x509::X509::from_pem(&bytes)
        .or_else(|_| openssl::x509::X509::from_der(&bytes))
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let result = context.object_rooted()?;

    fn assign(
        context: &mut NativeContext<'_, NodeHost>,
        object: RootId,
        name: &str,
        value: RootId,
    ) -> Result<(), RootedError> {
        let key = context.string_rooted(name);
        if context.define_data_property_rooted(object, key, value, true, true, true)? {
            Ok(())
        } else {
            Err(RootedError::host("Cannot define X509 metadata property"))
        }
    }

    for (key, value) in [
        (
            "subject",
            super::tls_shared_vm::certificate::multiline_name(cert.subject_name())
                .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?,
        ),
        (
            "issuer",
            super::tls_shared_vm::certificate::multiline_name(cert.issuer_name())
                .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?,
        ),
        ("validFrom", cert.not_before().to_string()),
        ("validTo", cert.not_after().to_string()),
    ] {
        let value = context.string_rooted(&value);
        assign(context, result, key, value)?;
    }
    let serial = cert
        .serial_number()
        .to_bn()
        .and_then(|value| value.to_hex_str())
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let serial = context.string_rooted(&serial);
    assign(context, result, "serialNumber", serial)?;
    for (key, digest) in [
        ("fingerprint", openssl::hash::MessageDigest::sha1()),
        ("fingerprint256", openssl::hash::MessageDigest::sha256()),
        ("fingerprint512", openssl::hash::MessageDigest::sha512()),
    ] {
        let bytes = cert
            .digest(digest)
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        let text = bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":");
        let value = context.string_rooted(&text);
        assign(context, result, key, value)?;
    }
    let der = cert
        .to_der()
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let raw = crate::modules::buffer_shared_vm::from_bytes(context, &der)?;
    assign(context, result, "raw", raw)?;
    let pem = cert
        .to_pem()
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let pem_text = context.string_rooted(&String::from_utf8_lossy(&pem));
    assign(context, result, "pem", pem_text)?;
    let legacy = super::tls_shared_vm::certificate::legacy(context, &cert)?;
    assign(context, result, "legacy", legacy)?;
    let alt_name_key = context.string_rooted("subjectaltname");
    let alt_name = context.get_property_rooted(legacy, alt_name_key)?;
    assign(context, result, "subjectAltName", alt_name)?;
    let info_access_key = context.string_rooted("infoaccess");
    let info_access = context.get_property_rooted(legacy, info_access_key)?;
    assign(context, result, "infoAccess", info_access)?;
    let ca_key = context.string_rooted("ca");
    let ca = context.get_property_rooted(legacy, ca_key)?;
    assign(context, result, "ca", ca)?;
    #[cfg(quench_openssl_provider)]
    {
        let key = cert
            .public_key()
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        let key = super::crypto_key_shared_vm::generated_asymmetric(
            context,
            Arc::new(super::crypto_asymmetric::OwnedKey::Public(key)),
            None,
        )?;
        assign(context, result, "publicKey", key)?;
    }
    #[cfg(not(quench_openssl_provider))]
    {
        let undefined = context.undefined();
        assign(context, result, "publicKey", undefined)?;
    }
    Ok(result)
}

pub(crate) fn x509_check_host(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use foreign_types::ForeignType;
    let bytes = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(bytes) = context.bytes_prefix_rooted(bytes, usize::MAX) else {
        return Err(type_error(context, "X509 input must be a BufferSource")?);
    };
    let cert = openssl::x509::X509::from_pem(&bytes)
        .or_else(|_| openssl::x509::X509::from_der(&bytes))
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let hostname = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let Some(hostname) = context.string_text(hostname)? else {
        return Err(type_error(context, "The \"name\" argument must be of type string")?);
    };
    if hostname.is_empty() || hostname.as_bytes().contains(&0) {
        return Ok(context.undefined());
    }
    let options = args.get(2).copied().unwrap_or_else(|| context.undefined());
    let subject = get(context, options, "subject")?;
    let subject = context.string_text(subject)?.unwrap_or_else(|| "default".to_owned());
    let flags = match subject.as_str() {
        "default" => 0,
        "always" => 0x1,
        "never" => 0x20,
        _ => return Err(type_error(context, "Invalid subject option")?),
    };
    let mut peer_name = std::ptr::null_mut();
    let result = unsafe {
        openssl_sys::X509_check_host(
            cert.as_ptr(),
            hostname.as_ptr().cast(),
            hostname.len(),
            flags,
            &mut peer_name,
        )
    };
    if result == 1 {
        let name = if peer_name.is_null() {
            hostname
        } else {
            let value = unsafe { std::ffi::CStr::from_ptr(peer_name) }
                .to_string_lossy()
                .into_owned();
            unsafe { openssl_sys::OPENSSL_free(peer_name.cast()) };
            value
        };
        Ok(context.string_rooted(&name))
    } else {
        if !peer_name.is_null() {
            unsafe { openssl_sys::OPENSSL_free(peer_name.cast()) };
        }
        Ok(context.undefined())
    }
}

fn private_key_from_pem(
    pem: &[u8],
    passphrase: &[u8],
) -> Result<openssl::pkey::PKey<openssl::pkey::Private>, openssl::error::ErrorStack> {
    if passphrase.is_empty() {
        openssl::pkey::PKey::private_key_from_pem(pem).or_else(|_| {
            openssl::rsa::Rsa::private_key_from_pem(pem).and_then(openssl::pkey::PKey::from_rsa)
        })
    } else {
        openssl::pkey::PKey::private_key_from_pem_passphrase(pem, passphrase).or_else(|_| {
            openssl::rsa::Rsa::private_key_from_pem_passphrase(pem, passphrase)
                .and_then(openssl::pkey::PKey::from_rsa)
        })
    }
}

fn private_key_from_data(
    data: &[u8],
    passphrase: &[u8],
    format: &str,
    key_type: &str,
) -> Result<openssl::pkey::PKey<openssl::pkey::Private>, openssl::error::ErrorStack> {
    if format == "der" {
        if key_type == "pkcs1" {
            openssl::rsa::Rsa::private_key_from_der(data).and_then(openssl::pkey::PKey::from_rsa)
        } else {
            openssl::pkey::PKey::private_key_from_der(data)
        }
    } else {
        private_key_from_pem(data, passphrase)
    }
}

fn public_key_from_data(
    data: &[u8],
    format: &str,
    key_type: &str,
) -> Result<openssl::pkey::PKey<openssl::pkey::Public>, openssl::error::ErrorStack> {
    if format == "der" && key_type == "pkcs1" {
        openssl::rsa::Rsa::public_key_from_der_pkcs1(data).and_then(openssl::pkey::PKey::from_rsa)
    } else if format == "der" {
        openssl::pkey::PKey::public_key_from_der(data)
    } else {
        openssl::pkey::PKey::public_key_from_pem(data).or_else(|_| {
            openssl::rsa::Rsa::public_key_from_pem_pkcs1(data)
                .and_then(openssl::pkey::PKey::from_rsa)
        })
    }
}

pub(crate) fn generate_rsa_key_pair(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let bits = args.first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|value| {
            value.is_finite() && *value >= 512.0 && *value <= 16384.0 && value.fract() == 0.0
        })
        .ok_or_else(|| RootedError::host("RSA modulus length is invalid"))? as u32;
    let exponent = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|value| {
            value.is_finite() && *value >= 3.0 && *value <= u32::MAX as f64 && value.fract() == 0.0
        })
        .unwrap_or(65537.0) as u32;
    let cipher_name = args
        .get(2)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_default();
    let passphrase = args
        .get(3)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let public_exponent = openssl::bn::BigNum::from_u32(exponent)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let rsa = openssl::rsa::Rsa::generate_with_e(bits, &public_exponent)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let public_pkcs1_pem = rsa
        .public_key_to_pem_pkcs1()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let private_pkcs1_pem = rsa
        .private_key_to_pem()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let encrypted_private_pem = if cipher_name.is_empty() {
        private_pkcs1_pem.clone()
    } else {
        let cipher = match cipher_name.to_ascii_lowercase().as_str() {
            "aes-128-cbc" => openssl::symm::Cipher::aes_128_cbc(),
            "aes-128-ecb" => openssl::symm::Cipher::aes_128_ecb(),
            "aes-192-cbc" => openssl::symm::Cipher::aes_192_cbc(),
            "aes-256-cbc" => openssl::symm::Cipher::aes_256_cbc(),
            "aes-256-ecb" => openssl::symm::Cipher::aes_256_ecb(),
            _ => return Err(RootedError::host(format!("Unknown cipher: {cipher_name}"))),
        };
        rsa.private_key_to_pem_passphrase(cipher, &passphrase)
            .map_err(|error| RootedError::host(error.to_string()))?
    };
    let public_pkcs1_der = rsa
        .public_key_to_der_pkcs1()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let private_pkcs1_der = rsa
        .private_key_to_der()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let key =
        openssl::pkey::PKey::from_rsa(rsa).map_err(|error| RootedError::host(error.to_string()))?;
    let private_pem = key
        .private_key_to_pem_pkcs8()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let public_pem = key
        .public_key_to_pem()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let private_der = key
        .private_key_to_der()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let public_der = key
        .public_key_to_der()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let private_values = private_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let public_values = public_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let public_pkcs1_values = public_pkcs1_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let private_pkcs1_values = private_pkcs1_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let public_pkcs1_der_values = public_pkcs1_der
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let private_pkcs1_der_values = private_pkcs1_der
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let encrypted_private_values = encrypted_private_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let private_der_values = private_der
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let public_der_values = public_der
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let private = context.array_rooted(&private_values)?;
    let public = context.array_rooted(&public_values)?;
    let public_pkcs1 = context.array_rooted(&public_pkcs1_values)?;
    let private_pkcs1 = context.array_rooted(&private_pkcs1_values)?;
    let public_pkcs1_der = context.array_rooted(&public_pkcs1_der_values)?;
    let private_pkcs1_der = context.array_rooted(&private_pkcs1_der_values)?;
    let encrypted_private = context.array_rooted(&encrypted_private_values)?;
    let private_der = context.array_rooted(&private_der_values)?;
    let public_der = context.array_rooted(&public_der_values)?;
    context.array_rooted(&[
        private,
        public,
        private_der,
        public_der,
        public_pkcs1,
        private_pkcs1,
        public_pkcs1_der,
        private_pkcs1_der,
        encrypted_private,
    ])
}

pub(crate) fn rsa_crypt(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let decrypt = args.first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let private = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let key_bytes = byte_array(
        context,
        *args
            .get(2)
            .ok_or_else(|| RootedError::host("RSA key is missing"))?,
    )?;
    let input = byte_array(
        context,
        *args
            .get(3)
            .ok_or_else(|| RootedError::host("RSA input is missing"))?,
    )?;
    let passphrase = args
        .get(4)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let format = args
        .get(5)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "pem".to_owned());
    let key_type = args
        .get(6)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "spki".to_owned());
    let padding = args
        .get(7)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .unwrap_or(1.0) as i32;
    let padding = match padding {
        1 => openssl::rsa::Padding::PKCS1,
        4 => openssl::rsa::Padding::PKCS1_OAEP,
        _ => return Err(RootedError::host("Unsupported RSA padding")),
    };
    let output = if decrypt {
        let key = private_key_from_data(&key_bytes, &passphrase, &format, &key_type)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut operation = openssl::encrypt::Decrypter::new(&key)
            .map_err(|error| RootedError::host(error.to_string()))?;
        operation
            .set_rsa_padding(padding)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut output = vec![
            0;
            operation
                .decrypt_len(&input)
                .map_err(|error| RootedError::host(error.to_string()))?
        ];
        let length = operation
            .decrypt(&input, &mut output)
            .map_err(|error| RootedError::host(error.to_string()))?;
        output.truncate(length);
        output
    } else if private {
        let key = private_key_from_data(&key_bytes, &passphrase, &format, &key_type)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut operation = openssl::encrypt::Encrypter::new(&key)
            .map_err(|error| RootedError::host(error.to_string()))?;
        operation
            .set_rsa_padding(padding)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut output = vec![
            0;
            operation
                .encrypt_len(&input)
                .map_err(|error| RootedError::host(error.to_string()))?
        ];
        let length = operation
            .encrypt(&input, &mut output)
            .map_err(|error| RootedError::host(error.to_string()))?;
        output.truncate(length);
        output
    } else {
        let key = match public_key_from_data(&key_bytes, &format, &key_type) {
            Ok(key) => key,
            Err(_) => {
                let private_key =
                    private_key_from_data(&key_bytes, &passphrase, &format, &key_type)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                let public_pem = private_key
                    .public_key_to_pem()
                    .map_err(|error| RootedError::host(error.to_string()))?;
                openssl::pkey::PKey::public_key_from_pem(&public_pem)
                    .map_err(|error| RootedError::host(error.to_string()))?
            }
        };
        let mut operation = openssl::encrypt::Encrypter::new(&key)
            .map_err(|error| RootedError::host(error.to_string()))?;
        operation
            .set_rsa_padding(padding)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut output = vec![
            0;
            operation
                .encrypt_len(&input)
                .map_err(|error| RootedError::host(error.to_string()))?
        ];
        let length = operation
            .encrypt(&input, &mut output)
            .map_err(|error| RootedError::host(error.to_string()))?;
        output.truncate(length);
        output
    };
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn ecdh(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let text_arg = |context: &mut NativeContext<'_, NodeHost>, index: usize, fallback: &str| {
        args.get(index)
            .and_then(|root| context.string_text(*root).ok().flatten())
            .unwrap_or_else(|| fallback.to_owned())
    };
    let action = text_arg(context, 0, "");
    let curve = text_arg(context, 1, "");
    let nid = match curve.to_ascii_lowercase().as_str() {
        "prime256v1" | "secp256r1" => openssl::nid::Nid::X9_62_PRIME256V1,
        "secp256k1" => openssl::nid::Nid::SECP256K1,
        "secp384r1" => openssl::nid::Nid::SECP384R1,
        "secp521r1" => openssl::nid::Nid::SECP521R1,
        "secp224r1" => openssl::nid::Nid::SECP224R1,
        "prime192v1" => openssl::nid::Nid::X9_62_PRIME192V1,
        _ => return Err(RootedError::host(format!("Invalid curve name: {curve}"))),
    };
    let group = openssl::ec::EcGroup::from_curve_name(nid)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let mut ctx =
        openssl::bn::BigNumContext::new().map_err(|error| RootedError::host(error.to_string()))?;
    let format = text_arg(context, 4, "uncompressed");
    let form = match format.as_str() {
        "compressed" => openssl::ec::PointConversionForm::COMPRESSED,
        "hybrid" => openssl::ec::PointConversionForm::HYBRID,
        _ => openssl::ec::PointConversionForm::UNCOMPRESSED,
    };
    let private_bytes = args
        .get(2)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let public_bytes = args
        .get(3)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let point_bytes = |point: &openssl::ec::EcPointRef,
                       group: &openssl::ec::EcGroupRef,
                       ctx: &mut openssl::bn::BigNumContextRef,
                       form| {
        point
            .to_bytes(group, form, ctx)
            .map_err(|error| RootedError::host(error.to_string()))
    };
    let output = match action.as_str() {
        "generate" => {
            let key = openssl::ec::EcKey::generate(&group)
                .map_err(|error| RootedError::host(error.to_string()))?;
            let private = key.private_key().to_vec();
            let public = point_bytes(key.public_key(), &group, &mut ctx, form)?;
            let private = private
                .iter()
                .map(|byte| context.number(f64::from(*byte)))
                .collect::<Vec<_>>();
            let public = public
                .iter()
                .map(|byte| context.number(f64::from(*byte)))
                .collect::<Vec<_>>();
            let private = context.array_rooted(&private)?;
            let public = context.array_rooted(&public)?;
            return context.array_rooted(&[private, public]);
        }
        "private" | "format" | "secret" | "public" => {
            let private = if action != "public" && action != "format" {
                let private = openssl::bn::BigNum::from_slice(&private_bytes)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                let mut order = openssl::bn::BigNum::new()
                    .map_err(|error| RootedError::host(error.to_string()))?;
                group
                    .order(&mut order, &mut ctx)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                if private.num_bits() == 0 || private.ucmp(&order) != std::cmp::Ordering::Less {
                    return Err(RootedError::host(
                        "Private key is not valid for specified curve",
                    ));
                }
                Some(private)
            } else {
                None
            };
            let public = if action == "public" || action == "format" || action == "secret" {
                let point = openssl::ec::EcPoint::from_bytes(&group, &public_bytes, &mut ctx)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                if !point
                    .is_on_curve(&group, &mut ctx)
                    .map_err(|error| RootedError::host(error.to_string()))?
                {
                    return Err(RootedError::host(
                        "Public key is not valid for specified curve",
                    ));
                }
                Some(point)
            } else {
                None
            };
            if action == "secret" {
                let private = private.as_ref().expect("private scalar parsed");
                let public = public.as_ref().expect("public point parsed");
                let private_point = {
                    let mut point = openssl::ec::EcPoint::new(&group)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    point
                        .mul_generator2(&group, private, &mut ctx)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    point
                };
                let private_key =
                    openssl::ec::EcKey::from_private_components(&group, private, &private_point)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                let public_key = openssl::ec::EcKey::from_public_key(&group, public)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                let private_key = openssl::pkey::PKey::from_ec_key(private_key)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                let public_key = openssl::pkey::PKey::from_ec_key(public_key)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                let mut deriver = openssl::derive::Deriver::new(&private_key)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                deriver
                    .set_peer(&public_key)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                deriver
                    .derive_to_vec()
                    .map_err(|error| RootedError::host(error.to_string()))?
            } else {
                let point = if let Some(private) = private.as_ref() {
                    let mut point = openssl::ec::EcPoint::new(&group)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    point
                        .mul_generator2(&group, private, &mut ctx)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    point
                } else {
                    public.expect("public point parsed")
                };
                if action == "format" || action == "public" {
                    point_bytes(&point, &group, &mut ctx, form)?
                } else {
                    let private = private.expect("private scalar parsed");
                    let key = openssl::ec::EcKey::from_private_components(&group, &private, &point)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    key.check_key()
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    let private = key.private_key().to_vec();
                    let public = point_bytes(key.public_key(), &group, &mut ctx, form)?;
                    let private = private
                        .iter()
                        .map(|byte| context.number(f64::from(*byte)))
                        .collect::<Vec<_>>();
                    let public = public
                        .iter()
                        .map(|byte| context.number(f64::from(*byte)))
                        .collect::<Vec<_>>();
                    let private = context.array_rooted(&private)?;
                    let public = context.array_rooted(&public)?;
                    return context.array_rooted(&[private, public]);
                }
            }
        }
        _ => {
            return Err(RootedError::host(format!(
                "Unknown ECDH operation: {action}"
            )));
        }
    };
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn random_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(size) = args
        .first()
        .copied()
        .and_then(|size| context.rooted_value(size))
        .and_then(|value| value.as_number())
    else {
        return Err(type_error(
            context,
            "The \"size\" argument must be of type number",
        )?);
    };
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 || size > 16_777_216.0 {
        return Err(type_error(
            context,
            "The \"size\" argument must be a non-negative integer",
        )?);
    }
    let mut bytes = vec![0_u8; size as usize];
    openssl::rand::rand_bytes(&mut bytes)
        .map_err(|_| RootedError::host("crypto random generation failed"))?;
    let values = bytes
        .into_iter()
        .map(|byte| context.number(f64::from(byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn pbkdf2(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let password = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("crypto PBKDF2 password is missing"))?;
    let salt = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto PBKDF2 salt is missing"))?;
    let iterations = args
        .get(2)
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= 1.0 && value.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto PBKDF2 iterations are invalid"))?
        as usize;
    let key_length = args
        .get(3)
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto PBKDF2 key length is invalid"))?
        as usize;
    let digest_name = args
        .get(4)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto PBKDF2 digest is not a string"))?;
    let digest = openssl::hash::MessageDigest::from_name(&digest_name)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    let password = byte_array(context, password)?;
    let salt = byte_array(context, salt)?;
    let mut output = vec![0; key_length];
    openssl::pkcs5::pbkdf2_hmac(&password, &salt, iterations, digest, &mut output)
        .map_err(|error| RootedError::host(format!("crypto PBKDF2 failed: {error}")))?;
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn scrypt(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let password = byte_array(
        context,
        *args
            .first()
            .ok_or_else(|| RootedError::host("scrypt password missing"))?,
    )?;
    let salt = byte_array(
        context,
        *args
            .get(1)
            .ok_or_else(|| RootedError::host("scrypt salt missing"))?,
    )?;
    let number = |index: usize, name: &str| -> Result<u64, RootedError> {
        args.get(index)
            .and_then(|root| context.rooted_value(*root))
            .and_then(|value| value.as_number())
            .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
            .map(|value| value as u64)
            .ok_or_else(|| RootedError::host(format!("scrypt {name} is invalid")))
    };
    let n = number(2, "N")?;
    let r = number(3, "r")?;
    let p = number(4, "p")?;
    let maxmem = number(5, "maxmem")?;
    let key_length = usize::try_from(number(6, "key length")?)
        .map_err(|_| RootedError::host("scrypt key length is invalid"))?;
    let mut output = vec![0; key_length];
    openssl::pkcs5::scrypt(&password, &salt, n, r, p, maxmem, &mut output)
        .map_err(|error| RootedError::host(format!("Invalid scrypt params: {error}")))?;
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn cipher_process(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto cipher algorithm is not a string"))?;
    let cipher = match algorithm.to_ascii_lowercase().as_str() {
        "aes-128-cbc" => openssl::symm::Cipher::aes_128_cbc(),
        "aes-128-ecb" => openssl::symm::Cipher::aes_128_ecb(),
        "aes-256-ecb" => openssl::symm::Cipher::aes_256_ecb(),
        "aes-128-gcm" => openssl::symm::Cipher::aes_128_gcm(),
        "aes-128-ocb" => openssl::symm::Cipher::aes_128_ocb(),
        "aes-192-gcm" => openssl::symm::Cipher::aes_192_gcm(),
        "aes-192-ocb" => openssl::symm::Cipher::aes_192_ocb(),
        "aes-256-cbc" => openssl::symm::Cipher::aes_256_cbc(),
        "aes-256-gcm" => openssl::symm::Cipher::aes_256_gcm(),
        "aes-256-ocb" => openssl::symm::Cipher::aes_256_ocb(),
        "chacha20-poly1305" => openssl::symm::Cipher::chacha20_poly1305(),
        "des-ede3-cbc" => openssl::symm::Cipher::des_ede3_cbc(),
        _ => {
            return Err(RootedError::host(format!("Unknown cipher: {algorithm}")));
        }
    };
    let key = byte_array(
        context,
        *args
            .get(1)
            .ok_or_else(|| RootedError::host("cipher key is missing"))?,
    )?;
    let iv = byte_array(
        context,
        *args
            .get(2)
            .ok_or_else(|| RootedError::host("cipher IV is missing"))?,
    )?;
    let input = byte_array(
        context,
        *args
            .get(3)
            .ok_or_else(|| RootedError::host("cipher input is missing"))?,
    )?;
    let final_block = args
        .get(4)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let decrypt = args
        .get(5)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let aad = args
        .get(6)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let auth_tag = args
        .get(7)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let auth_tag_length = args
        .get(8)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .unwrap_or(16.0) as usize;
    let auto_padding = args
        .get(9)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let is_gcm = algorithm.to_ascii_lowercase().ends_with("-gcm");
    let is_ocb = algorithm.to_ascii_lowercase().ends_with("-ocb");
    let is_aead = is_gcm || is_ocb || algorithm.eq_ignore_ascii_case("chacha20-poly1305");
    let mode = if decrypt {
        openssl::symm::Mode::Decrypt
    } else {
        openssl::symm::Mode::Encrypt
    };
    let iv = if cipher.iv_len() == Some(0) {
        None
    } else {
        Some(iv.as_slice())
    };
    let mut crypter = openssl::symm::Crypter::new(cipher, mode, &key, iv)
        .map_err(|error| RootedError::host(error.to_string()))?;
    crypter.pad(auto_padding);
    // OCB requires its configured tag length (encryption) or tag value
    // (decryption) before processing AAD or payload. GCM accepts its tag at
    // finalization, so retain that existing path below.
    if is_ocb {
        // Node configures the expected tag length during cipher initialization,
        // before a decipher receives the tag value. OpenSSL's provider rejects
        // a shortened tag unless that length is established first.
        crypter
            .set_tag_len(auth_tag_length)
            .map_err(|error| RootedError::host(error.to_string()))?;
        if decrypt && !auth_tag.is_empty() {
            crypter
                .set_tag(&auth_tag)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    if is_aead
        && !aad.is_empty() {
            crypter
                .aad_update(&aad)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
    let mut output = vec![0; input.len() + cipher.block_size()];
    let mut written = crypter
        .update(&input, &mut output)
        .map_err(|error| RootedError::host(error.to_string()))?;
    if final_block {
        if is_aead && decrypt && !is_ocb {
            crypter
                .set_tag(&auth_tag)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
        written += crypter
            .finalize(&mut output[written..])
            .map_err(|error| RootedError::host(error.to_string()))?;
    }
    output.truncate(written);
    let output_values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let output_array = context.array_rooted(&output_values)?;
    let tag_array = if is_aead && final_block && !decrypt {
        let mut tag = vec![0; auth_tag_length];
        crypter
            .get_tag(&mut tag)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let values = tag
            .iter()
            .map(|byte| context.number(f64::from(*byte)))
            .collect::<Vec<_>>();
        context.array_rooted(&values)?
    } else {
        context.array_rooted(&[])?
    };
    let result = context.object_rooted()?;
    set(context, result, "output", output_array)?;
    set(context, result, "tag", tag_array)?;
    Ok(result)
}

#[cfg(not(quench_openssl_provider))]
pub(crate) fn hash(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto hash algorithm is not a string"))?;
    let Some(input) = args.get(1).copied() else {
        return Err(type_error(
            context,
            "The hash input must be an array of bytes",
        )?);
    };
    let bytes = byte_array(context, input)?;
    let output_length = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .unwrap_or(0.0) as usize;
    let digest = match algorithm.to_ascii_lowercase().as_str() {
        "shake128" | "shake256" => {
            let algorithm = if algorithm.eq_ignore_ascii_case("shake128") {
                openssl::hash::MessageDigest::shake_128()
            } else {
                openssl::hash::MessageDigest::shake_256()
            };
            let mut hasher = openssl::hash::Hasher::new(algorithm)
                .map_err(|_| RootedError::host("crypto digest initialization failed"))?;
            hasher
                .update(&bytes)
                .map_err(|_| RootedError::host("crypto digest update failed"))?;
            let mut output = vec![0; output_length];
            hasher
                .finish_xof(&mut output)
                .map_err(|_| RootedError::host("crypto digest failed"))?;
            output
        }
        "sha1" => crate::modules::crypto_sha1::digest(&bytes),
        _ => {
            let algorithm = openssl::hash::MessageDigest::from_name(&algorithm)
                .ok_or_else(|| RootedError::host("Digest method not supported"))?;
            openssl::hash::hash(algorithm, &bytes)
                .map_err(|_| RootedError::host("crypto digest failed"))?
                .to_vec()
        }
    };
    let values = digest
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

#[cfg(not(quench_openssl_provider))]
pub(crate) fn hmac(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto HMAC algorithm is not a string"))?;
    let key_root = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto HMAC key is missing"))?;
    let input_root = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("crypto HMAC input is missing"))?;
    let key = byte_array(context, key_root)?;
    let input = byte_array(context, input_root)?;
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    // OpenSSL rejects a zero-length HMAC key. A single zero byte has the same
    // block-padded HMAC key representation as an empty key.
    let openssl_key = if key.is_empty() { vec![0] } else { key };
    let key = openssl::pkey::PKey::hmac(&openssl_key).map_err(|error| {
        RootedError::host(format!(
            "crypto HMAC key is invalid ({} bytes): {error}",
            openssl_key.len()
        ))
    })?;
    let mut signer = openssl::sign::Signer::new(digest, &key).map_err(|error| {
        RootedError::host(format!("crypto HMAC initialization failed: {error}"))
    })?;
    signer
        .update(&input)
        .map_err(|_| RootedError::host("crypto HMAC update failed"))?;
    let output = signer
        .sign_to_vec()
        .map_err(|_| RootedError::host("crypto HMAC failed"))?;
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn verify(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto verification algorithm is not a string"))?;
    let input = byte_array(
        context,
        *args
            .get(1)
            .ok_or_else(|| RootedError::host("crypto verification input is missing"))?,
    )?;
    let key_bytes = byte_array(
        context,
        *args
            .get(2)
            .ok_or_else(|| RootedError::host("crypto public key is missing"))?,
    )?;
    let signature = byte_array(
        context,
        *args
            .get(3)
            .ok_or_else(|| RootedError::host("crypto signature is missing"))?,
    )?;
    let passphrase = args
        .get(4)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let format = args
        .get(5)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "pem".to_owned());
    let key_type = args
        .get(6)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "spki".to_owned());
    let key = match public_key_from_data(&key_bytes, &format, &key_type) {
        Ok(key) => key,
        Err(_) => {
            let private_key = private_key_from_data(&key_bytes, &passphrase, &format, &key_type);
            if let Ok(private) = private_key {
                let public = private
                    .public_key_to_pem()
                    .map_err(|error| RootedError::host(error.to_string()))?;
                openssl::pkey::PKey::public_key_from_pem(&public)
                    .map_err(|error| RootedError::host(error.to_string()))?
            } else {
                openssl::x509::X509::from_pem(&key_bytes)
                    .and_then(|certificate| certificate.public_key())
                    .map_err(|error| RootedError::host(error.to_string()))?
            }
        }
    };
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    let mut verifier = openssl::sign::Verifier::new(digest, &key)
        .map_err(|error| RootedError::host(error.to_string()))?;
    verifier
        .update(&input)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let result = verifier
        .verify(&signature)
        .map_err(|error| RootedError::host(error.to_string()))?;
    Ok(context.boolean(result))
}

pub(crate) fn sign(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto signature algorithm is not a string"))?;
    let input_root = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto signature input is missing"))?;
    let key_root = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("crypto private key is missing"))?;
    let input = byte_array(context, input_root)?;
    let key_bytes = byte_array(context, key_root)?;
    let passphrase = args
        .get(3)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let format = args
        .get(4)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "pem".to_owned());
    let key_type = args
        .get(5)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "pkcs8".to_owned());
    let key = private_key_from_data(&key_bytes, &passphrase, &format, &key_type)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    let mut signer = openssl::sign::Signer::new(digest, &key)
        .map_err(|error| RootedError::host(error.to_string()))?;
    signer
        .update(&input)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let output = signer
        .sign_to_vec()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

fn byte_array(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
) -> Result<Vec<u8>, RootedError> {
    if let Some(bytes) = context.view_bytes_rooted(root) {
        return Ok(bytes);
    }
    let length = get(context, root, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto byte input is not array-like"))?
        as usize;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let value = get(context, root, &index.to_string())?;
        let byte = context
            .rooted_value(value)
            .and_then(|value| value.as_number())
            .filter(|byte| byte.is_finite() && *byte >= 0.0 && *byte <= f64::from(u8::MAX))
            .ok_or_else(|| RootedError::host("crypto byte input contained an invalid value"))?;
        bytes.push(byte as u8);
    }
    Ok(bytes)
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install crypto property {name}"
        )))
    }
}

fn type_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(message)?;
    Ok(context.throw(error))
}

/// Native hashes use the same private symbol captured by the original Node utility.
/// Resolve on first hash construction, after the crypto module has entered the cache.
pub(crate) fn handle_symbol(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    if let Some(symbol) = state.borrow().crypto_handle_symbol {
        return Ok(symbol);
    }
    crate::host::shared_vm::commonjs::node_library_export(
        context,
        "crypto.js",
        "internal/crypto/util.js",
        "kHandle",
    )?;
    let symbol = state.borrow().crypto_handle_symbol;
    symbol.ok_or_else(|| RootedError::host("Node crypto handle symbol capture missing"))
}

#[cfg(quench_openssl_provider)]
pub(crate) fn hash(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::crypto_digest_error::{self as errors, Operation};
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto hash algorithm is not a string"))?;
    let input = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto hash input is missing"))?;
    let bytes = byte_array(context, input)?;
    let length = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .filter(|value| !value.is_undefined())
        .map(|value| {
            value
                .as_number()
                .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0)
                .map(|n| n as usize)
                .ok_or_else(|| RootedError::host("Invalid digest output length"))
        })
        .transpose()?;
    let digest = super::crypto_digest::Digest::fetch(&algorithm)
        .map_err(|error| errors::throw(context, Operation::Hash, error))?;
    let output = digest
        .hash(&bytes, length)
        .map_err(|error| errors::throw(context, Operation::Hash, error))?;
    let roots = output
        .into_iter()
        .map(|byte| context.number(f64::from(byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&roots)
}

pub(crate) fn mac_digest(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let key = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("crypto MAC key is missing"))?;
    let input = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto MAC input is missing"))?;
    let cipher = args
        .get(2)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_default();
    let algorithm = args
        .get(3)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "cmac".to_string());
    let key = byte_array(context, key)?;
    let input = byte_array(context, input)?;
    #[cfg(quench_openssl_provider)]
    let output = match algorithm.as_str() {
        "cmac" => super::crypto_secret_job_shared_vm::kmac_request::cmac(&key, &cipher, &input)
            .map_err(|error| RootedError::host(format!("CMAC operation failed: {error}")))?,
        "poly1305" => {
            use openssl::pkey::{Id, PKey};
            use openssl::sign::Signer;
            let pkey = PKey::private_key_from_raw_bytes(&key, Id::POLY1305).map_err(|error| {
                RootedError::host(format!("Poly1305 key initialization failed: {error}"))
            })?;
            let mut signer = Signer::new_without_digest(&pkey).map_err(|error| {
                RootedError::host(format!("Poly1305 initialization failed: {error}"))
            })?;
            signer
                .update(&input)
                .map_err(|error| RootedError::host(format!("Poly1305 update failed: {error}")))?;
            signer.sign_to_vec().map_err(|error| {
                RootedError::host(format!("Poly1305 finalization failed: {error}"))
            })?
        }
        _ => return Err(RootedError::host("unsupported crypto MAC algorithm")),
    };
    #[cfg(not(quench_openssl_provider))]
    let output: Vec<u8> = {
        let _ = (key, cipher, input, algorithm);
        return Err(RootedError::host(
            "CMAC is unavailable without OpenSSL provider",
        ));
    };
    let roots = output
        .into_iter()
        .map(|byte| context.number(f64::from(byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&roots)
}
#[cfg(quench_openssl_provider)]
pub(crate) fn hmac(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::crypto_digest_error::{self as errors, Operation};
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto HMAC algorithm is not a string"))?;
    let key = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto HMAC key is missing"))?;
    let input = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("crypto HMAC input is missing"))?;
    let key = byte_array(context, key)?;
    let input = byte_array(context, input)?;
    let operation = Operation::Hmac(&algorithm);
    let digest = super::crypto_digest::Digest::fetch(&algorithm)
        .map_err(|error| errors::throw(context, operation, error))?;
    let output = digest
        .hmac(&key, &input)
        .map_err(|error| errors::throw(context, operation, error))?;
    let roots = output
        .into_iter()
        .map(|byte| context.number(f64::from(byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&roots)
}
