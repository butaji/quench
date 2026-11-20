//! `KeyObject` and its secret, public and private classes; key creation, generation and
//! export in PEM, DER and JWK.

use super::errors::{self, CryptoError};
use super::{arg, define_getters, define_methods, get, is_nullish, is_undefined, put, with_native, Class, Context, Members, Native, Outcome};
use crate::modules::crypto_asymmetric::{Encoding, Format, Jwk, KeyError, KeyKind, OwnedKey};
use crate::modules::crypto_asymmetric_generate::{DhParameters, Generation, Pss};
use crate::modules::crypto_secret_material::Material;
use quench_runtime::{RootId, RootedError};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Key material: a secret's bytes, or an OpenSSL key pair half.
pub(crate) enum Key {
    Secret(Material),
    Asymmetric(OwnedKey),
}

impl Key {
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::Secret(_) => "secret",
            Self::Asymmetric(key) if key.kind() == KeyKind::Public => "public",
            Self::Asymmetric(_) => "private",
        }
    }

    pub(crate) fn asymmetric(&self) -> Option<&OwnedKey> {
        match self {
            Self::Asymmetric(key) => Some(key),
            Self::Secret(_) => None,
        }
    }

    pub(crate) fn secret(&self) -> Option<&[u8]> {
        match self {
            Self::Secret(bytes) => Some(bytes),
            Self::Asymmetric(_) => None,
        }
    }
}

/// The RSA public exponent Node generates with by default (F4).
const DEFAULT_PUBLIC_EXPONENT: f64 = 65_537.0;
/// The AES key sizes `generateKey('aes')` accepts, in bits.
pub(crate) const AES_KEY_BITS: [f64; 3] = [128.0, 192.0, 256.0];
/// The bit range `generateKey('hmac')` accepts.
const HMAC_MIN_BITS: f64 = 8.0;
const HMAC_MAX_BITS: f64 = i32::MAX as f64;
/// The DH generator Node uses when none is given.
const DH_DEFAULT_GENERATOR: f64 = 2.0;

// ---- Instances -----------------------------------------------------------------------

/// A KeyObject over `key`, of the class its type names.
pub(crate) fn key_object(c: &mut Context<'_>, key: Arc<Key>) -> Outcome {
    let class = match key.type_name() {
        "secret" => Class::SecretKeyObject,
        "public" => Class::PublicKeyObject,
        _ => Class::PrivateKeyObject,
    };
    let constructor = super::class(c, class)?;
    let prototype = get(c, constructor, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let symbol = super::intrinsics(c)?.key_type;
    let type_name = c.string_rooted(key.type_name());
    c.define_data_property_rooted(object, symbol, type_name, true, true, true)?;
    super::attach(c, object, Native::Key(key))?;
    Ok(object)
}

/// The key a KeyObject holds.
pub(crate) fn key_object_key(c: &mut Context<'_>, value: RootId) -> Result<Option<Arc<Key>>, RootedError> {
    let key = with_native(c, value, |native| match native {
        Native::Key(key) => Some(key.clone()),
        _ => None,
    })?;
    Ok(key.flatten())
}

/// The key a KeyObject or CryptoKey holds.
pub(crate) fn key_of(c: &mut Context<'_>, value: RootId) -> Result<Option<Arc<Key>>, RootedError> {
    if let Some(key) = key_object_key(c, value)? {
        return Ok(Some(key));
    }
    match super::web::crypto_key_object(c, value)? {
        Some(object) => key_object_key(c, object),
        None => Ok(None),
    }
}

fn this_key(c: &mut Context<'_>, this: RootId) -> Result<Arc<Key>, RootedError> {
    match key_object_key(c, this)? {
        Some(key) => Ok(key),
        None => Err(errors::throw(c, CryptoError::InvalidThis("KeyObject"))),
    }
}

/// The bytes of a secret KeyObject; a non-secret KeyObject is the wrong type.
pub(crate) fn key_object_secret(c: &mut Context<'_>, value: RootId) -> Result<Option<Vec<u8>>, RootedError> {
    let Some(key) = key_object_key(c, value)? else {
        return Ok(None);
    };
    match key.secret() {
        Some(bytes) => Ok(Some(bytes.to_vec())),
        None => Err(errors::throw(c, CryptoError::InvalidKeyObjectType(key.type_name(), "secret"))),
    }
}

/// Node's `prepareSecretKey`: a secret KeyObject or CryptoKey, a string under `encoding`,
/// or bytes.
pub(crate) fn prepare_secret_key(c: &mut Context<'_>, value: RootId, encoding: RootId) -> Result<Vec<u8>, RootedError> {
    if let Some(key) = key_of(c, value)? {
        return match key.secret() {
            Some(bytes) => Ok(bytes.to_vec()),
            None => Err(errors::throw(c, CryptoError::InvalidKeyObjectType(key.type_name(), "secret"))),
        };
    }
    if super::is_string(c, value) {
        return super::string_bytes(c, value, encoding);
    }
    super::buffer_bytes(c, value).ok_or_else(|| errors::invalid_type(c, value, "key", super::accepts::KEY_LIKE))
}

// ---- Parsing key input ---------------------------------------------------------------

/// What a key argument is for (`kCreatePublic`, `kConsumePrivate`, ...).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyUse {
    CreatePublic,
    CreatePrivate,
    ConsumePublic,
    ConsumePrivate,
}

impl KeyUse {
    fn expected(self) -> &'static str {
        match self {
            Self::CreatePrivate => super::accepts::BINARY_LIKE,
            _ => super::accepts::KEY_LIKE,
        }
    }

    fn public(self) -> Option<bool> {
        match self {
            Self::CreatePrivate | Self::ConsumePrivate => Some(false),
            _ => None,
        }
    }

    fn kind(self) -> KeyKind {
        match self {
            Self::CreatePublic | Self::ConsumePublic => KeyKind::Public,
            Self::CreatePrivate | Self::ConsumePrivate => KeyKind::Private,
        }
    }
}

