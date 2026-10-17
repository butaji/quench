//! Canonical native PKey ownership. Weak JS handle identities never own JS roots;
//! the actual EVP_PKEY survives through every independently held Arc request.
use super::{
    crypto_asymmetric::{Encoding, Format, KeyError, KeyKind, OwnedKey},
    crypto_secret_material::Material,
};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::sync::Arc;
type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) use super::crypto_key_material_shared_vm::{
    asymmetric as key, register_asymmetric as register,
};
fn invalid(c: &mut Context<'_>, code: &str, text: &str) -> RootedError {
    match c.type_error_rooted(text) {
        Ok(error) => {
            let name = c.string_rooted("code");
            let value = c.string_rooted(code);
            match c.define_data_property_rooted(error, name, value, true, true, true) {
                Ok(_) => c.throw(error),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
}
fn native_error(c: &mut Context<'_>, error: KeyError) -> RootedError {
    match error {
        KeyError::OpenSsl(stack) => super::crypto_error_shared_vm::decorated(c, stack),
        KeyError::MissingPassphrase => invalid(
            c,
            "ERR_MISSING_PASSPHRASE",
            "Passphrase required for encrypted key",
        ),
        KeyError::InvalidJwk => invalid(c, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK key"),
        KeyError::Unsupported(text) => invalid(c, "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS", text),
    }
}
fn number(c: &mut Context<'_>, value: RootId) -> Result<u32, RootedError> {
    Ok(c.to_number(value)? as u32)
}
fn optional_bytes(c: &mut Context<'_>, value: RootId) -> Option<Material> {
    c.bytes_prefix_rooted(value, usize::MAX).map(Material::new)
}
fn optional_string(c: &mut Context<'_>, value: RootId) -> Result<Option<String>, RootedError> {
    c.string_text(value)
}
fn encoding(value: u32) -> Option<Encoding> {
    super::crypto_key_layout::Encoding::from_native(value).map(|encoding| encoding.provider())
}
fn serialization_format(value: u32) -> Option<Format> {
    match super::crypto_key_layout::Format::from_native(value)? {
        super::crypto_key_layout::Format::Der => Some(Format::Der),
        super::crypto_key_layout::Format::Pem => Some(Format::Pem),
        _ => None,
    }
}
/// Called only after the central KeyObjectHandle receiver and source-side
/// encoding validation. Return type is the actual native KeyType ABI.
pub(crate) fn initialize(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<u32, RootedError> {
    let missing = c.undefined();
    let arg = |n: usize| args.get(n).copied().unwrap_or(missing);
    let kind = match number(c, arg(0))? {
        1 => KeyKind::Public,
        2 => KeyKind::Private,
        _ => {
            return Err(invalid(
                c,
                "ERR_INVALID_ARG_VALUE",
                "Invalid asymmetric key type",
            ));
        }
    };
    let owned = import_input(c, kind, args)?;
    register(c, handle, owned)?;
    Ok(if kind == KeyKind::Public { 1 } else { 2 })
}
/// `args` is the native [targetType,data,format,type,passphrase,namedCurve] ABI.
/// Jobs and key consumers borrow this same parser without allocating a JS handle.
pub(crate) fn import_input(
    c: &mut Context<'_>,
    kind: KeyKind,
    args: &[RootId],
) -> Result<Arc<OwnedKey>, RootedError> {
    let missing = c.undefined();
    let arg = |n: usize| args.get(n).copied().unwrap_or(missing);
    if let Some(old) = key(c, arg(1))? {
        let owned = match (kind, old.kind()) {
            (KeyKind::Public, _) => Arc::new(old.to_public().map_err(|e| native_error(c, e))?),
            (KeyKind::Private, KeyKind::Private) => old,
            _ => {
                return Err(invalid(
                    c,
                    "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
                    "Invalid key object type public, expected private.",
                ));
            }
        };
        return Ok(owned);
    }
    if number(c, arg(2))? == 2 {
        return import_jwk(c, arg(1), kind);
    }
    let bytes = optional_bytes(c, arg(1)).ok_or_else(|| {
        invalid(
            c,
            "ERR_INVALID_ARG_TYPE",
            "Key material must be a BufferSource",
        )
    })?;
    let raw_format = match number(c, arg(2))? {
        3 => Some(super::crypto_asymmetric::RawFormat::Public),
        4 => Some(super::crypto_asymmetric::RawFormat::Private),
        5 => Some(super::crypto_asymmetric::RawFormat::Seed),
        _ => None,
    };
    if let Some(format) = raw_format {
        let algorithm = optional_string(c, arg(3))?
            .ok_or_else(|| invalid(c, "ERR_INVALID_ARG_TYPE", "Raw key type must be a string"))?;
        let named_curve = optional_string(c, arg(5))?;
        let owned = OwnedKey::import_raw(kind, &bytes, format, &algorithm, named_curve.as_deref())
            .map_err(|e| native_error(c, e))?;
        return Ok(Arc::new(owned));
    }
    let format = serialization_format(number(c, arg(2))?)
        .ok_or_else(|| invalid(c, "ERR_INVALID_ARG_VALUE", "Unsupported key format"))?;
    let enc = if c.rooted_value(arg(3)).is_some_and(|v| v.is_undefined()) {
        None
    } else {
        encoding(number(c, arg(3))?)
    };
    let passphrase = optional_bytes(c, arg(4));
    let named_curve = optional_string(c, arg(5))?;
    let owned = OwnedKey::import(
        kind,
        &bytes,
        format,
        enc,
        passphrase.as_deref(),
        named_curve.as_deref(),
    )
    .map_err(|e| native_error(c, e))?;
    Ok(Arc::new(owned))
}
pub(crate) fn export(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let key = key(c, handle)?.ok_or_else(|| {
        invalid(
            c,
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Uninitialized asymmetric key",
        )
    })?;
    let missing = c.undefined();
    let arg = |n: usize| args.get(n).copied().unwrap_or(missing);
    let format = serialization_format(number(c, arg(0))?)
        .ok_or_else(|| invalid(c, "ERR_INVALID_ARG_VALUE", "Unsupported key format"))?;
    let enc = encoding(number(c, arg(1))?)
        .ok_or_else(|| invalid(c, "ERR_INVALID_ARG_VALUE", "Unsupported key encoding"))?;
    let cipher = optional_string(c, arg(2))?;
    let passphrase = optional_bytes(c, arg(3));
    let bytes = Material::new(
        key.export(format, enc, cipher.as_deref(), passphrase.as_deref())
            .map_err(|e| native_error(c, e))?,
    );
    if format == Format::Pem {
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| RootedError::host("Native PEM is not UTF-8"))?;
        Ok(c.string_rooted(text))
    } else {
        super::buffer_shared_vm::from_bytes(c, &bytes)
    }
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let name = c.string_rooted(name);
    c.define_data_property_rooted(object, name, value, true, true, true)?;
    Ok(())
}
pub(crate) fn key_detail(
    c: &mut Context<'_>,
    handle: RootId,
    target: RootId,
) -> Result<RootId, RootedError> {
    let key = key(c, handle)?.ok_or_else(|| {
        invalid(
            c,
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Uninitialized asymmetric key",
        )
    })?;
    if !matches!(
        key.key_type(),
        Some("rsa" | "rsa-pss" | "dsa" | "ec" | "dh")
    ) {
        let error = c.range_error_rooted("Invalid key type")?;
        let name = c.string_rooted("code");
        let value = c.string_rooted("ERR_CRYPTO_INVALID_KEYTYPE");
        c.define_data_property_rooted(error, name, value, true, true, true)?;
        return Err(c.throw(error));
    }
    let details = key.details().map_err(|e| native_error(c, e))?;
    for (name, value) in [
        ("modulusLength", details.modulus_length),
        ("divisorLength", details.divisor_length),
    ] {
        if let Some(value) = value {
            let value = c.number(f64::from(value));
            put(c, target, name, value)?
        }
    }
    if let Some(bytes) = details.public_exponent {
        let value = super::crypto_secret_job_shared_vm::encode_bytes(c, &bytes)?;
        put(c, target, "publicExponent", value)?
    }
    for (name, value) in [
        ("namedCurve", details.named_curve),
        ("hashAlgorithm", details.hash_algorithm),
        ("mgf1HashAlgorithm", details.mgf1_hash_algorithm),
    ] {
        if let Some(value) = value {
            let value = c.string_rooted(&value);
            put(c, target, name, value)?
        }
    }
    if let Some(value) = details.salt_length {
        let value = c.number(f64::from(value));
        put(c, target, "saltLength", value)?
    }
    Ok(target)
}
pub(crate) fn key_type(c: &mut Context<'_>, handle: RootId) -> Result<RootId, RootedError> {
    let key = key(c, handle)?.ok_or_else(|| {
        invalid(
            c,
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Uninitialized asymmetric key",
        )
    })?;
    Ok(match key.key_type_name() {
        Some(value) => c.string_rooted(&value),
        None => c.undefined(),
    })
}
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let name = c.string_rooted(name);
    c.get_property_rooted(object, name)
}
/// JWK strings are transient projections. Decoded private components are owned
/// by Jwk immediately and scrubbed even if a later accessor throws.
fn import_jwk(
    c: &mut Context<'_>,
    object: RootId,
    kind: KeyKind,
) -> Result<Arc<OwnedKey>, RootedError> {
    use super::crypto_asymmetric::Jwk;
    let kty = get(c, object, "kty")?;
    let kty = c
        .string_text(kty)?
        .ok_or_else(|| invalid(c, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK key"))?;
    let mut jwk = Jwk {
        kty,
        crv: None,
        fields: Default::default(),
    };
    if !matches!(jwk.kty.as_str(), "RSA" | "EC" | "OKP") {
        return Err(invalid(
            c,
            "ERR_CRYPTO_INVALID_JWK",
            &format!("{} is not a supported JWK key type", jwk.kty),
        ));
    }
    if jwk.kty != "RSA" {
        let crv = get(c, object, "crv")?;
        jwk.crv = c.string_text(crv)?;
    }
    let fields: &[&str] = match jwk.kty.as_str() {
        "RSA" => &["n", "e", "d", "p", "q", "dp", "dq", "qi"],
        "EC" => &["x", "y", "d"],
        _ => &["x", "d"],
    };
    for name in fields {
        if matches!(*name, "p" | "q" | "dp" | "dq" | "qi") && !jwk.fields.contains_key("d") {
            continue;
        }
        let value = get(c, object, name)?;
        if let Some(mut text) = c.string_text(value)? {
            let bytes = super::buffer_codec::base64_decode(text.as_bytes());
            super::crypto_secret_material::cleanse(unsafe { text.as_bytes_mut() });
            jwk.fields.insert((*name).into(), bytes);
        }
    }
    if kind == KeyKind::Private && !jwk.fields.contains_key("d") {
        return Err(invalid(
            c,
            "ERR_CRYPTO_INVALID_JWK",
            "JWK does not contain private key material",
        ));
    }
    let owned = OwnedKey::import_jwk(kind, &jwk).map_err(|error| {
        let text = match jwk.kty.as_str() {
            "RSA" => "Invalid JWK RSA key",
            "EC" => "Invalid JWK EC key",
            "OKP" => "Invalid JWK OKP key",
            _ => "Invalid JWK key",
        };
        match error {
            KeyError::InvalidJwk => invalid(c, "ERR_CRYPTO_INVALID_JWK", text),
            error => native_error(c, error),
        }
    })?;
    Ok(Arc::new(owned))
}
pub(crate) fn export_jwk(
    c: &mut Context<'_>,
    handle: RootId,
    target: RootId,
    allow_rsa_pss: bool,
) -> Result<RootId, RootedError> {
    let key = key(c, handle)?.ok_or_else(|| {
        invalid(
            c,
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Uninitialized asymmetric key",
        )
    })?;
    export_owned_jwk(c, &key, target, allow_rsa_pss)
}
pub(crate) fn export_owned_jwk(
    c: &mut Context<'_>,
    key: &OwnedKey,
    target: RootId,
    allow_rsa_pss: bool,
) -> Result<RootId, RootedError> {
    let jwk = key
        .export_jwk_with_flags(allow_rsa_pss)
        .map_err(|e| native_error(c, e))?;
    let value = c.string_rooted(&jwk.kty);
    put(c, target, "kty", value)?;
    // Native Node insertion order is observable through ownKeys and JSON.
    let fields: &[&str] = match jwk.kty.as_str() {
        "RSA" => &["n", "e", "d", "p", "q", "dp", "dq", "qi"],
        "EC" => &["x", "y", "crv", "d"],
        "OKP" => &["crv", "x", "d"],
        _ => &[],
    };
    for name in fields {
        if *name == "crv" {
            if let Some(crv) = &jwk.crv {
                let value = c.string_rooted(crv);
                put(c, target, name, value)?
            }
            continue;
        }
        if let Some(bytes) = jwk.fields.get(*name) {
            let mut text = super::buffer_codec::base64_encode(bytes, false, true);
            let value = c.string_rooted(&text);
            super::crypto_secret_material::cleanse(unsafe { text.as_bytes_mut() });
            put(c, target, name, value)?;
        }
    }
    Ok(target)
}
pub(crate) fn raw(
    c: &mut Context<'_>,
    handle: RootId,
    private: bool,
    compressed: bool,
) -> Result<RootId, RootedError> {
    let key = key(c, handle)?.ok_or_else(|| {
        invalid(
            c,
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Uninitialized asymmetric key",
        )
    })?;
    let bytes = Material::new(
        if private {
            key.raw_private()
        } else {
            key.raw_public(compressed)
        }
        .map_err(|e| native_error(c, e))?,
    );
    super::buffer_shared_vm::from_bytes(c, &bytes)
}
