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
fn incompatible_raw_format(c: &mut Context<'_>) -> RootedError {
    match c.error_rooted("The selected key encoding is incompatible with the key type") {
        Ok(error) => {
            let name = c.string_rooted("code");
            let value = c.string_rooted("ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS");
            match c.define_data_property_rooted(error, name, value, true, true, true) {
                Ok(_) => c.throw(error),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
}
fn decoder_unsupported(c: &mut Context<'_>) -> RootedError {
    match c.error_rooted("error:1E08010C:DECODER routines::unsupported") {
        Ok(error) => {
            let key = c.string_rooted("library");
            let value = c.string_rooted("DECODER routines");
            match c.define_data_property_rooted(error, key, value, true, true, true) {
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
        KeyError::UnsupportedPrivatePkcs1 => decoder_unsupported(c),
        KeyError::InvalidRawKey => invalid(c, "ERR_INVALID_ARG_VALUE", "Invalid key data"),
        KeyError::InvalidCurve(name) => invalid(
            c,
            "ERR_CRYPTO_INVALID_CURVE",
            &format!("Invalid EC curve: {name}"),
        ),
        KeyError::MissingPassphrase => invalid(
            c,
            "ERR_MISSING_PASSPHRASE",
            "Passphrase required for encrypted key",
        ),
        KeyError::UnsupportedJwkCurve(curve) => {
            let message = format!("Unsupported JWK EC curve: {curve}.");
            match c.error_rooted(&message) {
                Ok(error) => {
                    let name = c.string_rooted("code");
                    let value = c.string_rooted("ERR_CRYPTO_JWK_UNSUPPORTED_CURVE");
                    match c.define_data_property_rooted(error, name, value, true, true, true) {
                        Ok(_) => c.throw(error),
                        Err(error) => error,
                    }
                }
                Err(error) => error,
            }
        }
        KeyError::UnsupportedJwkKeyType => match c.error_rooted("Unsupported JWK Key Type.") {
            Ok(error) => {
                let name = c.string_rooted("code");
                let value = c.string_rooted("ERR_CRYPTO_JWK_UNSUPPORTED_KEY_TYPE");
                match c.define_data_property_rooted(error, name, value, true, true, true) {
                    Ok(_) => c.throw(error),
                    Err(error) => error,
                }
            }
            Err(error) => error,
        },
        KeyError::UnsupportedJwkAlgorithm => match c.error_rooted("Unsupported JWK AKP \"alg\"") {
            Ok(error) => {
                let name = c.string_rooted("code");
                let value = c.string_rooted("ERR_CRYPTO_INVALID_JWK");
                match c.define_data_property_rooted(error, name, value, true, true, true) {
                    Ok(_) => c.throw(error),
                    Err(error) => error,
                }
            }
            Err(error) => error,
        },
        KeyError::MissingKeySeed => match c.error_rooted("key does not have an available seed") {
            Ok(error) => {
                let name = c.string_rooted("code");
                let value = c.string_rooted("ERR_CRYPTO_OPERATION_FAILED");
                match c.define_data_property_rooted(error, name, value, true, true, true) {
                    Ok(_) => c.throw(error),
                    Err(error) => error,
                }
            }
            Err(error) => error,
        },
        KeyError::InvalidJwk => invalid(c, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK key"),
        // Node distinguishes a cipher name that cannot be resolved from
        // incompatible options for a cipher it did resolve. Keep the provider
        // lookup error intact through key serialization.
        KeyError::Unsupported("unknown cipher") => {
            invalid(c, "ERR_CRYPTO_UNKNOWN_CIPHER", "Unknown cipher")
        }
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
    let input_format = number(c, arg(2))?;
    if input_format == super::crypto_key_layout::Format::Store as u32 {
        // Node's prepareAsymmetricKey() turns private-key URLs into a STORE
        // record: { uri, properties }. File URLs have already been converted
        // to paths by fileURLToPath(), while provider URIs remain strings.
        // The OpenSSL STORE API is not exposed by openssl-rs, so support the
        // common file-backed case here and feed its PEM bytes through the same
        // key importer as ordinary private key material.
        if kind != KeyKind::Private {
            return Err(invalid(
                c,
                "ERR_INVALID_ARG_TYPE",
                "Private key URL cannot be used as a public key",
            ));
        }
        let record = arg(1);
        let uri = if let Some(uri) = c.string_text(record)? {
            uri
        } else {
            let key = c.string_rooted("uri");
            let uri = c.get_property_rooted(record, key)?;
            c.string_text(uri)?
                .ok_or_else(|| invalid(c, "ERR_INVALID_ARG_TYPE", "Key URL must be a string"))?
        };
        let path = std::path::PathBuf::from(&uri);
        let path = if path.is_absolute() {
            Some(path)
        } else {
            url::Url::parse(&uri)
                .ok()
                .and_then(|url| url.to_file_path().ok())
        };
        let Some(path) = path else {
            return Err(store_unsupported(c));
        };
        let data =
            std::fs::read(path).map_err(|error| store_operation_failed(c, &error.to_string()))?;
        let passphrase = optional_bytes(c, arg(4));
        // Node reports an explicit missing-passphrase error for encrypted
        // file-backed keys. OpenSSL 3's decoder otherwise returns its generic
        // interrupted/cancelled stack before the STORE boundary can expose
        // that more useful key-loading condition.
        let encrypted_pem = data
            .windows(b"-----BEGIN ENCRYPTED PRIVATE KEY-----".len())
            .any(|window| window == b"-----BEGIN ENCRYPTED PRIVATE KEY-----")
            || data
                .windows(b"Proc-Type: 4,ENCRYPTED".len())
                .any(|window| window == b"Proc-Type: 4,ENCRYPTED");
        if encrypted_pem && passphrase.is_none() {
            return Err(native_error(c, KeyError::MissingPassphrase));
        }
        let owned = OwnedKey::import(kind, &data, Format::Pem, None, passphrase.as_deref(), None)
            .map_err(|error| match error {
            error @ KeyError::MissingPassphrase => native_error(c, error),
            KeyError::OpenSsl(_) if passphrase.is_none() => {
                store_operation_failed(c, "Unable to load private key from store URI")
            }
            error => native_error(c, error),
        })?;
        return Ok(Arc::new(owned));
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
        const RAW_KEY_TYPES: &[&str] = &[
            "rsa",
            "rsa-pss",
            "dsa",
            "dh",
            "ec",
            "ed25519",
            "ed448",
            "x25519",
            "x448",
            "ml-dsa-44",
            "ml-dsa-65",
            "ml-dsa-87",
            "ml-kem-512",
            "ml-kem-768",
            "ml-kem-1024",
            "slh-dsa-sha2-128s",
            "slh-dsa-sha2-128f",
            "slh-dsa-shake-128s",
            "slh-dsa-shake-128f",
            "slh-dsa-sha2-192s",
            "slh-dsa-sha2-192f",
            "slh-dsa-shake-192s",
            "slh-dsa-shake-192f",
            "slh-dsa-sha2-256s",
            "slh-dsa-sha2-256f",
            "slh-dsa-shake-256s",
            "slh-dsa-shake-256f",
        ];
        if !RAW_KEY_TYPES.contains(&algorithm.as_str()) {
            let received = algorithm.split('\0').next().unwrap_or(&algorithm);
            return Err(invalid(
                c,
                "ERR_INVALID_ARG_VALUE",
                &format!("Invalid asymmetricKeyType: {received}"),
            ));
        }
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

/// Synchronously derive a shared secret from the prepared private/public
/// KeyObjects used by node:crypto's diffieHellman() API.
pub(crate) fn diffie_hellman(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    let private_object = args.first().copied().unwrap_or(missing);
    let public_object = args.get(1).copied().unwrap_or(missing);
    if let Some(kind) = super::crypto_key_shared_vm::key_object_type(c, private_object)? {
        if kind != "private" {
            return Err(invalid(
                c,
                "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
                &format!("Invalid key object type {kind}, expected private."),
            ));
        }
    }
    if let Some(kind) = super::crypto_key_shared_vm::key_object_type(c, public_object)? {
        if !matches!(kind, "private" | "public") {
            return Err(invalid(
                c,
                "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
                &format!("Invalid key object type {kind}, expected private or public."),
            ));
        }
    }
    let private_input = super::crypto_key_shared_vm::key_object_handle(c, private_object)?
        .unwrap_or(private_object);
    let public_input =
        super::crypto_key_shared_vm::key_object_handle(c, public_object)?.unwrap_or(public_object);
    let private_kind = c.number(2.0);
    let public_kind = c.number(1.0);
    let undefined = c.undefined();
    let private = import_input(
        c,
        KeyKind::Private,
        &[private_kind, private_input, undefined],
    )?;
    let public = import_input(c, KeyKind::Public, &[public_kind, public_input, undefined])?;
    let private_type = private.key_type();
    let public_type = public.key_type();
    let supported =
        |key_type: Option<&str>| matches!(key_type, Some("dh" | "ec" | "x25519" | "x448"));
    if private_type != public_type || !supported(private_type) {
        let error = c.error_rooted("Incompatible key types for Diffie-Hellman")?;
        let key = c.string_rooted("code");
        let value = c.string_rooted("ERR_CRYPTO_INCOMPATIBLE_KEY");
        c.define_data_property_rooted(error, key, value, true, true, true)?;
        return Err(c.throw(error));
    }
    let private_key = private.private_ref().ok_or_else(|| {
        invalid(
            c,
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Private key required",
        )
    })?;
    let mut derive = openssl::derive::Deriver::new(private_key)
        .map_err(|error| super::crypto_error_shared_vm::decorated(c, error))?;
    derive
        .set_peer(public.public_ref())
        .map_err(|error| super::crypto_error_shared_vm::decorated(c, error))?;
    let secret = derive
        .derive_to_vec()
        .map_err(|error| super::crypto_error_shared_vm::decorated(c, error))?;
    super::buffer_shared_vm::from_bytes(c, &secret)
}

fn store_operation_failed(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.error_rooted(message) {
        Ok(error) => {
            let key = c.string_rooted("code");
            let value = c.string_rooted("ERR_CRYPTO_OPERATION_FAILED");
            match c.define_data_property_rooted(error, key, value, true, true, true) {
                Ok(_) => c.throw(error),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
}

fn store_unsupported(c: &mut Context<'_>) -> RootedError {
    match c.error_rooted("No store loader is available for this key URI") {
        Ok(error) => {
            let key = c.string_rooted("code");
            let value = c.string_rooted("ERR_OSSL_OSSL_STORE_UNSUPPORTED");
            match c.define_data_property_rooted(error, key, value, true, true, true) {
                Ok(_) => c.throw(error),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
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

/// Export an EC private key in WebCrypto's PKCS#8 form. Node embeds the public
/// point in the RFC 5915 ECPrivateKey inside PrivateKeyInfo, retaining the
/// outer PKCS#8 version 0. OpenSSL's generic serializer omits that inner
/// `[1] BIT STRING`; add it only for this WebCrypto-specific operation so
/// ordinary KeyObject PKCS#8 exports retain their existing encoding.
pub(crate) fn export_ec_private_pkcs8(
    c: &mut Context<'_>,
    handle: RootId,
) -> Result<RootId, RootedError> {
    let key = key(c, handle)?.ok_or_else(|| {
        invalid(
            c,
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Uninitialized asymmetric key",
        )
    })?;
    if key.key_type_name().as_deref() != Some("ec") {
        let format = c.number(0.0);
        let encoding = c.number(1.0);
        return export(c, handle, &[format, encoding]);
    }

    use super::crypto_asymmetric::{Encoding, Format};
    let private_info = key
        .export(Format::Der, Encoding::Pkcs8, None, None)
        .map_err(|error| native_error(c, error))?;
    let public_point = key
        .raw_public(false)
        .map_err(|error| native_error(c, error))?;

    fn read_der_length(bytes: &[u8], cursor: &mut usize) -> Option<usize> {
        let first = *bytes.get(*cursor)?;
        *cursor += 1;
        if first & 0x80 == 0 {
            return Some(usize::from(first));
        }
        let count = usize::from(first & 0x7f);
        if count == 0 || count > std::mem::size_of::<usize>() {
            return None;
        }
        let start = *cursor;
        let end = start.checked_add(count)?;
        let encoded = bytes.get(start..end)?;
        let mut length = 0usize;
        for byte in encoded {
            length = length.checked_mul(256)?.checked_add(usize::from(*byte))?;
        }
        *cursor = end;
        Some(length)
    }

    fn append_der_length(output: &mut Vec<u8>, length: usize) {
        if length < 128 {
            output.push(length as u8);
            return;
        }
        let bytes = length.to_be_bytes();
        let first = bytes.iter().position(|byte| *byte != 0).unwrap_or(bytes.len() - 1);
        output.push(0x80 | (bytes.len() - first) as u8);
        output.extend_from_slice(&bytes[first..]);
    }

    fn read_tlv<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<(u8, &'a [u8], &'a [u8])> {
        let start = *cursor;
        let tag = *bytes.get(start)?;
        *cursor = start.checked_add(1)?;
        let length = read_der_length(bytes, cursor)?;
        let content_start = *cursor;
        let content_end = content_start.checked_add(length)?;
        let content = bytes.get(content_start..content_end)?;
        *cursor = content_end;
        Some((tag, content, bytes.get(start..content_end)?))
    }

    fn encode_tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(content.len() + 8);
        encoded.push(tag);
        append_der_length(&mut encoded, content.len());
        encoded.extend_from_slice(content);
        encoded
    }

    let fail_der = || RootedError::host("EC PKCS#8 export had an invalid DER structure");
    let mut outer_cursor = 0usize;
    let (outer_tag, outer_content, outer_sequence) =
        read_tlv(&private_info, &mut outer_cursor).ok_or_else(fail_der)?;
    if outer_tag != 0x30 || outer_cursor != private_info.len() {
        return Err(fail_der());
    }

    let mut outer_items = Vec::new();
    let mut content_cursor = 0usize;
    let mut inserted_public = false;
    while content_cursor < outer_content.len() {
        let (tag, content, encoded) =
            read_tlv(outer_content, &mut content_cursor).ok_or_else(fail_der)?;
        if tag != 0x04 || inserted_public {
            outer_items.extend_from_slice(encoded);
            continue;
        }

        // The PrivateKey OCTET STRING contains an ECPrivateKey sequence.
        let mut inner_cursor = 0usize;
        let (inner_tag, inner_content, inner_sequence) =
            read_tlv(content, &mut inner_cursor).ok_or_else(fail_der)?;
        if inner_tag != 0x30 || inner_cursor != content.len() {
            return Err(fail_der());
        }

        let mut inner_items = Vec::new();
        let mut inner_content_cursor = 0usize;
        let mut has_public = false;
        while inner_content_cursor < inner_content.len() {
            let (inner_item_tag, _, encoded) =
                read_tlv(inner_content, &mut inner_content_cursor).ok_or_else(fail_der)?;
            has_public |= inner_item_tag == 0xa1;
            inner_items.extend_from_slice(encoded);
        }
        if !has_public {
            // RFC 5915 [1] is EXPLICIT, and wraps a BIT STRING whose first
            // content octet records zero unused bits.
            let mut bit_string = Vec::with_capacity(public_point.len() + 1);
            bit_string.push(0x00);
            bit_string.extend_from_slice(&public_point);
            let encoded_bit_string = encode_tlv(0x03, &bit_string);
            let encoded_public = encode_tlv(0xa1, &encoded_bit_string);
            inner_items.extend_from_slice(&encoded_public);
        }
        let encoded_inner = encode_tlv(0x30, &inner_items);
        outer_items.extend_from_slice(&encode_tlv(0x04, &encoded_inner));
        inserted_public = true;
        let _ = inner_sequence;
    }
    if !inserted_public {
        return Err(fail_der());
    }

    let _ = outer_sequence;
    let bytes = Material::new(encode_tlv(0x30, &outer_items));
    super::buffer_shared_vm::from_bytes(c, &bytes)
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
        alg: None,
        fields: Default::default(),
        oth: Vec::new(),
    };
    if !matches!(jwk.kty.as_str(), "RSA" | "EC" | "OKP" | "AKP") {
        return Err(invalid(
            c,
            "ERR_CRYPTO_INVALID_JWK",
            &format!("{} is not a supported JWK key type", jwk.kty),
        ));
    }
    if jwk.kty == "AKP" {
        let alg = get(c, object, "alg")?;
        jwk.alg = c.string_text(alg)?;
        if jwk.alg.is_none() {
            return Err(native_error(c, KeyError::UnsupportedJwkAlgorithm));
        }
    } else if jwk.kty != "RSA" {
        let crv = get(c, object, "crv")?;
        jwk.crv = c.string_text(crv)?;
    }
    let fields: &[&str] = match jwk.kty.as_str() {
        "RSA" => &["n", "e", "d", "p", "q", "dp", "dq", "qi"],
        "EC" => &["x", "y", "d"],
        "AKP" => &["pub", "priv"],
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
        } else if matches!(*name, "d" | "pub" | "priv")
            && !c.rooted_value(value).is_some_and(|v| v.is_undefined())
        {
            // A missing private component has its own Node error, but a
            // present component of the wrong type is malformed JWK data.
            return Err(invalid(
                c,
                "ERR_CRYPTO_INVALID_JWK",
                &format!("Invalid JWK {} key", jwk.kty),
            ));
        }
    }
    if jwk.kty == "RSA" {
        let other_primes = get(c, object, "oth")?;
        if !c
            .rooted_value(other_primes)
            .is_some_and(|value| value.is_undefined())
        {
            if !c.is_array_rooted(other_primes)? {
                return Err(invalid(c, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK RSA key"));
            }
            let length_value = get(c, other_primes, "length")?;
            let length = c.to_number(length_value)?;
            if !length.is_finite() || length < 0.0 || length > 8.0 {
                return Err(invalid(c, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK RSA key"));
            }
            for index in 0..length as usize {
                let index = c.number(index as f64);
                let item = c.get_property_rooted(other_primes, index)?;
                if !c.is_object_rooted(item)? || c.is_array_rooted(item)? {
                    return Err(invalid(c, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK RSA key"));
                }
                let mut components = [Vec::new(), Vec::new(), Vec::new()];
                for (slot, name) in components.iter_mut().zip(["r", "d", "t"]) {
                    let value = get(c, item, name)?;
                    let Some(mut text) = c.string_text(value)? else {
                        return Err(invalid(c, "ERR_CRYPTO_INVALID_JWK", "Invalid JWK RSA key"));
                    };
                    *slot = super::buffer_codec::base64_decode(text.as_bytes());
                    super::crypto_secret_material::cleanse(unsafe { text.as_bytes_mut() });
                }
                jwk.oth.push(components);
            }
        }
    }
    if kind == KeyKind::Private
        && !(if jwk.kty == "AKP" {
            jwk.fields.contains_key("priv")
        } else {
            jwk.fields.contains_key("d")
        })
    {
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
            "AKP" => "Invalid JWK AKP key",
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
    if let Some(alg) = &jwk.alg {
        let value = c.string_rooted(alg);
        put(c, target, "alg", value)?;
    }
    // Native Node insertion order is observable through ownKeys and JSON.
    let fields: &[&str] = match jwk.kty.as_str() {
        "RSA" => &["n", "e", "d", "p", "q", "dp", "dq", "qi"],
        "EC" => &["x", "y", "crv", "d"],
        "OKP" => &["crv", "x", "d"],
        "AKP" => &["pub", "priv"],
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
    if !jwk.oth.is_empty() {
        let mut values = Vec::with_capacity(jwk.oth.len());
        for [r, d, t] in &jwk.oth {
            let item = c.object_rooted()?;
            for (name, bytes) in [("r", r), ("d", d), ("t", t)] {
                let mut text = super::buffer_codec::base64_encode(bytes, false, true);
                let value = c.string_rooted(&text);
                super::crypto_secret_material::cleanse(unsafe { text.as_bytes_mut() });
                put(c, item, name, value)?;
            }
            values.push(item);
        }
        let value = c.array_rooted(&values)?;
        put(c, target, "oth", value)?;
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
    if matches!(key.key_type(), Some("rsa" | "rsa-pss" | "dsa" | "dh")) {
        return Err(incompatible_raw_format(c));
    }
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
pub(crate) fn raw_seed(c: &mut Context<'_>, handle: RootId) -> Result<RootId, RootedError> {
    let key = key(c, handle)?.ok_or_else(|| {
        invalid(
            c,
            "ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE",
            "Uninitialized asymmetric key",
        )
    })?;
    if !key
        .key_type_name()
        .is_some_and(|name| name.starts_with("ml-dsa-") || name.starts_with("ml-kem-"))
    {
        return Err(incompatible_raw_format(c));
    }
    let bytes = Material::new(key.raw_private().map_err(|e| native_error(c, e))?);
    super::buffer_shared_vm::from_bytes(c, &bytes)
}