/// A key's encoding options, as `parseKeyEncoding` reads them.
pub(crate) struct KeyEncoding {
    pub(crate) format: KeyFormat,
    pub(crate) encoding: Option<Encoding>,
    pub(crate) cipher: Option<String>,
    pub(crate) passphrase: Option<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyFormat {
    Pem,
    Der,
    Jwk,
}

/// `options.name`, or `options.object.name` for a nested encoding.
fn option_name(name: &str, object: Option<&str>) -> String {
    match object {
        Some(object) => format!("options.{object}.{name}"),
        None => format!("options.{name}"),
    }
}

fn encoding_name(encoding: Encoding) -> &'static str {
    match encoding {
        Encoding::Pkcs1 => "pkcs1",
        Encoding::Pkcs8 => "pkcs8",
        Encoding::Spki => "spki",
        Encoding::Sec1 => "sec1",
    }
}

/// `parseKeyEncoding(options, keyType, isPublic, objName)`; `key_type` is `None` for input.
pub(crate) fn parse_key_encoding(
    c: &mut Context<'_>,
    options: RootId,
    key_type: Option<&str>,
    public: Option<bool>,
    object: Option<&str>,
) -> Result<KeyEncoding, RootedError> {
    super::object_argument(c, options, "options")?;
    let input = key_type.is_none();
    let format_value = get(c, options, "format")?;
    let format = match c.string_text(format_value)?.as_deref() {
        None if is_undefined(c, format_value) && input => KeyFormat::Pem,
        Some("pem") => KeyFormat::Pem,
        Some("der") => KeyFormat::Der,
        Some("jwk") => KeyFormat::Jwk,
        _ => return Err(errors::invalid_value(c, format_value, &option_name("format", object), "is invalid")),
    };
    let required = (!input || format == KeyFormat::Der) && format != KeyFormat::Jwk;
    let type_value = get(c, options, "type")?;
    let type_text = c.string_text(type_value)?;
    let incompatible = |c: &mut Context<'_>, name: &'static str, required: &str, reason: &'static str| {
        if key_type.is_some_and(|key_type| key_type != required) {
            Err(errors::throw(c, CryptoError::IncompatibleKeyOptions(name, reason)))
        } else {
            Ok(())
        }
    };
    let encoding = match type_text.as_deref() {
        None if is_undefined(c, type_value) && !required => None,
        Some("pkcs1") => {
            incompatible(c, "pkcs1", "rsa", "can only be used for RSA keys")?;
            Some(Encoding::Pkcs1)
        }
        Some("spki") if public != Some(false) => Some(Encoding::Spki),
        Some("pkcs8") if public != Some(true) => Some(Encoding::Pkcs8),
        Some("sec1") if public != Some(true) => {
            incompatible(c, "sec1", "ec", "can only be used for EC keys")?;
            Some(Encoding::Sec1)
        }
        _ => return Err(errors::invalid_value(c, type_value, &option_name("type", object), "is invalid")),
    };
    let mut cipher = None;
    let mut passphrase = None;
    if public != Some(true) {
        let cipher_value = get(c, options, "cipher")?;
        let passphrase_value = get(c, options, "passphrase")?;
        let encoding_value = get(c, options, "encoding")?;
        if !input {
            if !is_nullish(c, cipher_value) {
                let Some(name) = c.string_text(cipher_value)? else {
                    return Err(errors::invalid_value(c, cipher_value, &option_name("cipher", object), "is invalid"));
                };
                if format == KeyFormat::Der && matches!(encoding, Some(Encoding::Pkcs1 | Encoding::Sec1)) {
                    let name = encoding.map(encoding_name).unwrap_or_default();
                    return Err(errors::throw(c, CryptoError::IncompatibleKeyOptions(name, "does not support encryption")));
                }
                cipher = Some(name);
            } else if !is_undefined(c, passphrase_value) {
                return Err(errors::invalid_value(c, cipher_value, &option_name("cipher", object), "is invalid"));
            }
        }
        let string_or_buffer = super::is_string(c, passphrase_value) || super::buffer_bytes(c, passphrase_value).is_some();
        let checked = if input { !is_undefined(c, passphrase_value) } else { cipher.is_some() };
        if checked && !string_or_buffer {
            return Err(errors::invalid_value(c, passphrase_value, &option_name("passphrase", object), "is invalid"));
        }
        if !is_undefined(c, passphrase_value) {
            passphrase = Some(super::binary_like(c, passphrase_value, "key.passphrase", encoding_value)?);
        }
    }
    Ok(KeyEncoding { format, encoding, cipher, passphrase })
}

/// The KeyObject handle `prepareAsymmetricKey` takes from a KeyObject or CryptoKey.
fn key_object_for(c: &mut Context<'_>, value: RootId, key: Arc<Key>, key_use: KeyUse) -> Result<Arc<Key>, RootedError> {
    if key_use == KeyUse::CreatePrivate {
        return Err(errors::invalid_type(c, value, "key", super::accepts::BINARY_LIKE));
    }
    let type_name = key.type_name();
    if type_name != "private" {
        if matches!(key_use, KeyUse::ConsumePrivate | KeyUse::CreatePublic) {
            return Err(errors::throw(c, CryptoError::InvalidKeyObjectType(type_name, "private")));
        }
        if type_name != "public" {
            return Err(errors::throw(c, CryptoError::InvalidKeyObjectType(type_name, "private or public")));
        }
    }
    Ok(key)
}

/// Node's `prepareAsymmetricKey` and the parse that follows it: the key `value` names.
pub(crate) fn asymmetric_key(c: &mut Context<'_>, value: RootId, key_use: KeyUse) -> Result<Arc<Key>, RootedError> {
    if let Some(key) = key_of(c, value)? {
        return key_object_for(c, value, key, key_use);
    }
    let undefined = c.undefined();
    if super::is_string(c, value) || super::is_view(c, value)? || super::is_any_buffer(c, value)? {
        let data = super::binary_like(c, value, "key", undefined)?;
        let encoding = KeyEncoding { format: KeyFormat::Pem, encoding: None, cipher: None, passphrase: None };
        return import(c, &data, &encoding, key_use);
    }
    if !c.is_object_rooted(value)? {
        return Err(errors::invalid_type(c, value, "key", key_use.expected()));
    }
    let data = get(c, value, "key")?;
    if let Some(key) = key_of(c, data)? {
        return key_object_for(c, data, key, key_use);
    }
    let format = get(c, value, "format")?;
    if c.string_text(format)?.as_deref() == Some("jwk") {
        super::object_argument(c, data, "key.key")?;
        return import_jwk(c, data, key_use.kind()).map(Arc::new);
    }
    let string_or_buffer = super::is_string(c, data) || super::is_view(c, data)? || super::is_any_buffer(c, data)?;
    if !string_or_buffer {
        return Err(errors::invalid_type(c, data, "key.key", key_use.expected()));
    }
    let text_encoding = get(c, value, "encoding")?;
    let bytes = super::binary_like(c, data, "key", text_encoding)?;
    let encoding = parse_key_encoding(c, value, None, key_use.public(), None)?;
    import(c, &bytes, &encoding, key_use)
}

fn import(c: &mut Context<'_>, data: &[u8], encoding: &KeyEncoding, key_use: KeyUse) -> Result<Arc<Key>, RootedError> {
    let format = match encoding.format {
        KeyFormat::Der => Format::Der,
        _ => Format::Pem,
    };
    let kind = key_use.kind();
    let imported = OwnedKey::import(kind, data, format, encoding.encoding, encoding.passphrase.as_deref(), None);
    // A public key may be read from private key input; consuming keeps the private half.
    let imported = match (imported, key_use) {
        (Err(_), KeyUse::ConsumePublic) => OwnedKey::import(KeyKind::Private, data, format, encoding.encoding, encoding.passphrase.as_deref(), None),
        (imported, _) => imported,
    };
    let key = imported.map_err(|error| key_error(c, error))?;
    Ok(Arc::new(Key::Asymmetric(key)))
}

/// A KeyError as the error Node raises for it.
pub(crate) fn key_error(c: &mut Context<'_>, error: KeyError) -> RootedError {
    match error {
        KeyError::OpenSsl(stack) => errors::openssl(c, stack),
        KeyError::InvalidJwk => errors::throw(c, CryptoError::InvalidJwk),
        KeyError::InvalidCurve(_) | KeyError::UnsupportedJwkCurve(_) => errors::throw(c, CryptoError::InvalidCurve),
        KeyError::UnsupportedJwkKeyType => errors::throw(c, CryptoError::JwkUnsupportedKeyType),
        KeyError::MissingPassphrase => errors::throw(c, CryptoError::MissingPassphrase),
        KeyError::Unsupported(message) => errors::throw(c, CryptoError::Plain(message)),
        other => {
            let message = format!("{other:?}");
            match c.error_rooted(&message) {
                Ok(error) => c.throw(error),
                Err(error) => error,
            }
        }
    }
}

// ---- JWK -----------------------------------------------------------------------------

/// `validateOneOf(value, name, accepted)` for string choices.
fn one_of(c: &mut Context<'_>, value: RootId, name: &str, accepted: &[&str]) -> Result<String, RootedError> {
    match c.string_text(value)? {
        Some(text) if accepted.contains(&text.as_str()) => Ok(text),
        _ => {
            let list = accepted.iter().map(|choice| format!("'{choice}'")).collect::<Vec<_>>().join(", ");
            Err(errors::invalid_value(c, value, name, &format!("must be one of: {list}")))
        }
    }
}

fn base64url_field(c: &mut Context<'_>, jwk: RootId, name: &str) -> Result<Option<Vec<u8>>, RootedError> {
    let value = get(c, jwk, name)?;
    Ok(c.string_text(value)?.map(|text| crate::modules::buffer_codec::base64_decode(text.as_bytes())))
}

/// The JWK members `kty` names, in Node's import order.
const RSA_FIELDS: [&str; 8] = ["n", "e", "d", "p", "q", "dp", "dq", "qi"];
/// The members of an RSA public JWK: the first of `RSA_FIELDS`.
const RSA_PUBLIC_FIELDS: usize = 2;
const EC_CURVES: [&str; 4] = ["P-256", "secp256k1", "P-384", "P-521"];

/// The raw key sizes of the OKP curves (RFC 8032, RFC 7748).
const OKP_KEY_BYTES: [(&str, usize); 4] = [("Ed25519", 32), ("Ed448", 57), ("X25519", 32), ("X448", 56)];

fn okp_key_size(curve: &str) -> usize {
    OKP_KEY_BYTES.iter().find(|(name, _)| *name == curve).map_or(0, |(_, size)| *size)
}

/// `getKeyObjectHandleFromJwk`.
pub(crate) fn import_jwk(c: &mut Context<'_>, jwk: RootId, kind: KeyKind) -> Result<Key, RootedError> {
    super::object_argument(c, jwk, "key")?;
    let kty_value = get(c, jwk, "kty")?;
    let kty = one_of(c, kty_value, "key.kty", &["RSA", "EC", "OKP"])?;
    let private = kind == KeyKind::Private;
    let mut fields = BTreeMap::new();
    let mut crv = None;
    match kty.as_str() {
        "OKP" => {
            let curve_value = get(c, jwk, "crv")?;
            super::string_argument(c, curve_value, "key.crv")?;
            let curve = one_of(c, curve_value, "key.crv", &OKP_KEY_BYTES.map(|(name, _)| name))?;
            let x = get(c, jwk, "x")?;
            super::string_argument(c, x, "key.x")?;
            if private {
                let d = get(c, jwk, "d")?;
                super::string_argument(c, d, "key.d")?;
            }
            let name = if private { "d" } else { "x" };
            let data = base64url_field(c, jwk, name)?.unwrap_or_default();
            if data.len() != okp_key_size(&curve) {
                return Err(errors::throw(c, CryptoError::InvalidJwk));
            }
            for name in ["x", "d"] {
                if let Some(bytes) = base64url_field(c, jwk, name)? {
                    fields.insert(name.to_owned(), bytes);
                }
            }
            if !private {
                fields.remove("d");
            }
            crv = Some(curve);
        }
        "EC" => {
            let curve_value = get(c, jwk, "crv")?;
            super::string_argument(c, curve_value, "key.crv")?;
            crv = Some(one_of(c, curve_value, "key.crv", &EC_CURVES)?);
            for name in ["x", "y"] {
                let value = get(c, jwk, name)?;
                super::string_argument(c, value, &format!("key.{name}"))?;
            }
            for name in ["x", "y", "d"] {
                if let Some(bytes) = base64url_field(c, jwk, name)? {
                    fields.insert(name.to_owned(), bytes);
                }
            }
        }
        _ => {
            let required = if private { &RSA_FIELDS[..] } else { &RSA_FIELDS[..RSA_PUBLIC_FIELDS] };
            for name in required {
                let value = get(c, jwk, name)?;
                super::string_argument(c, value, &format!("key.{name}"))?;
            }
            for name in RSA_FIELDS {
                if let Some(bytes) = base64url_field(c, jwk, name)? {
                    fields.insert(name.to_owned(), bytes);
                }
            }
        }
    }
    if private {
        let d = get(c, jwk, "d")?;
        super::string_argument(c, d, "key.d")?;
    } else {
        for name in ["d", "p", "q", "dp", "dq", "qi"] {
            fields.remove(name);
        }
    }
    let jwk = Jwk { kty, crv, alg: None, fields, oth: Vec::new() };
    match OwnedKey::import_jwk(kind, &jwk) {
        Ok(key) => Ok(Key::Asymmetric(key)),
        Err(KeyError::OpenSsl(_) | KeyError::InvalidJwk | KeyError::Unsupported(_)) => Err(errors::throw(c, CryptoError::InvalidJwk)),
        Err(error) => Err(key_error(c, error)),
    }
}

/// The JWK members of a Jwk, in the order Node writes them.
fn jwk_member_order(kty: &str, private: bool) -> &'static [&'static str] {
    match (kty, private) {
        ("RSA", false) => &["kty", "n", "e"],
        ("RSA", true) => &["kty", "n", "e", "d", "p", "q", "dp", "dq", "qi"],
        ("EC", false) => &["kty", "x", "y", "crv"],
        ("EC", true) => &["kty", "x", "y", "crv", "d"],
        (_, false) => &["crv", "x", "kty"],
        (_, true) => &["crv", "d", "x", "kty"],
    }
}

/// `exportJwk`: the key as a JWK object.
pub(crate) fn export_jwk(c: &mut Context<'_>, key: &Key) -> Outcome {
    let result = c.object_rooted()?;
    match key {
        Key::Secret(bytes) => {
            let kty = c.string_rooted("oct");
            put(c, result, "kty", kty)?;
            let k = c.string_rooted(&crate::modules::buffer_codec::base64_encode(bytes, false, true));
            put(c, result, "k", k)?;
        }
        Key::Asymmetric(owned) => {
            let jwk = owned.export_jwk_with_flags(false).map_err(|error| key_error(c, error))?;
            for &member in jwk_member_order(&jwk.kty, owned.kind() == KeyKind::Private) {
                let value = match member {
                    "kty" => c.string_rooted(&jwk.kty),
                    "crv" => c.string_rooted(jwk.crv.as_deref().unwrap_or_default()),
                    field => match jwk.fields.get(field) {
                        Some(bytes) => c.string_rooted(&crate::modules::buffer_codec::base64_encode(bytes, false, true)),
                        None => continue,
                    },
                };
                put(c, result, member, value)?;
            }
        }
    }
    Ok(result)
}

// ---- KeyObject members ---------------------------------------------------------------

/// `new KeyObject(type, handle)`: Node's validation; handles are never exposed.
pub(crate) fn constructor(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (type_value, handle) = (arg(c, args, 0), arg(c, args, 1));
    let known = matches!(c.string_text(type_value)?.as_deref(), Some("secret" | "public" | "private"));
    if !known {
        return Err(errors::invalid_value(c, type_value, "type", "is invalid"));
    }
    Err(errors::invalid_type(c, handle, "handle", "of type object"))
}

/// `keyObject.type`.
pub(crate) fn get_type(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let key = this_key(c, this)?;
    Ok(c.string_rooted(key.type_name()))
}

/// `keyObject.equals(otherKeyObject)`.
pub(crate) fn equals(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let other = arg(c, args, 0);
    let key = this_key(c, this)?;
    let Some(other_key) = key_object_key(c, other)? else {
        return Err(errors::invalid_type(c, other, "otherKeyObject", "an instance of KeyObject"));
    };
    let equal = match (&*key, &*other_key) {
        (Key::Secret(left), Key::Secret(right)) => left.len() == right.len() && openssl::memcmp::eq(left, right),
        (Key::Asymmetric(left), Key::Asymmetric(right)) => left.equals(right),
        _ => false,
    };
    Ok(c.boolean(equal))
}

/// `secretKeyObject.symmetricKeySize`.
pub(crate) fn symmetric_key_size(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let key = this_key(c, this)?;
    let size = key.secret().map_or(0, <[u8]>::len);
    Ok(c.number(size as f64))
}

/// `asymmetricKeyObject.asymmetricKeyType`.
pub(crate) fn asymmetric_key_type(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let key = this_key(c, this)?;
    match key.asymmetric().and_then(OwnedKey::key_type) {
        Some(name) => Ok(c.string_rooted(name)),
        None => Ok(c.undefined()),
    }
}

/// The value of a public exponent's big-endian bytes, as a bigint.
fn exponent_bigint(c: &mut Context<'_>, bytes: &[u8]) -> Outcome {
    let value = bytes.iter().fold(0i128, |value, byte| (value << u8::BITS) | i128::from(*byte));
    Ok(c.bigint_rooted(value))
}

/// `asymmetricKeyObject.asymmetricKeyDetails`.
pub(crate) fn asymmetric_key_details(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let key = this_key(c, this)?;
    let Some(owned) = key.asymmetric() else {
        return Ok(c.undefined());
    };
    let details = owned.details().map_err(|error| key_error(c, error))?;
    let result = c.object_rooted()?;
    if let Some(length) = details.modulus_length {
        let value = c.number(f64::from(length));
        put(c, result, "modulusLength", value)?;
    }
    if let Some(exponent) = &details.public_exponent {
        let value = exponent_bigint(c, exponent)?;
        put(c, result, "publicExponent", value)?;
    }
    if let Some(length) = details.divisor_length {
        let value = c.number(f64::from(length));
        put(c, result, "divisorLength", value)?;
    }
    if let Some(curve) = &details.named_curve {
        let value = c.string_rooted(curve);
        put(c, result, "namedCurve", value)?;
    }
    if let Some(hash) = &details.hash_algorithm {
        let value = c.string_rooted(hash);
        put(c, result, "hashAlgorithm", value)?;
    }
    if let Some(hash) = &details.mgf1_hash_algorithm {
        let value = c.string_rooted(hash);
        put(c, result, "mgf1HashAlgorithm", value)?;
    }
    if let Some(length) = details.salt_length {
        let value = c.number(f64::from(length));
        put(c, result, "saltLength", value)?;
    }
    Ok(result)
}

/// Encoded key bytes as `export` returns them: PEM as a string, DER as a Buffer.
fn encoded(c: &mut Context<'_>, bytes: Vec<u8>, format: KeyFormat) -> Outcome {
    match format {
        KeyFormat::Pem => Ok(c.string_rooted(&String::from_utf8_lossy(&bytes))),
        _ => super::buffer(c, bytes),
    }
}

/// An asymmetric key encoded as `encoding` describes.
pub(crate) fn encode_key(c: &mut Context<'_>, key: &Key, encoding: &KeyEncoding) -> Outcome {
    let Some(owned) = key.asymmetric() else {
        return Err(errors::throw(c, CryptoError::Plain("Secret keys have no asymmetric encoding")));
    };
    if encoding.format == KeyFormat::Jwk {
        return export_jwk(c, key);
    }
    if let Some(cipher) = &encoding.cipher {
        if super::cipher::cipher_by_name(cipher).is_none() {
            return Err(errors::throw(c, CryptoError::UnknownCipher));
        }
    }
    let format = if encoding.format == KeyFormat::Der { Format::Der } else { Format::Pem };
    let kind = encoding.encoding.unwrap_or_else(|| if owned.kind() == KeyKind::Public { Encoding::Spki } else { Encoding::Pkcs8 });
    let bytes = owned.export(format, kind, encoding.cipher.as_deref(), encoding.passphrase.as_deref());
    let bytes = bytes.map_err(|error| key_error(c, error))?;
    encoded(c, bytes, encoding.format)
}

/// `secretKeyObject.export([options])`.
pub(crate) fn export_secret(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    let key = this_key(c, this)?;
    if !is_undefined(c, options) {
        super::object_argument(c, options, "options")?;
        let format = get(c, options, "format")?;
        let choice = c.string_text(format)?;
        match choice.as_deref() {
            Some("jwk") => return export_jwk(c, &key),
            Some("buffer") => {}
            None if is_undefined(c, format) => {}
            _ => return Err(errors::invalid_value(c, format, "options.format", "must be one of: undefined, 'buffer', 'jwk'")),
        }
    }
    super::buffer(c, key.secret().unwrap_or_default().to_vec())
}

/// `publicKeyObject.export(options)` and `privateKeyObject.export(options)`.
pub(crate) fn export_asymmetric(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    let key = this_key(c, this)?;
    let owned = key.asymmetric().ok_or_else(|| RootedError::host("asymmetric export of a secret key"))?;
    let public = owned.kind() == KeyKind::Public;
    if c.truthy_rooted(options)? {
        let format = get(c, options, "format")?;
        if c.string_text(format)?.as_deref() == Some("jwk") {
            let passphrase = get(c, options, "passphrase")?;
            if !public && !is_undefined(c, passphrase) {
                return Err(errors::throw(c, CryptoError::IncompatibleKeyOptions("jwk", "does not support encryption")));
            }
            return export_jwk(c, &key);
        }
    }
    let key_type = owned.key_type().unwrap_or_default();
    let encoding = parse_key_encoding(c, options, Some(key_type), Some(public), None)?;
    encode_key(c, &key, &encoding)
}

/// `KeyObject.from(cryptoKey)`.
pub(crate) fn from(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let key = arg(c, args, 0);
    match super::web::crypto_key_object(c, key)? {
        Some(object) => Ok(object),
        None => Err(errors::invalid_type(c, key, "key", "an instance of CryptoKey")),
    }
}

/// `keyObject.toCryptoKey(algorithm, extractable, keyUsages)`.
pub(crate) fn to_crypto_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    this_key(c, this)?;
    let (algorithm, extractable, usages) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    super::web::key_object_to_crypto_key(c, this, algorithm, extractable, usages)
}

pub(crate) fn build(c: &mut Context<'_>) -> Result<Vec<(Class, RootId)>, RootedError> {
    let key_object = c.host_constructor_named(crate::host::shared_vm::operation!("cryptoKeyObjectConstructor"), "KeyObject")?;
    let prototype = get(c, key_object, "prototype")?;
    define_getters(c, prototype, &[("type", "cryptoKeyObjectType")], Members::Declared)?;
    define_methods(c, prototype, &[("equals", "cryptoKeyObjectEquals")], Members::Declared)?;
    super::to_string_tag(c, prototype, "KeyObject")?;
    define_methods(c, key_object, &[("from", "cryptoKeyObjectFrom")], Members::Declared)?;

    let secret = super::subclass(c, "cryptoKeyObjectConstructor", "SecretKeyObject", key_object)?;
    let prototype = get(c, secret, "prototype")?;
    define_getters(c, prototype, &[("symmetricKeySize", "cryptoKeyObjectSymmetricKeySize")], Members::Declared)?;
    define_methods(c, prototype, &[("export", "cryptoKeyObjectExportSecret"), ("toCryptoKey", "cryptoKeyObjectToCryptoKey")], Members::Declared)?;

    let asymmetric = super::subclass(c, "cryptoKeyObjectConstructor", "AsymmetricKeyObject", key_object)?;
    let prototype = get(c, asymmetric, "prototype")?;
    define_getters(
        c,
        prototype,
        &[("asymmetricKeyType", "cryptoKeyObjectAsymmetricKeyType"), ("asymmetricKeyDetails", "cryptoKeyObjectAsymmetricKeyDetails")],
        Members::Declared,
    )?;
    define_methods(c, prototype, &[("toCryptoKey", "cryptoKeyObjectToCryptoKey")], Members::Declared)?;

    let mut classes = vec![(Class::KeyObject, key_object), (Class::SecretKeyObject, secret)];
    for (class, name) in [(Class::PublicKeyObject, "PublicKeyObject"), (Class::PrivateKeyObject, "PrivateKeyObject")] {
        let constructor = super::subclass(c, "cryptoKeyObjectConstructor", name, asymmetric)?;
        let prototype = get(c, constructor, "prototype")?;
        define_methods(c, prototype, &[("export", "cryptoKeyObjectExportAsymmetric")], Members::Declared)?;
        classes.push((class, constructor));
    }
    Ok(classes)
}

/// `isKeyObject(value)` (lib/internal/crypto/keys.js).
pub(crate) fn is_key_object(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let found = key_object_key(c, value)?.is_some();
    Ok(c.boolean(found))
}

/// `internal/crypto/keys`: the key classes and the public creators, with the predicates.
pub(crate) fn internal_module(c: &mut Context<'_>) -> Outcome {
    let crypto = super::exports::module(c)?;
    let module = c.object_rooted()?;
    for name in ["createSecretKey", "createPublicKey", "createPrivateKey"] {
        let function = get(c, crypto, name)?;
        put(c, module, name, function)?;
    }
    for (name, class) in [
        ("KeyObject", Class::KeyObject),
        ("CryptoKey", Class::CryptoKey),
        ("SecretKeyObject", Class::SecretKeyObject),
        ("PublicKeyObject", Class::PublicKeyObject),
        ("PrivateKeyObject", Class::PrivateKeyObject),
    ] {
        let constructor = super::class(c, class)?;
        put(c, module, name, constructor)?;
    }
    for (name, operation) in [("isKeyObject", "cryptoIsKeyObject"), ("isCryptoKey", "cryptoIsCryptoKey")] {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(c, module, name, function)?;
    }
    Ok(module)
}

// ---- Creating keys -------------------------------------------------------------------

/// `crypto.createSecretKey(key[, encoding])`.
pub(crate) fn create_secret_key(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (key, encoding) = (arg(c, args, 0), arg(c, args, 1));
    let bytes = if super::is_string(c, key) {
        super::string_bytes(c, key, encoding)?
    } else {
        match super::buffer_bytes(c, key) {
            Some(bytes) if key_of(c, key)?.is_none() => bytes,
            _ => return Err(errors::invalid_type(c, key, "key", super::accepts::BUFFER_SOURCE)),
        }
    };
    key_object(c, Arc::new(Key::Secret(Material::new(bytes))))
}

/// `crypto.createPublicKey(key)`: a public key, derived from a private one when given one.
pub(crate) fn create_public_key(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let key = asymmetric_key(c, value, KeyUse::CreatePublic)?;
    let public = match key.asymmetric() {
        Some(owned) if owned.kind() == KeyKind::Private => {
            let public = owned.to_public().map_err(|error| key_error(c, error))?;
            Arc::new(Key::Asymmetric(public))
        }
        _ => key,
    };
    key_object(c, public)
}

/// `crypto.createPrivateKey(key)`.
pub(crate) fn create_private_key(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let key = asymmetric_key(c, value, KeyUse::CreatePrivate)?;
    key_object(c, key)
}

// ---- Generating keys -----------------------------------------------------------------

/// The `publicKeyEncoding`/`privateKeyEncoding` of a key pair request.
struct PairEncoding {
    public: Option<KeyEncoding>,
    private: Option<KeyEncoding>,
}

fn pair_encoding(c: &mut Context<'_>, key_type: &str, options: RootId) -> Result<PairEncoding, RootedError> {
    let mut halves = [None, None];
    for (index, (name, public)) in [("publicKeyEncoding", true), ("privateKeyEncoding", false)].into_iter().enumerate() {
        let value = super::option(c, options, name)?;
        if is_nullish(c, value) {
            continue;
        }
        if !c.is_object_rooted(value)? {
            return Err(errors::invalid_value(c, value, &format!("options.{name}"), "is invalid"));
        }
        halves[index] = Some(parse_key_encoding(c, value, Some(key_type), Some(public), Some(name))?);
    }
    let [public, private] = halves;
    Ok(PairEncoding { public, private })
}

/// A validated key pair request.
enum PairRequest {
    Rsa { bits: u32, exponent: u32, pss: Option<(Option<String>, Option<String>, Option<u32>)> },
    Dsa { bits: u32, divisor_bits: Option<u32> },
    Ec { curve: String, explicit: bool },
    Named(&'static str),
    Dh { group: Option<String>, prime: Option<Vec<u8>>, bits: Option<u32>, generator: u32 },
}

fn optional_string(c: &mut Context<'_>, options: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let value = get(c, options, name)?;
    if is_undefined(c, value) {
        return Ok(None);
    }
    super::string_argument(c, value, &format!("options.{name}")).map(Some)
}

fn incompatible_pair(c: &mut Context<'_>, first: &str, second: &str) -> RootedError {
    let message = format!("Option \"{first}\" cannot be used in combination with option \"{second}\"");
    errors::throw(c, CryptoError::IncompatibleOptionPair(message))
}

fn pair_request(c: &mut Context<'_>, key_type: &str, options: RootId) -> Result<PairRequest, RootedError> {
    let uint32 = u32::MAX as f64;
    let int32 = i32::MAX as f64;
    Ok(match key_type {
        "rsa" | "rsa-pss" => {
            super::object_argument(c, options, "options")?;
            let modulus = get(c, options, "modulusLength")?;
            let bits = super::integer_argument(c, modulus, "options.modulusLength", 0.0, uint32)? as u32;
            let exponent = get(c, options, "publicExponent")?;
            let exponent = if is_nullish(c, exponent) {
                DEFAULT_PUBLIC_EXPONENT
            } else {
                super::integer_argument(c, exponent, "options.publicExponent", 0.0, uint32)?
            } as u32;
            let pss = if key_type == "rsa-pss" {
                let salt = get(c, options, "saltLength")?;
                let salt = if is_undefined(c, salt) { None } else { Some(super::integer_argument(c, salt, "options.saltLength", 0.0, int32)? as u32) };
                let hash = match optional_string(c, options, "hashAlgorithm")? {
                    Some(hash) => Some(hash),
                    None => optional_string(c, options, "hash")?,
                };
                let mgf1 = match optional_string(c, options, "mgf1HashAlgorithm")? {
                    Some(hash) => Some(hash),
                    None => optional_string(c, options, "mgf1Hash")?,
                };
                Some((hash, mgf1, salt))
            } else {
                None
            };
            PairRequest::Rsa { bits, exponent, pss }
        }
        "dsa" => {
            super::object_argument(c, options, "options")?;
            let modulus = get(c, options, "modulusLength")?;
            let bits = super::integer_argument(c, modulus, "options.modulusLength", 0.0, uint32)? as u32;
            let divisor = get(c, options, "divisorLength")?;
            let divisor_bits = if is_nullish(c, divisor) { None } else { Some(super::integer_argument(c, divisor, "options.divisorLength", 0.0, int32)? as u32) };
            PairRequest::Dsa { bits, divisor_bits }
        }
        "ec" => {
            super::object_argument(c, options, "options")?;
            let curve = get(c, options, "namedCurve")?;
            let curve = super::string_argument(c, curve, "options.namedCurve")?;
            let encoding = get(c, options, "paramEncoding")?;
            let explicit = match c.string_text(encoding)?.as_deref() {
                _ if is_nullish(c, encoding) => false,
                Some("named") => false,
                Some("explicit") => true,
                _ => return Err(errors::invalid_value(c, encoding, "options.paramEncoding", "is invalid")),
            };
            PairRequest::Ec { curve, explicit }
        }
        "ed25519" => PairRequest::Named("ED25519"),
        "ed448" => PairRequest::Named("ED448"),
        "x25519" => PairRequest::Named("X25519"),
        "x448" => PairRequest::Named("X448"),
        "dh" => {
            super::object_argument(c, options, "options")?;
            let group = get(c, options, "group")?;
            let prime = get(c, options, "prime")?;
            let prime_length = get(c, options, "primeLength")?;
            let generator = get(c, options, "generator")?;
            if !is_nullish(c, group) {
                for (name, value) in [("prime", prime), ("primeLength", prime_length), ("generator", generator)] {
                    if !is_nullish(c, value) {
                        return Err(incompatible_pair(c, "group", name));
                    }
                }
                let group = super::string_argument(c, group, "options.group")?;
                PairRequest::Dh { group: Some(group), prime: None, bits: None, generator: DH_DEFAULT_GENERATOR as u32 }
            } else {
                let (prime_bytes, bits) = if !is_nullish(c, prime) {
                    if !is_nullish(c, prime_length) {
                        return Err(incompatible_pair(c, "prime", "primeLength"));
                    }
                    if !super::is_view(c, prime)? {
                        return Err(errors::invalid_type(c, prime, "options.prime", super::accepts::VIEW));
                    }
                    (super::buffer_bytes(c, prime), None)
                } else if !is_nullish(c, prime_length) {
                    (None, Some(super::integer_argument(c, prime_length, "options.primeLength", 0.0, int32)? as u32))
                } else {
                    return Err(errors::throw(c, CryptoError::MissingOption("At least one of the group, prime, or primeLength options")));
                };
                let generator = if is_nullish(c, generator) { DH_DEFAULT_GENERATOR } else { super::integer_argument(c, generator, "options.generator", 0.0, int32)? };
                PairRequest::Dh { group: None, prime: prime_bytes, bits, generator: generator as u32 }
            }
        }
        _ => {
            let value = c.string_rooted(key_type);
            return Err(errors::invalid_value(c, value, "type", "must be a supported key type"));
        }
    })
}

fn generate_pair(c: &mut Context<'_>, request: &PairRequest) -> Result<OwnedKey, RootedError> {
    let result = match request {
        PairRequest::Rsa { bits, exponent, pss } => {
            let digests = match pss {
                Some((hash, mgf1, _)) => {
                    let mut fetched = [None, None];
                    for (index, name) in [hash, mgf1].into_iter().enumerate() {
                        if let Some(name) = name {
                            let digest = super::hash::digest_by_name(c, name).map_err(|_| errors::throw(c, CryptoError::InvalidDigest(Some(name))))?;
                            fetched[index] = Some(digest);
                        }
                    }
                    Some(fetched)
                }
                None => None,
            };
            let pss = match (pss, &digests) {
                (Some((_, _, salt)), Some([digest, mgf1])) => Some(Pss { digest: digest.as_deref(), mgf1_digest: mgf1.as_deref(), salt_length: *salt }),
                _ => None,
            };
            OwnedKey::generate(Generation::Rsa { bits: *bits, exponent: *exponent, pss })
        }
        PairRequest::Dsa { bits, divisor_bits } => OwnedKey::generate(Generation::Dsa { bits: *bits, divisor_bits: *divisor_bits }),
        PairRequest::Ec { curve, explicit } => {
            if !crate::modules::crypto_asymmetric_generate::curve_available(curve) {
                return Err(errors::throw(c, CryptoError::InvalidCurve));
            }
            OwnedKey::generate(Generation::Ec { curve, explicit: *explicit })
        }
        PairRequest::Named(name) => OwnedKey::generate(Generation::Named(name)),
        PairRequest::Dh { group, prime, bits, generator } => {
            let params = match (group, prime, bits) {
                (Some(group), _, _) => DhParameters::Group(group),
                (_, Some(prime), _) => DhParameters::Prime(prime),
                (_, _, bits) => DhParameters::Bits(bits.unwrap_or_default()),
            };
            OwnedKey::generate(Generation::Dh { params, generator: *generator })
        }
    };
    result.map_err(|error| match error {
        KeyError::OpenSsl(stack) if matches!(request, PairRequest::Dh { group: Some(_), .. }) && stack.errors().is_empty() => {
            errors::throw(c, CryptoError::UnknownDhGroup)
        }
        error => key_error(c, error),
    })
}

/// Both halves of a generated pair, encoded as requested or as KeyObjects.
fn pair_values(c: &mut Context<'_>, key: OwnedKey, encoding: &PairEncoding) -> Result<[RootId; 2], RootedError> {
    let public = key.to_public().map_err(|error| key_error(c, error))?;
    let public = Arc::new(Key::Asymmetric(public));
    let private = Arc::new(Key::Asymmetric(key));
    let public = match &encoding.public {
        Some(encoding) => encode_key(c, &public, encoding)?,
        None => key_object(c, public)?,
    };
    let private = match &encoding.private {
        Some(encoding) => encode_key(c, &private, encoding)?,
        None => key_object(c, private)?,
    };
    Ok([public, private])
}

fn key_pair_inputs(c: &mut Context<'_>, key_type: RootId, options: RootId) -> Result<(PairRequest, PairEncoding), RootedError> {
    let key_type = super::string_argument(c, key_type, "type")?;
    let encoding = pair_encoding(c, &key_type, options)?;
    if !is_undefined(c, options) {
        super::object_argument(c, options, "options")?;
    }
    let request = pair_request(c, &key_type, options)?;
    Ok((request, encoding))
}

/// `crypto.generateKeyPairSync(type[, options])`.
pub(crate) fn generate_key_pair_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (key_type, options) = (arg(c, args, 0), arg(c, args, 1));
    let (request, encoding) = key_pair_inputs(c, key_type, options)?;
    let key = generate_pair(c, &request)?;
    let [public, private] = pair_values(c, key, &encoding)?;
    let result = c.object_rooted()?;
    put(c, result, "publicKey", public)?;
    put(c, result, "privateKey", private)?;
    Ok(result)
}

/// `crypto.generateKeyPair(type[, options], callback)`.
pub(crate) fn generate_key_pair(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (key_type, mut options, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if c.is_callable_rooted(options)? {
        callback = options;
        options = c.undefined();
    }
    let callback = super::callback_argument(c, callback, "callback")?;
    let (request, encoding) = key_pair_inputs(c, key_type, options)?;
    let result = generate_pair(c, &request).and_then(|key| pair_values(c, key, &encoding)).map(Vec::from);
    super::complete(c, callback, result)
}

fn secret_key_length(c: &mut Context<'_>, key_type: RootId, options: RootId) -> Result<usize, RootedError> {
    let name = super::string_argument(c, key_type, "type")?;
    super::object_argument(c, options, "options")?;
    let length = get(c, options, "length")?;
    match name.as_str() {
        "hmac" => {
            let bits = super::integer_argument(c, length, "options.length", HMAC_MIN_BITS, HMAC_MAX_BITS)?;
            Ok(bits as usize / u8::BITS as usize)
        }
        "aes" => {
            let bits = c.rooted_value(length).and_then(|value| value.as_number());
            match bits.filter(|bits| AES_KEY_BITS.contains(bits)) {
                Some(bits) => Ok(bits as usize / u8::BITS as usize),
                None => Err(errors::invalid_value(c, length, "options.length", "must be one of: 128, 192, 256")),
            }
        }
        _ => Err(errors::invalid_value(c, key_type, "type", "must be a supported key type")),
    }
}

fn random_secret(c: &mut Context<'_>, length: usize) -> Outcome {
    let mut bytes = vec![0; length];
    super::random::fill(&mut bytes).map_err(|stack| errors::openssl(c, stack))?;
    key_object(c, Arc::new(Key::Secret(Material::new(bytes))))
}

/// `crypto.generateKeySync(type, options)`.
pub(crate) fn generate_key_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (key_type, options) = (arg(c, args, 0), arg(c, args, 1));
    let length = secret_key_length(c, key_type, options)?;
    random_secret(c, length)
}

/// `crypto.generateKey(type, options, callback)`.
pub(crate) fn generate_key(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (key_type, mut options, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if c.is_callable_rooted(options)? {
        callback = options;
        options = c.undefined();
    }
    let callback = super::callback_argument(c, callback, "callback")?;
    let length = secret_key_length(c, key_type, options)?;
    let result = random_secret(c, length).map(|key| vec![key]);
    super::complete(c, callback, result)
}
