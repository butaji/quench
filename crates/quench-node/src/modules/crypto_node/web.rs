//! WebCrypto: `Crypto`, `SubtleCrypto` and `CryptoKey`.
//!
//! A CryptoKey's native state is its key material and the algorithm, extractability and
//! usages it was made with; the guest-visible `algorithm` and `usages` objects and the
//! KeyObject `KeyObject.from()` returns are kept in private slots so each read gives the
//! same object.

use super::errors::{self, CryptoError};
use super::keys::{self, Key};
use super::{arg, define_getters, define_methods, get, is_nullish, is_undefined, put, with_native, Class, Context, Members, Native, Outcome};
use crate::modules::crypto_asymmetric::{Encoding, Format, KeyKind, OwnedKey};
use crate::modules::crypto_asymmetric_generate::Generation;
use crate::modules::crypto_secret_material::Material;
use crate::modules::private_slots;
use quench_runtime::{RootId, RootedError};
use std::sync::Arc;

const KEY_OBJECT_SLOT: &str = "\0crypto:cryptoKey.keyObject";
const ALGORITHM_SLOT: &str = "\0crypto:cryptoKey.algorithm";
const USAGES_SLOT: &str = "\0crypto:cryptoKey.usages";

// ---- The domain ----------------------------------------------------------------------

/// The WebCrypto algorithms, by their registered names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Algorithm {
    RsassaPkcs1,
    RsaPss,
    RsaOaep,
    Ecdsa,
    Ecdh,
    AesCtr,
    AesCbc,
    AesGcm,
    AesKw,
    Hmac,
    Ed25519,
    Ed448,
    X25519,
    X448,
    Pbkdf2,
    Hkdf,
}

const ALGORITHMS: [(Algorithm, &str); 16] = [
    (Algorithm::RsassaPkcs1, "RSASSA-PKCS1-v1_5"),
    (Algorithm::RsaPss, "RSA-PSS"),
    (Algorithm::RsaOaep, "RSA-OAEP"),
    (Algorithm::Ecdsa, "ECDSA"),
    (Algorithm::Ecdh, "ECDH"),
    (Algorithm::AesCtr, "AES-CTR"),
    (Algorithm::AesCbc, "AES-CBC"),
    (Algorithm::AesGcm, "AES-GCM"),
    (Algorithm::AesKw, "AES-KW"),
    (Algorithm::Hmac, "HMAC"),
    (Algorithm::Ed25519, "Ed25519"),
    (Algorithm::Ed448, "Ed448"),
    (Algorithm::X25519, "X25519"),
    (Algorithm::X448, "X448"),
    (Algorithm::Pbkdf2, "PBKDF2"),
    (Algorithm::Hkdf, "HKDF"),
];

/// What a key may be used for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Usage {
    Encrypt,
    Decrypt,
    Sign,
    Verify,
    DeriveKey,
    DeriveBits,
    WrapKey,
    UnwrapKey,
}

const USAGES: [(Usage, &str); 8] = [
    (Usage::Encrypt, "encrypt"),
    (Usage::Decrypt, "decrypt"),
    (Usage::Sign, "sign"),
    (Usage::Verify, "verify"),
    (Usage::DeriveKey, "deriveKey"),
    (Usage::DeriveBits, "deriveBits"),
    (Usage::WrapKey, "wrapKey"),
    (Usage::UnwrapKey, "unwrapKey"),
];

impl Usage {
    fn name(self) -> &'static str {
        USAGES[self as usize].1
    }
}

impl Algorithm {
    fn name(self) -> &'static str {
        ALGORITHMS[self as usize].1
    }

    fn by_name(name: &str) -> Option<Self> {
        ALGORITHMS.iter().find(|(_, known)| known.eq_ignore_ascii_case(name)).map(|(algorithm, _)| *algorithm)
    }

    fn is_aes(self) -> bool {
        matches!(self, Self::AesCtr | Self::AesCbc | Self::AesGcm | Self::AesKw)
    }

    fn is_rsa(self) -> bool {
        matches!(self, Self::RsassaPkcs1 | Self::RsaPss | Self::RsaOaep)
    }

    fn is_ec(self) -> bool {
        matches!(self, Self::Ecdsa | Self::Ecdh)
    }

    fn is_okp(self) -> bool {
        matches!(self, Self::Ed25519 | Self::Ed448 | Self::X25519 | Self::X448)
    }

    /// The usages a public (or secret) key and a private key of this algorithm may have.
    fn usages(self) -> (&'static [Usage], &'static [Usage]) {
        use Usage::*;
        match self {
            Self::RsassaPkcs1 | Self::RsaPss | Self::Ecdsa | Self::Ed25519 | Self::Ed448 => (&[Verify], &[Sign]),
            Self::RsaOaep => (&[Encrypt, WrapKey], &[Decrypt, UnwrapKey]),
            Self::Ecdh | Self::X25519 | Self::X448 => (&[], &[DeriveKey, DeriveBits]),
            Self::AesCtr | Self::AesCbc | Self::AesGcm => (&[Encrypt, Decrypt, WrapKey, UnwrapKey], &[]),
            Self::AesKw => (&[WrapKey, UnwrapKey], &[]),
            Self::Hmac => (&[Sign, Verify], &[]),
            Self::Pbkdf2 | Self::Hkdf => (&[DeriveKey, DeriveBits], &[]),
        }
    }

    /// The OpenSSL key type of an asymmetric algorithm.
    fn key_type(self) -> &'static str {
        match self {
            Self::RsassaPkcs1 | Self::RsaOaep | Self::RsaPss => "rsa",
            Self::Ecdsa | Self::Ecdh => "ec",
            Self::Ed25519 => "ed25519",
            Self::Ed448 => "ed448",
            Self::X25519 => "x25519",
            Self::X448 => "x448",
            _ => "secret",
        }
    }
}

/// The digests WebCrypto names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

/// `(hash, WebCrypto name, OpenSSL name, block size in bits)`.
const HASHES: [(Hash, &str, &str, u32); 4] = [
    (Hash::Sha1, "SHA-1", "sha1", 512),
    (Hash::Sha256, "SHA-256", "sha256", 512),
    (Hash::Sha384, "SHA-384", "sha384", 1024),
    (Hash::Sha512, "SHA-512", "sha512", 1024),
];

impl Hash {
    fn name(self) -> &'static str {
        HASHES[self as usize].1
    }

    fn openssl(self) -> &'static str {
        HASHES[self as usize].2
    }

    fn block_bits(self) -> u32 {
        HASHES[self as usize].3
    }

    fn by_name(name: &str) -> Option<Self> {
        HASHES.iter().find(|entry| entry.1.eq_ignore_ascii_case(name)).map(|entry| entry.0)
    }

    /// The JWK `alg` suffix (`256` in `HS256`).
    fn jwk_suffix(self) -> &'static str {
        match self {
            Self::Sha1 => "1",
            Self::Sha256 => "256",
            Self::Sha384 => "384",
            Self::Sha512 => "512",
        }
    }
}

/// The curves WebCrypto names: `(WebCrypto name, OpenSSL name)`.
const CURVES: [(&str, &str); 3] = [("P-256", "prime256v1"), ("P-384", "secp384r1"), ("P-521", "secp521r1")];
/// The AES key sizes in bits.
const AES_KEY_BITS: [u32; 3] = [128, 192, 256];
/// AES-GCM's tag lengths in bits, and the default.
const GCM_TAG_BITS: [u32; 7] = [32, 64, 96, 104, 112, 120, 128];
const GCM_DEFAULT_TAG_BITS: u32 = 128;
const AES_BLOCK_BYTES: usize = 16;
/// AES-CTR's counter length bounds, in bits.
const CTR_MAX_LENGTH_BITS: u32 = 128;
use crate::modules::crypto_secret_job_shared_vm::BITS_PER_BYTE;

/// A CryptoKey's algorithm, as the native layer needs it.
#[derive(Clone)]
pub(crate) struct KeyAlgorithm {
    algorithm: Algorithm,
    hash: Option<Hash>,
    length: Option<u32>,
    curve: Option<&'static str>,
    modulus_length: Option<u32>,
    public_exponent: Option<Vec<u8>>,
    /// Whether `hash` precedes `length` in the guest projection (an imported HMAC key).
    hash_first: bool,
}

impl KeyAlgorithm {
    fn new(algorithm: Algorithm) -> Self {
        Self { algorithm, hash: None, length: None, curve: None, modulus_length: None, public_exponent: None, hash_first: false }
    }
}

/// A CryptoKey's native state.
pub(crate) struct CryptoKeyState {
    key: Arc<Key>,
    algorithm: KeyAlgorithm,
    extractable: bool,
    usages: Vec<Usage>,
}

// ---- CryptoKey -----------------------------------------------------------------------

fn crypto_key_state(c: &mut Context<'_>, value: RootId) -> Result<Option<(Arc<Key>, KeyAlgorithm, bool, Vec<Usage>)>, RootedError> {
    let found = with_native(c, value, |native| match native {
        Native::CryptoKey(state) => Some((state.key.clone(), state.algorithm.clone(), state.extractable, state.usages.clone())),
        _ => None,
    })?;
    Ok(found.flatten())
}

/// `isCryptoKey(value)` (lib/internal/crypto/keys.js).
pub(crate) fn is_crypto_key(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let found = crypto_key_state(c, value)?.is_some();
    Ok(c.boolean(found))
}

/// The KeyObject a CryptoKey holds (`KeyObject.from`), one per CryptoKey.
pub(crate) fn crypto_key_object(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    let Some((key, ..)) = crypto_key_state(c, value)? else {
        return Ok(None);
    };
    if let Some(object) = private_slots::read(c, value, KEY_OBJECT_SLOT)? {
        return Ok(Some(object));
    }
    let object = keys::key_object(c, key)?;
    private_slots::write(c, value, KEY_OBJECT_SLOT, object)?;
    Ok(Some(object))
}

/// A same-realm clone of a CryptoKey.
pub(crate) fn clone_crypto_key(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    match crypto_key_state(c, value)? {
        Some((key, algorithm, extractable, usages)) => crypto_key(c, key, algorithm, extractable, usages).map(Some),
        None => Ok(None),
    }
}

/// A CryptoKey's transport record, without its key material.
pub(crate) fn crypto_key_record(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    let Some((_, algorithm, extractable, usages)) = crypto_key_state(c, value)? else {
        return Ok(None);
    };
    let record = c.null_object_rooted()?;
    let name = c.string_rooted(algorithm.algorithm.name());
    put(c, record, "name", name)?;
    if let Some(hash) = algorithm.hash {
        let hash = c.string_rooted(hash.name());
        put(c, record, "hash", hash)?;
    }
    for (field, number) in [("length", algorithm.length), ("modulusLength", algorithm.modulus_length)] {
        if let Some(number) = number {
            let number = c.number(f64::from(number));
            put(c, record, field, number)?;
        }
    }
    if let Some(curve) = algorithm.curve {
        let curve = c.string_rooted(curve);
        put(c, record, "namedCurve", curve)?;
    }
    if let Some(exponent) = &algorithm.public_exponent {
        let exponent = c.uint8_array_rooted(exponent)?;
        put(c, record, "publicExponent", exponent)?;
    }
    let hash_first = c.boolean(algorithm.hash_first);
    put(c, record, "hashFirst", hash_first)?;
    let extractable = c.boolean(extractable);
    put(c, record, "extractable", extractable)?;
    let names: Vec<String> = usages.iter().map(|usage| usage.name().to_owned()).collect();
    let usages = super::string_array(c, &names)?;
    put(c, record, "usages", usages)?;
    Ok(Some(record))
}

/// The CryptoKey a transport record describes, over the received key.
pub(crate) fn crypto_key_from_record(c: &mut Context<'_>, key: Arc<Key>, record: RootId) -> Outcome {
    let text = |c: &mut Context<'_>, name: &str| -> Result<Option<String>, RootedError> {
        let value = get(c, record, name)?;
        c.string_text(value)
    };
    let number = |c: &mut Context<'_>, name: &str| -> Result<Option<u32>, RootedError> {
        let value = get(c, record, name)?;
        Ok(c.rooted_value(value).and_then(|value| value.as_number()).map(|number| number as u32))
    };
    let name = text(c, "name")?.unwrap_or_default();
    let algorithm_kind = Algorithm::by_name(&name).ok_or_else(|| RootedError::host("unknown CryptoKey algorithm in transport record"))?;
    let mut algorithm = KeyAlgorithm::new(algorithm_kind);
    algorithm.hash = text(c, "hash")?.and_then(|hash| Hash::by_name(&hash));
    algorithm.length = number(c, "length")?;
    algorithm.modulus_length = number(c, "modulusLength")?;
    algorithm.curve = text(c, "namedCurve")?.and_then(|curve| CURVES.iter().find(|(web, _)| *web == curve).map(|(web, _)| *web));
    let exponent = get(c, record, "publicExponent")?;
    algorithm.public_exponent = super::buffer_bytes(c, exponent);
    let hash_first = get(c, record, "hashFirst")?;
    algorithm.hash_first = c.truthy_rooted(hash_first)?;
    let extractable = get(c, record, "extractable")?;
    let extractable = c.truthy_rooted(extractable)?;
    let usages_value = get(c, record, "usages")?;
    let mut usages = Vec::new();
    for item in c.array_like_values_rooted(usages_value)? {
        let item = c.to_string(item)?;
        if let Some(&(usage, _)) = USAGES.iter().find(|(_, known)| *known == item) {
            usages.push(usage);
        }
    }
    crypto_key(c, key, algorithm, extractable, usages)
}

/// The guest projection of a key algorithm (`key.algorithm`).
fn algorithm_object(c: &mut Context<'_>, algorithm: &KeyAlgorithm) -> Outcome {
    let result = c.object_rooted()?;
    let name = c.string_rooted(algorithm.algorithm.name());
    put(c, result, "name", name)?;
    let hash_object = |c: &mut Context<'_>, hash: Hash| -> Outcome {
        let object = c.object_rooted()?;
        let name = c.string_rooted(hash.name());
        put(c, object, "name", name)?;
        Ok(object)
    };
    if let Some(modulus) = algorithm.modulus_length {
        let value = c.number(f64::from(modulus));
        put(c, result, "modulusLength", value)?;
    }
    if let Some(exponent) = &algorithm.public_exponent {
        let value = c.uint8_array_rooted(exponent)?;
        put(c, result, "publicExponent", value)?;
    }
    if algorithm.hash_first {
        if let Some(hash) = algorithm.hash {
            let value = hash_object(c, hash)?;
            put(c, result, "hash", value)?;
        }
    }
    if let Some(length) = algorithm.length {
        let value = c.number(f64::from(length));
        put(c, result, "length", value)?;
    }
    if !algorithm.hash_first {
        if let Some(hash) = algorithm.hash {
            let value = hash_object(c, hash)?;
            put(c, result, "hash", value)?;
        }
    }
    if let Some(curve) = algorithm.curve {
        let value = c.string_rooted(curve);
        put(c, result, "namedCurve", value)?;
    }
    Ok(result)
}

/// A new CryptoKey over `key`; a secret or private key must have a usage.
fn crypto_key(c: &mut Context<'_>, key: Arc<Key>, algorithm: KeyAlgorithm, extractable: bool, usages: Vec<Usage>) -> Outcome {
    if usages.is_empty() && key.type_name() != "public" {
        return Err(errors::dom(c, "Usages cannot be empty when creating a key.", "SyntaxError"));
    }
    let constructor = super::class(c, Class::CryptoKey)?;
    let prototype = get(c, constructor, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let algorithm_value = algorithm_object(c, &algorithm)?;
    private_slots::write(c, object, ALGORITHM_SLOT, algorithm_value)?;
    let names: Vec<String> = usages.iter().map(|usage| usage.name().to_owned()).collect();
    let usages_value = super::string_array(c, &names)?;
    private_slots::write(c, object, USAGES_SLOT, usages_value)?;
    super::attach(c, object, Native::CryptoKey(Box::new(CryptoKeyState { key, algorithm, extractable, usages })))?;
    Ok(object)
}

fn this_crypto_key(c: &mut Context<'_>, this: RootId) -> Result<(Arc<Key>, KeyAlgorithm, bool, Vec<Usage>), RootedError> {
    match crypto_key_state(c, this)? {
        Some(state) => Ok(state),
        None => Err(errors::throw(c, CryptoError::InvalidThis("CryptoKey"))),
    }
}

/// `cryptoKey.type`.
pub(crate) fn key_type(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let (key, ..) = this_crypto_key(c, this)?;
    Ok(c.string_rooted(key.type_name()))
}

/// `cryptoKey.extractable`.
pub(crate) fn key_extractable(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let (_, _, extractable, _) = this_crypto_key(c, this)?;
    Ok(c.boolean(extractable))
}

/// `cryptoKey.algorithm`.
pub(crate) fn key_algorithm(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    this_crypto_key(c, this)?;
    Ok(private_slots::read(c, this, ALGORITHM_SLOT)?.unwrap_or_else(|| c.undefined()))
}

/// `cryptoKey.usages`.
pub(crate) fn key_usages(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    this_crypto_key(c, this)?;
    Ok(private_slots::read(c, this, USAGES_SLOT)?.unwrap_or_else(|| c.undefined()))
}

/// `cryptoKey[util.inspect.custom](depth, options)`: the key's four members.
pub(crate) fn key_inspect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (depth, options) = (arg(c, args, 0), arg(c, args, 1));
    if c.to_number(depth)? < 0.0 {
        return Ok(this);
    }
    let members = c.object_rooted()?;
    for (name, getter) in [("type", key_type as fn(&mut Context<'_>, RootId, &[RootId]) -> Outcome), ("extractable", key_extractable), ("algorithm", key_algorithm), ("usages", key_usages)] {
        let value = getter(c, this, &[])?;
        put(c, members, name, value)?;
    }
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let copy = c.object_rooted()?;
    let copy = super::call_method(c, object, "assign", &[copy, options])?;
    let option_depth = get(c, copy, "depth")?;
    let nested = if is_nullish(c, option_depth) {
        c.null()
    } else {
        let depth = c.to_number(option_depth)?;
        c.number(depth - 1.0)
    };
    put(c, copy, "depth", nested)?;
    let inspect = crate::modules::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let text = c.call_rooted(inspect, undefined, &[members, copy])?;
    let text = c.to_string(text)?;
    Ok(c.string_rooted(&format!("CryptoKey {text}")))
}

/// The constructors WebIDL forbids calling.
pub(crate) fn illegal_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Err(errors::throw(c, CryptoError::IllegalConstructor))
}

// ---- Crypto --------------------------------------------------------------------------

fn require_crypto(c: &mut Context<'_>, this: RootId) -> Result<(), RootedError> {
    let crypto = super::webcrypto(c)?;
    if c.strict_equal_rooted(this, crypto)? {
        Ok(())
    } else {
        Err(errors::throw(c, CryptoError::InvalidThis("Crypto")))
    }
}

/// `crypto.subtle`.
pub(crate) fn subtle(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    require_crypto(c, this)?;
    subtle_instance(c)
}

fn subtle_instance(c: &mut Context<'_>) -> Outcome {
    let crypto = super::webcrypto(c)?;
    if let Some(subtle) = private_slots::read(c, crypto, "\0crypto:subtle")? {
        return Ok(subtle);
    }
    let constructor = super::class(c, Class::SubtleCrypto)?;
    let prototype = get(c, constructor, "prototype")?;
    let subtle = c.object_rooted()?;
    c.set_prototype_rooted(subtle, prototype)?;
    private_slots::write(c, crypto, "\0crypto:subtle", subtle)?;
    Ok(subtle)
}

/// `crypto.getRandomValues(typedArray)`.
pub(crate) fn get_random_values(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    require_crypto(c, this)?;
    let array = arg(c, args, 0);
    super::random::random_values(c, array)
}

/// `crypto.randomUUID()`.
pub(crate) fn random_uuid(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    require_crypto(c, this)?;
    super::random::random_uuid(c, this, &[])
}

/// The `Crypto` instance.
pub(crate) fn create_crypto(c: &mut Context<'_>) -> Outcome {
    let constructor = super::class(c, Class::Crypto)?;
    let prototype = get(c, constructor, "prototype")?;
    let crypto = c.object_rooted()?;
    c.set_prototype_rooted(crypto, prototype)?;
    Ok(crypto)
}

/// The SubtleCrypto methods: `(name, operation, required arguments)`.
const SUBTLE_METHODS: [(&str, &str, usize); 12] = [
    ("encrypt", "cryptoSubtleEncrypt", 3),
    ("decrypt", "cryptoSubtleDecrypt", 3),
    ("sign", "cryptoSubtleSign", 3),
    ("verify", "cryptoSubtleVerify", 4),
    ("digest", "cryptoSubtleDigest", 2),
    ("generateKey", "cryptoSubtleGenerateKey", 3),
    ("deriveKey", "cryptoSubtleDeriveKey", 5),
    ("deriveBits", "cryptoSubtleDeriveBits", 2),
    ("importKey", "cryptoSubtleImportKey", 5),
    ("exportKey", "cryptoSubtleExportKey", 2),
    ("wrapKey", "cryptoSubtleWrapKey", 4),
    ("unwrapKey", "cryptoSubtleUnwrapKey", 7),
];

pub(crate) fn build(c: &mut Context<'_>) -> Result<Vec<(Class, RootId)>, RootedError> {
    let crypto = c.host_constructor_named(crate::host::shared_vm::operation!("cryptoWebIllegalConstructor"), "Crypto")?;
    let prototype = get(c, crypto, "prototype")?;
    define_getters(c, prototype, &[("subtle", "cryptoWebSubtle")], Members::Assigned)?;
    define_methods(c, prototype, &[("getRandomValues", "cryptoWebGetRandomValues"), ("randomUUID", "cryptoWebRandomUuid")], Members::Assigned)?;
    super::to_string_tag(c, prototype, "Crypto")?;

    let subtle = c.host_constructor_named(crate::host::shared_vm::operation!("cryptoWebIllegalConstructor"), "SubtleCrypto")?;
    let prototype = get(c, subtle, "prototype")?;
    let methods: Vec<(&str, &str)> = SUBTLE_METHODS.iter().map(|(name, operation, _)| (*name, *operation)).collect();
    define_methods(c, prototype, &methods, Members::Assigned)?;
    super::to_string_tag(c, prototype, "SubtleCrypto")?;

    let key = c.host_constructor_named(crate::host::shared_vm::operation!("cryptoWebIllegalConstructor"), "CryptoKey")?;
    let prototype = get(c, key, "prototype")?;
    define_getters(
        c,
        prototype,
        &[("type", "cryptoKeyType"), ("extractable", "cryptoKeyExtractable"), ("algorithm", "cryptoKeyAlgorithm"), ("usages", "cryptoKeyUsages")],
        Members::Assigned,
    )?;
    super::to_string_tag(c, prototype, "CryptoKey")?;
    let inspect = c.host_function_named(crate::host::shared_vm::operation!("cryptoKeyInspect"), "[nodejs.util.inspect.custom]")?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let name = c.string_rooted("nodejs.util.inspect.custom");
    let custom = super::call_method(c, symbol, "for", &[name])?;
    c.define_data_property_rooted(prototype, custom, inspect, true, false, true)?;
    Ok(vec![(Class::Crypto, crypto), (Class::SubtleCrypto, subtle), (Class::CryptoKey, key)])
}

// ---- WebIDL conversions --------------------------------------------------------------

fn ordinal(index: usize) -> String {
    let position = index + 1;
    let suffix = match position {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    };
    format!("{position}{suffix}")
}

/// One SubtleCrypto call's context: the method name, for error prefixes.
#[derive(Clone, Copy)]
struct Call {
    method: &'static str,
}

impl Call {
    fn prefix(self) -> String {
        format!("Failed to execute '{}' on 'SubtleCrypto'", self.method)
    }

    /// A `BufferSource` argument's bytes; shared memory is no BufferSource.
    fn buffer_source(self, c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<Vec<u8>, RootedError> {
        const NOT_INSTANCE: &str = "is not instance of ArrayBuffer, Buffer, TypedArray, or DataView.";
        let value = arg(c, args, index);
        let shared = |c: &mut Context<'_>, value: RootId| c.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::SharedArrayBuffer);
        let problem = if shared(c, value)? {
            NOT_INSTANCE
        } else if c.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::ArrayBufferView)? && {
            let buffer = c.get_named_property_rooted(value, "buffer")?;
            shared(c, buffer)?
        } {
            "is a view on a SharedArrayBuffer, which is not allowed."
        } else {
            match super::buffer_bytes(c, value) {
                Some(bytes) => return Ok(bytes),
                None => NOT_INSTANCE,
            }
        };
        let message = format!("{}: {} argument {problem}", self.prefix(), ordinal(index));
        Err(errors::throw(c, CryptoError::InvalidArgTypeMessage(message)))
    }

    fn crypto_key(self, c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<(RootId, Arc<Key>, KeyAlgorithm, bool, Vec<Usage>), RootedError> {
        let value = arg(c, args, index);
        match crypto_key_state(c, value)? {
            Some((key, algorithm, extractable, usages)) => Ok((value, key, algorithm, extractable, usages)),
            None => {
                let message = format!("{}: {} argument is not of type CryptoKey.", self.prefix(), ordinal(index));
                Err(errors::throw(c, CryptoError::InvalidArgTypeMessage(message)))
            }
        }
    }

    fn boolean(self, c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<bool, RootedError> {
        let value = arg(c, args, index);
        c.truthy_rooted(value)
    }

    fn format(self, c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<KeyFormat, RootedError> {
        let value = arg(c, args, index);
        let text = c.to_string(value)?;
        match text.as_str() {
            "raw" => Ok(KeyFormat::Raw),
            "pkcs8" => Ok(KeyFormat::Pkcs8),
            "spki" => Ok(KeyFormat::Spki),
            "jwk" => Ok(KeyFormat::Jwk),
            _ => {
                let message = format!("{}: {} argument value '{text}' is not a valid enum value of type KeyFormat.", self.prefix(), ordinal(index));
                Err(errors::throw(c, CryptoError::InvalidArgValueMessage(message)))
            }
        }
    }

    fn usages(self, c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<Vec<Usage>, RootedError> {
        let value = arg(c, args, index);
        if !c.is_object_rooted(value)? {
            let message = format!("{}: {} argument can not be converted to sequence.", self.prefix(), ordinal(index));
            return Err(errors::throw(c, CryptoError::InvalidArgTypeMessage(message)));
        }
        let mut usages = Vec::new();
        for (position, item) in c.array_like_values_rooted(value)?.into_iter().enumerate() {
            let text = c.to_string(item)?;
            let Some(&(usage, _)) = USAGES.iter().find(|(_, name)| *name == text) else {
                let message = format!("{}: {} argument, index {position} value '{text}' is not a valid enum value of type KeyUsage.", self.prefix(), ordinal(index));
                return Err(errors::throw(c, CryptoError::InvalidArgValueMessage(message)));
            };
            if !usages.contains(&usage) {
                usages.push(usage);
            }
        }
        Ok(usages)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyFormat {
    Raw,
    Pkcs8,
    Spki,
    Jwk,
}

/// The operations an algorithm is normalized for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    GenerateKey,
    Sign,
    Verify,
    ImportKey,
    Encrypt,
    Decrypt,
    DeriveBits,
    GetKeyLength,
    WrapKey,
    UnwrapKey,
}

fn supports(operation: Operation, algorithm: Algorithm) -> bool {
    use Algorithm::*;
    match operation {
        Operation::GenerateKey => !matches!(algorithm, Pbkdf2 | Hkdf),
        Operation::Sign | Operation::Verify => matches!(algorithm, RsassaPkcs1 | RsaPss | Ecdsa | Hmac | Ed25519 | Ed448),
        Operation::ImportKey => true,
        Operation::Encrypt | Operation::Decrypt => matches!(algorithm, RsaOaep | AesCtr | AesCbc | AesGcm),
        Operation::DeriveBits => matches!(algorithm, Ecdh | X25519 | X448 | Hkdf | Pbkdf2),
        Operation::GetKeyLength => matches!(algorithm, AesCtr | AesCbc | AesGcm | AesKw | Hmac | Hkdf | Pbkdf2),
        Operation::WrapKey | Operation::UnwrapKey => matches!(algorithm, RsaOaep | AesCtr | AesCbc | AesGcm | AesKw),
    }
}

/// A normalized algorithm: its identity and the dictionary it was given.
struct Normalized {
    algorithm: Algorithm,
    dictionary: RootId,
}

fn unrecognized(c: &mut Context<'_>) -> RootedError {
    errors::dom(c, "Unrecognized algorithm name", "NotSupportedError")
}

fn missing_member(c: &mut Context<'_>, dictionary: &str, member: &str) -> RootedError {
    let message = format!("Failed to normalize algorithm: passed algorithm can not be converted to '{dictionary}' because '{member}' is required in '{dictionary}'.");
    errors::throw(c, CryptoError::MissingOptionMessage(message))
}

/// The algorithm's name: the string itself, or the dictionary's required `name`.
fn algorithm_name(c: &mut Context<'_>, value: RootId) -> Result<(String, RootId), RootedError> {
    if super::is_string(c, value) {
        let name = c.to_string(value)?;
        let dictionary = c.object_rooted()?;
        put(c, dictionary, "name", value)?;
        return Ok((name, dictionary));
    }
    if !c.is_object_rooted(value)? {
        return Err(unrecognized(c));
    }
    let name = get(c, value, "name")?;
    if is_undefined(c, name) {
        return Err(missing_member(c, "Algorithm", "name"));
    }
    Ok((c.to_string(name)?, value))
}

fn normalize(c: &mut Context<'_>, value: RootId, operation: Operation) -> Result<Normalized, RootedError> {
    let (name, dictionary) = algorithm_name(c, value)?;
    match Algorithm::by_name(&name).filter(|algorithm| supports(operation, *algorithm)) {
        Some(algorithm) => {
            if EXPERIMENTAL_ALGORITHMS.contains(&algorithm) {
                warn_experimental(c, algorithm)?;
            }
            Ok(Normalized { algorithm, dictionary })
        }
        None => Err(unrecognized(c)),
    }
}

/// The algorithms Node marks experimental, each warned about once.
const EXPERIMENTAL_ALGORITHMS: [Algorithm; 2] = [Algorithm::Ed448, Algorithm::X448];

fn warn_experimental(c: &mut Context<'_>, algorithm: Algorithm) -> Result<(), RootedError> {
    let shared = c.host_mut().shared_state();
    if shared.borrow().crypto.experimental_warned.contains(&algorithm) {
        return Ok(());
    }
    shared.borrow_mut().crypto.experimental_warned.push(algorithm);
    let process = shared.borrow().process_module.ok_or_else(|| RootedError::host("process is not initialized"))?;
    let message = format!("The {} Web Crypto API algorithm is an experimental feature and might change at any time", algorithm.name());
    let message = c.string_rooted(&message);
    let kind = c.string_rooted("ExperimentalWarning");
    super::call_method(c, process, "emitWarning", &[message, kind])?;
    Ok(())
}

fn normalize_hash(c: &mut Context<'_>, value: RootId) -> Result<Hash, RootedError> {
    let (name, _) = algorithm_name(c, value)?;
    Hash::by_name(&name).ok_or_else(|| unrecognized(c))
}

/// A required dictionary member.
fn member(c: &mut Context<'_>, normalized: &Normalized, dictionary: &str, name: &str) -> Outcome {
    let value = get(c, normalized.dictionary, name)?;
    if is_undefined(c, value) {
        return Err(missing_member(c, dictionary, name));
    }
    Ok(value)
}

fn hash_member(c: &mut Context<'_>, normalized: &Normalized, dictionary: &str) -> Result<Hash, RootedError> {
    let value = member(c, normalized, dictionary, "hash")?;
    normalize_hash(c, value)
}

fn bytes_member(c: &mut Context<'_>, normalized: &Normalized, dictionary: &str, name: &str) -> Result<Vec<u8>, RootedError> {
    let value = member(c, normalized, dictionary, name)?;
    match super::buffer_bytes(c, value) {
        Some(bytes) => Ok(bytes),
        None => {
            let message = format!("Failed to normalize algorithm: {name} is not instance of ArrayBuffer, Buffer, TypedArray, or DataView.");
            Err(errors::throw(c, CryptoError::InvalidArgTypeMessage(message)))
        }
    }
}

fn optional_bytes_member(c: &mut Context<'_>, normalized: &Normalized, dictionary: &str, name: &str) -> Result<Option<Vec<u8>>, RootedError> {
    let value = get(c, normalized.dictionary, name)?;
    if is_undefined(c, value) {
        return Ok(None);
    }
    bytes_member(c, normalized, dictionary, name).map(Some)
}

fn number_member(c: &mut Context<'_>, normalized: &Normalized, dictionary: &str, name: &str) -> Result<f64, RootedError> {
    let value = member(c, normalized, dictionary, name)?;
    c.to_number(value)
}

fn optional_number_member(c: &mut Context<'_>, normalized: &Normalized, name: &str) -> Result<Option<f64>, RootedError> {
    let value = get(c, normalized.dictionary, name)?;
    if is_undefined(c, value) {
        return Ok(None);
    }
    c.to_number(value).map(Some)
}

fn curve_member(c: &mut Context<'_>, normalized: &Normalized, dictionary: &str) -> Result<&'static str, RootedError> {
    let value = member(c, normalized, dictionary, "namedCurve")?;
    let name = c.to_string(value)?;
    match CURVES.iter().find(|(web, _)| *web == name) {
        Some((web, _)) => Ok(web),
        None => Err(errors::dom(c, "Unrecognized namedCurve", "NotSupportedError")),
    }
}

fn openssl_curve(curve: &str) -> &'static str {
    CURVES.iter().find(|(web, _)| *web == curve).map(|(_, openssl)| *openssl).unwrap_or_default()
}

fn web_curve(openssl: &str) -> Option<&'static str> {
    CURVES.iter().find(|(_, name)| *name == openssl).map(|(web, _)| *web)
}

// ---- Errors --------------------------------------------------------------------------

fn operation_failed(c: &mut Context<'_>) -> RootedError {
    errors::dom(c, "The operation failed for an operation-specific reason", "OperationError")
}

fn data_error(c: &mut Context<'_>, message: &str) -> RootedError {
    errors::dom(c, message, "DataError")
}

/// The usages a new key may have: a subset of `allowed`.
fn check_usages(c: &mut Context<'_>, algorithm: Algorithm, usages: &[Usage], allowed: &[Usage]) -> Result<(), RootedError> {
    if usages.iter().any(|usage| !allowed.contains(usage)) {
        let message = format!("Unsupported key usage for {} key", article_name(algorithm));
        return Err(errors::dom(c, &message, "SyntaxError"));
    }
    Ok(())
}

/// "an HMAC", "an AES", "a ECDSA": how Node names a key family in usage errors.
fn article_name(algorithm: Algorithm) -> String {
    match algorithm {
        Algorithm::Hmac => "an HMAC".to_owned(),
        algorithm if algorithm.is_aes() => "an AES".to_owned(),
        Algorithm::RsaOaep | Algorithm::RsaPss | Algorithm::RsassaPkcs1 => "an RSA".to_owned(),
        algorithm => format!("a {}", algorithm.name()),
    }
}

fn filter_usages(usages: &[Usage], allowed: &[Usage]) -> Vec<Usage> {
    usages.iter().copied().filter(|usage| allowed.contains(usage)).collect()
}

// ---- The SubtleCrypto methods --------------------------------------------------------

/// Runs one SubtleCrypto method: the receiver and argument count checks, then `body`,
/// settling the promise it returns.
fn subtle_call(c: &mut Context<'_>, this: RootId, args: &[RootId], index: usize, body: fn(&mut Context<'_>, Call, &[RootId]) -> Outcome) -> Outcome {
    let (method, _, required) = SUBTLE_METHODS[index];
    let call = Call { method };
    let result = (|| {
        let subtle = subtle_instance(c)?;
        if !c.strict_equal_rooted(this, subtle)? {
            return Err(errors::throw(c, CryptoError::InvalidThis("SubtleCrypto")));
        }
        if args.len() < required {
            let noun = if required == 1 { "argument" } else { "arguments" };
            let message = format!("{}: {required} {noun} required, but only {} present.", call.prefix(), args.len());
            return Err(errors::throw(c, CryptoError::MissingArgsMessage(message)));
        }
        body(c, call, args)
    })();
    super::settled(c, result)
}

macro_rules! subtle_methods {
    ($($function:ident => $index:literal, $body:ident;)*) => {
        $(
            pub(crate) fn $function(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
                subtle_call(c, this, args, $index, $body)
            }
        )*
    };
}

subtle_methods! {
    subtle_encrypt => 0, encrypt_body;
    subtle_decrypt => 1, decrypt_body;
    subtle_sign => 2, sign_body;
    subtle_verify => 3, verify_body;
    subtle_digest => 4, digest_body;
    subtle_generate_key => 5, generate_key_body;
    subtle_derive_key => 6, derive_key_body;
    subtle_derive_bits => 7, derive_bits_body;
    subtle_import_key => 8, import_key_body;
    subtle_export_key => 9, export_key_body;
    subtle_wrap_key => 10, wrap_key_body;
    subtle_unwrap_key => 11, unwrap_key_body;
}

fn digest_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let algorithm = arg(c, args, 0);
    let data = call.buffer_source(c, args, 1)?;
    let hash = normalize_hash(c, algorithm)?;
    let digest = super::hash::digest_by_name(c, hash.openssl()).and_then(|digest| digest.hash(&data, None));
    let bytes = digest.map_err(|_| operation_failed(c))?;
    super::array_buffer(c, bytes)
}

// ---- generateKey ---------------------------------------------------------------------

fn generate_key_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let algorithm = arg(c, args, 0);
    let extractable = call.boolean(c, args, 1)?;
    let usages = call.usages(c, args, 2)?;
    let normalized = normalize(c, algorithm, Operation::GenerateKey)?;
    let kind = normalized.algorithm;
    match kind {
        Algorithm::Hmac => {
            let hash = hash_member(c, &normalized, "HmacKeyGenParams")?;
            let length = optional_number_member(c, &normalized, "length")?.map(|length| length as u32).unwrap_or_else(|| hash.block_bits());
            check_usages(c, kind, &usages, kind.usages().0)?;
            if length == 0 {
                return Err(operation_failed(c));
            }
            let mut algorithm = KeyAlgorithm::new(kind);
            algorithm.hash = Some(hash);
            algorithm.length = Some(length);
            let bytes = random_secret(c, length.div_ceil(BITS_PER_BYTE) as usize)?;
            crypto_key(c, Arc::new(Key::Secret(Material::new(bytes))), algorithm, extractable, usages)
        }
        kind if kind.is_aes() => {
            let length = number_member(c, &normalized, "AesKeyGenParams", "length")? as u32;
            if !AES_KEY_BITS.contains(&length) {
                return Err(errors::dom(c, "AES key length must be 128, 192, or 256 bits", "OperationError"));
            }
            check_usages(c, kind, &usages, kind.usages().0)?;
            let mut algorithm = KeyAlgorithm::new(kind);
            algorithm.length = Some(length);
            let bytes = random_secret(c, (length / BITS_PER_BYTE) as usize)?;
            crypto_key(c, Arc::new(Key::Secret(Material::new(bytes))), algorithm, extractable, usages)
        }
        _ => generate_pair(c, &normalized, extractable, usages),
    }
}

fn random_secret(c: &mut Context<'_>, length: usize) -> Result<Vec<u8>, RootedError> {
    let mut bytes = vec![0; length];
    super::random::fill(&mut bytes).map_err(|stack| errors::openssl(c, stack))?;
    Ok(bytes)
}

fn generate_pair(c: &mut Context<'_>, normalized: &Normalized, extractable: bool, usages: Vec<Usage>) -> Outcome {
    let kind = normalized.algorithm;
    let (public_usages, private_usages) = kind.usages();
    let mut allowed = public_usages.to_vec();
    allowed.extend_from_slice(private_usages);
    let mut algorithm = KeyAlgorithm::new(kind);
    let generation = if kind.is_rsa() {
        let modulus = number_member(c, normalized, "RsaHashedKeyGenParams", "modulusLength")? as u32;
        let exponent = bytes_member(c, normalized, "RsaHashedKeyGenParams", "publicExponent")?;
        let hash = hash_member(c, normalized, "RsaHashedKeyGenParams")?;
        algorithm.modulus_length = Some(modulus);
        algorithm.hash = Some(hash);
        let value = exponent.iter().fold(0u64, |value, byte| (value << BITS_PER_BYTE) | u64::from(*byte));
        algorithm.public_exponent = Some(exponent);
        let Ok(exponent) = u32::try_from(value) else {
            return Err(operation_failed(c));
        };
        Generation::Rsa { bits: modulus, exponent, pss: None }
    } else if kind.is_ec() {
        let curve = curve_member(c, normalized, "EcKeyGenParams")?;
        algorithm.curve = Some(curve);
        Generation::Ec { curve: openssl_curve(curve), explicit: false }
    } else {
        Generation::Named(match kind {
            Algorithm::Ed25519 => "ED25519",
            Algorithm::Ed448 => "ED448",
            Algorithm::X25519 => "X25519",
            _ => "X448",
        })
    };
    check_usages(c, kind, &usages, &allowed)?;
    let private_usage = filter_usages(&usages, private_usages);
    let key = OwnedKey::generate(generation).map_err(|error| keys::key_error(c, error))?;
    let public = key.to_public().map_err(|error| keys::key_error(c, error))?;
    let public = crypto_key(c, Arc::new(Key::Asymmetric(public)), algorithm.clone(), true, filter_usages(&usages, public_usages))?;
    let private = crypto_key(c, Arc::new(Key::Asymmetric(key)), algorithm, extractable, private_usage)?;
    let pair = c.null_object_rooted()?;
    put(c, pair, "publicKey", public)?;
    put(c, pair, "privateKey", private)?;
    Ok(pair)
}

// ---- importKey -----------------------------------------------------------------------

/// What an import produced: the key, its algorithm, and whether HMAC's import ordering
/// of `algorithm` members applies.
struct Imported {
    key: Arc<Key>,
    algorithm: KeyAlgorithm,
}

fn import_key_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let format = call.format(c, args, 0)?;
    let key_data = arg(c, args, 1);
    let data = if format == KeyFormat::Jwk {
        if !c.is_object_rooted(key_data)? {
            let message = format!("{}: 2nd argument can not be converted to a dictionary", call.prefix());
            return Err(errors::throw(c, CryptoError::InvalidArgTypeMessage(message)));
        }
        None
    } else {
        Some(call.buffer_source(c, args, 1)?)
    };
    let algorithm = arg(c, args, 2);
    let extractable = call.boolean(c, args, 3)?;
    let usages = call.usages(c, args, 4)?;
    let normalized = normalize(c, algorithm, Operation::ImportKey)?;
    import_key(c, format, data, key_data, &normalized, extractable, usages)
}

fn import_key(c: &mut Context<'_>, format: KeyFormat, data: Option<Vec<u8>>, jwk: RootId, normalized: &Normalized, extractable: bool, usages: Vec<Usage>) -> Outcome {
    let kind = normalized.algorithm;
    let (public_usages, private_usages) = kind.usages();
    let imported = match kind {
        Algorithm::Hmac => {
            let hash = hash_member(c, normalized, "HmacImportParams")?;
            check_usages(c, kind, &usages, public_usages)?;
            let bytes = secret_input(c, format, data, jwk, kind)?;
            if bytes.is_empty() {
                return Err(data_error(c, "Zero-length key is not supported"));
            }
            let data_bits = bytes.len() as u32 * BITS_PER_BYTE;
            let length = match optional_number_member(c, normalized, "length")? {
                Some(0.0) => return Err(data_error(c, "Zero-length key is not supported")),
                Some(length) if !(length as u32).is_multiple_of(BITS_PER_BYTE) => return Err(errors::dom(c, "Unsupported algorithm.length", "NotSupportedError")),
                Some(length) if length as u32 != data_bits => return Err(data_error(c, "Invalid key length")),
                _ => data_bits,
            };
            let mut algorithm = KeyAlgorithm::new(kind);
            algorithm.hash = Some(hash);
            algorithm.length = Some(length);
            algorithm.hash_first = true;
            Imported { key: Arc::new(Key::Secret(Material::new(bytes))), algorithm }
        }
        kind if kind.is_aes() => {
            check_usages(c, kind, &usages, public_usages)?;
            let bytes = secret_input(c, format, data, jwk, kind)?;
            if !AES_KEY_BITS.contains(&(bytes.len() as u32 * BITS_PER_BYTE)) {
                return Err(data_error(c, "Invalid key length"));
            }
            let mut algorithm = KeyAlgorithm::new(kind);
            algorithm.length = Some(bytes.len() as u32 * BITS_PER_BYTE);
            Imported { key: Arc::new(Key::Secret(Material::new(bytes))), algorithm }
        }
        Algorithm::Pbkdf2 | Algorithm::Hkdf => {
            if format != KeyFormat::Raw {
                return Err(errors::dom(c, &unimportable(kind, format), "NotSupportedError"));
            }
            check_usages(c, kind, &usages, public_usages)?;
            if extractable {
                return Err(errors::dom(c, &format!("{} keys are not extractable", kind.name()), "SyntaxError"));
            }
            let bytes = data.unwrap_or_default();
            Imported { key: Arc::new(Key::Secret(Material::new(bytes))), algorithm: KeyAlgorithm::new(kind) }
        }
        _ => import_asymmetric(c, format, data, jwk, normalized)?,
    };
    let allowed = match imported.key.type_name() {
        "private" => private_usages,
        _ => public_usages,
    };
    check_usages(c, kind, &usages, allowed)?;
    if format == KeyFormat::Jwk {
        check_jwk(c, jwk, &imported.algorithm, extractable, &usages)?;
    }
    crypto_key(c, imported.key, imported.algorithm, extractable, usages)
}

/// The JWK `use` an algorithm's keys declare.
fn jwk_use(algorithm: Algorithm) -> &'static str {
    match algorithm {
        Algorithm::RsaOaep | Algorithm::Ecdh | Algorithm::X25519 | Algorithm::X448 => "enc",
        algorithm if algorithm.is_aes() => "enc",
        _ => "sig",
    }
}

/// The JWK `alg` an imported key must declare when it declares one; `None` when any is
/// accepted.
fn expected_jwk_alg(algorithm: &KeyAlgorithm) -> Option<Vec<String>> {
    Some(match algorithm.algorithm {
        Algorithm::Ecdsa => vec![match algorithm.curve {
            Some("P-384") => "ES384".to_owned(),
            Some("P-521") => "ES512".to_owned(),
            _ => "ES256".to_owned(),
        }],
        Algorithm::Ed25519 | Algorithm::Ed448 => vec![algorithm.algorithm.name().to_owned(), "EdDSA".to_owned()],
        Algorithm::Ecdh | Algorithm::X25519 | Algorithm::X448 | Algorithm::Pbkdf2 | Algorithm::Hkdf => return None,
        _ => vec![jwk_alg(algorithm)?],
    })
}

/// The JWK members every import checks: `use`, `key_ops`, `ext` and `alg`.
fn check_jwk(c: &mut Context<'_>, jwk: RootId, algorithm: &KeyAlgorithm, extractable: bool, usages: &[Usage]) -> Result<(), RootedError> {
    let declared_use = get(c, jwk, "use")?;
    if !is_undefined(c, declared_use) && c.string_text(declared_use)?.as_deref() != Some(jwk_use(algorithm.algorithm)) {
        return Err(data_error(c, "Invalid JWK \"use\" Parameter"));
    }
    let operations = get(c, jwk, "key_ops")?;
    if !is_undefined(c, operations) {
        let mut declared = Vec::new();
        for operation in c.array_like_values_rooted(operations)? {
            let operation = c.to_string(operation)?;
            if declared.contains(&operation) {
                return Err(data_error(c, "Duplicate key operation"));
            }
            declared.push(operation);
        }
        if usages.iter().any(|usage| !declared.iter().any(|operation| operation == usage.name())) {
            return Err(data_error(c, "Key operations and usage mismatch"));
        }
    }
    let ext = get(c, jwk, "ext")?;
    if !is_undefined(c, ext) && !c.truthy_rooted(ext)? && extractable {
        return Err(data_error(c, "JWK \"ext\" Parameter and extractable mismatch"));
    }
    let alg = get(c, jwk, "alg")?;
    if !is_undefined(c, alg) {
        if let Some(expected) = expected_jwk_alg(algorithm) {
            let alg = c.to_string(alg)?;
            if !expected.contains(&alg) {
                return Err(data_error(c, "JWK \"alg\" does not match the requested algorithm"));
            }
        }
    }
    Ok(())
}

fn unimportable(kind: Algorithm, format: KeyFormat) -> String {
    format!("Unable to import {} using {} format", kind.name(), format_name(format))
}

fn format_name(format: KeyFormat) -> &'static str {
    match format {
        KeyFormat::Raw => "raw",
        KeyFormat::Pkcs8 => "pkcs8",
        KeyFormat::Spki => "spki",
        KeyFormat::Jwk => "jwk",
    }
}

/// A secret key's bytes from `raw` data or an `oct` JWK.
fn secret_input(c: &mut Context<'_>, format: KeyFormat, data: Option<Vec<u8>>, jwk: RootId, kind: Algorithm) -> Result<Vec<u8>, RootedError> {
    match format {
        KeyFormat::Raw => Ok(data.unwrap_or_default()),
        KeyFormat::Jwk => {
            let kty = get(c, jwk, "kty")?;
            if c.string_text(kty)?.as_deref() != Some("oct") {
                return Err(data_error(c, "Invalid JWK \"kty\" Parameter"));
            }
            let k = get(c, jwk, "k")?;
            match c.string_text(k)? {
                Some(text) => Ok(crate::modules::buffer_codec::base64_decode(text.as_bytes())),
                None => Err(data_error(c, "Invalid JWK secret key format")),
            }
        }
        _ => Err(errors::dom(c, &unimportable(kind, format), "NotSupportedError")),
    }
}

fn import_asymmetric(c: &mut Context<'_>, format: KeyFormat, data: Option<Vec<u8>>, jwk: RootId, normalized: &Normalized) -> Result<Imported, RootedError> {
    let kind = normalized.algorithm;
    let mut algorithm = KeyAlgorithm::new(kind);
    let curve = if kind.is_ec() { Some(curve_member(c, normalized, "EcKeyImportParams")?) } else { None };
    if kind.is_rsa() {
        algorithm.hash = Some(hash_member(c, normalized, "RsaHashedImportParams")?);
    }
    let failed = |c: &mut Context<'_>| data_error(c, "Invalid keyData");
    let owned = match format {
        KeyFormat::Spki | KeyFormat::Pkcs8 => {
            let (kind_of, encoding) = if format == KeyFormat::Spki { (KeyKind::Public, Encoding::Spki) } else { (KeyKind::Private, Encoding::Pkcs8) };
            let bytes = data.unwrap_or_default();
            OwnedKey::import(kind_of, &bytes, Format::Der, Some(encoding), None, None).map_err(|_| failed(c))?
        }
        KeyFormat::Raw => {
            let bytes = data.unwrap_or_default();
            if kind.is_ec() {
                ec_public_from_raw(curve.unwrap_or_default(), &bytes).ok_or_else(|| failed(c))?
            } else if kind.is_okp() {
                let id = match kind {
                    Algorithm::Ed25519 => openssl::pkey::Id::ED25519,
                    Algorithm::Ed448 => openssl::pkey::Id::ED448,
                    Algorithm::X25519 => openssl::pkey::Id::X25519,
                    _ => openssl::pkey::Id::X448,
                };
                let key = openssl::pkey::PKey::public_key_from_raw_bytes(&bytes, id).map_err(|_| failed(c))?;
                OwnedKey::Public(key)
            } else {
                return Err(errors::dom(c, &unimportable(kind, KeyFormat::Raw), "NotSupportedError"));
            }
        }
        KeyFormat::Jwk => {
            let d = get(c, jwk, "d")?;
            let private = !is_undefined(c, d);
            let kind_of = if private { KeyKind::Private } else { KeyKind::Public };
            match keys::import_jwk(c, jwk, kind_of) {
                Ok(Key::Asymmetric(owned)) => owned,
                _ => return Err(failed(c)),
            }
        }
    };
    let actual = owned.key_type().unwrap_or_default();
    if actual != kind.key_type() && !(kind == Algorithm::RsaPss && actual == "rsa-pss") {
        return Err(data_error(c, "Invalid key type"));
    }
    if let Some(curve) = curve {
        let details = owned.details().map_err(|error| keys::key_error(c, error))?;
        let named = details.named_curve.as_deref().and_then(web_curve);
        if named != Some(curve) {
            return Err(data_error(c, "Named curve mismatch"));
        }
        algorithm.curve = Some(curve);
    }
    if kind.is_rsa() {
        let details = owned.details().map_err(|error| keys::key_error(c, error))?;
        algorithm.modulus_length = details.modulus_length;
        algorithm.public_exponent = details.public_exponent;
    }
    Ok(Imported { key: Arc::new(Key::Asymmetric(owned)), algorithm })
}

fn ec_public_from_raw(curve: &str, bytes: &[u8]) -> Option<OwnedKey> {
    use openssl::ec::{EcGroup, EcKey, EcPoint};
    let nid = openssl::nid::Nid::from_raw(unsafe { openssl_sys::OBJ_sn2nid(std::ffi::CString::new(openssl_curve(curve)).ok()?.as_ptr()) });
    let group = EcGroup::from_curve_name(nid).ok()?;
    let mut context = openssl::bn::BigNumContext::new().ok()?;
    let point = EcPoint::from_bytes(&group, bytes, &mut context).ok()?;
    let key = EcKey::from_public_key(&group, &point).ok()?;
    Some(OwnedKey::Public(openssl::pkey::PKey::from_ec_key(key).ok()?))
}

// ---- exportKey -----------------------------------------------------------------------

fn export_key_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let format = call.format(c, args, 0)?;
    let (_, key, algorithm, extractable, usages) = call.crypto_key(c, args, 1)?;
    if !extractable {
        return Err(errors::dom(c, "key is not extractable", "InvalidAccessException"));
    }
    export_key(c, format, &key, &algorithm, &usages)
}

fn export_bytes(c: &mut Context<'_>, format: KeyFormat, key: &Key, algorithm: &KeyAlgorithm) -> Result<Vec<u8>, RootedError> {
    let unsupported = |c: &mut Context<'_>| {
        let message = format!("Unable to export {} {} key using {} format", algorithm.algorithm.name(), key.type_name(), format_name(format));
        errors::dom(c, &message, "NotSupportedError")
    };
    match (format, key) {
        (KeyFormat::Raw, Key::Secret(bytes)) => Ok(bytes.to_vec()),
        (KeyFormat::Raw, Key::Asymmetric(owned)) if owned.kind() == KeyKind::Public && (algorithm.algorithm.is_ec() || algorithm.algorithm.is_okp()) => {
            owned.raw_public(false).map_err(|error| keys::key_error(c, error))
        }
        (KeyFormat::Raw, Key::Asymmetric(_)) => {
            let message = format!("Unable to export {} {} key using raw format", algorithm.algorithm.name(), key.type_name());
            Err(errors::dom(c, &message, "NotSupportedError"))
        }
        (KeyFormat::Spki, Key::Asymmetric(owned)) if owned.kind() == KeyKind::Public => {
            owned.export(Format::Der, Encoding::Spki, None, None).map_err(|error| keys::key_error(c, error))
        }
        (KeyFormat::Pkcs8, Key::Asymmetric(owned)) if owned.kind() == KeyKind::Private => {
            owned.export(Format::Der, Encoding::Pkcs8, None, None).map_err(|error| keys::key_error(c, error))
        }
        _ => Err(unsupported(c)),
    }
}

/// The JWK `alg` a key's algorithm implies.
fn jwk_alg(algorithm: &KeyAlgorithm) -> Option<String> {
    let suffix = algorithm.hash.map(Hash::jwk_suffix).unwrap_or_default();
    let bits = algorithm.length.unwrap_or_default();
    Some(match algorithm.algorithm {
        Algorithm::Hmac => format!("HS{suffix}"),
        Algorithm::AesGcm => format!("A{bits}GCM"),
        Algorithm::AesCbc => format!("A{bits}CBC"),
        Algorithm::AesCtr => format!("A{bits}CTR"),
        Algorithm::AesKw => format!("A{bits}KW"),
        Algorithm::RsassaPkcs1 => format!("RS{suffix}"),
        Algorithm::RsaPss => format!("PS{suffix}"),
        Algorithm::RsaOaep if algorithm.hash == Some(Hash::Sha1) => "RSA-OAEP".to_owned(),
        Algorithm::RsaOaep => format!("RSA-OAEP-{suffix}"),
        Algorithm::Ed25519 => "Ed25519".to_owned(),
        Algorithm::Ed448 => "Ed448".to_owned(),
        _ => return None,
    })
}

fn export_key(c: &mut Context<'_>, format: KeyFormat, key: &Key, algorithm: &KeyAlgorithm, usages: &[Usage]) -> Outcome {
    if format != KeyFormat::Jwk {
        let bytes = export_bytes(c, format, key, algorithm)?;
        return super::array_buffer(c, bytes);
    }
    let fields = keys::export_jwk(c, key)?;
    let result = c.object_rooted()?;
    let names: Vec<String> = usages.iter().map(|usage| usage.name().to_owned()).collect();
    let key_ops = super::string_array(c, &names)?;
    put(c, result, "key_ops", key_ops)?;
    let ext = c.boolean(true);
    put(c, result, "ext", ext)?;
    if let Some(alg) = jwk_alg(algorithm) {
        let alg = c.string_rooted(&alg);
        put(c, result, "alg", alg)?;
    }
    for name in c.enumerable_own_keys_rooted(fields, false, false)? {
        let value = c.get_property_rooted(fields, name)?;
        c.set_property_rooted(result, name, value, result)?;
    }
    if algorithm.algorithm.is_ec() {
        if let Some(curve) = algorithm.curve {
            let curve = c.string_rooted(curve);
            put(c, result, "crv", curve)?;
        }
    }
    Ok(result)
}

// ---- sign / verify -------------------------------------------------------------------

fn usable(c: &mut Context<'_>, usages: &[Usage], usage: Usage, normalized: &Normalized, algorithm: &KeyAlgorithm) -> Result<(), RootedError> {
    if !usages.contains(&usage) || normalized.algorithm != algorithm.algorithm {
        let message = format!("Unable to use this key to {}", usage.name());
        return Err(errors::dom(c, &message, "InvalidAccessError"));
    }
    Ok(())
}

/// The digest and signature options of a signing algorithm over a key.
fn signature_settings(c: &mut Context<'_>, normalized: &Normalized, algorithm: &KeyAlgorithm) -> Result<(Option<Hash>, crate::modules::crypto_asymmetric_operations::SignatureOptions), RootedError> {
    use crate::modules::crypto_asymmetric_operations::SignatureOptions;
    let mut options = SignatureOptions { padding: None, salt_length: None, ieee_p1363: false };
    let hash = match normalized.algorithm {
        Algorithm::Ecdsa => {
            options.ieee_p1363 = true;
            Some(hash_member(c, normalized, "EcdsaParams")?)
        }
        Algorithm::RsaPss => {
            options.padding = Some(openssl::rsa::Padding::PKCS1_PSS);
            options.salt_length = Some(number_member(c, normalized, "RsaPssParams", "saltLength")? as i32);
            algorithm.hash
        }
        Algorithm::RsassaPkcs1 => {
            options.padding = Some(openssl::rsa::Padding::PKCS1);
            algorithm.hash
        }
        _ => None,
    };
    Ok((hash, options))
}

fn hmac(c: &mut Context<'_>, key: &Key, algorithm: &KeyAlgorithm, data: &[u8]) -> Result<Vec<u8>, RootedError> {
    let hash = algorithm.hash.unwrap_or(Hash::Sha256);
    let digest = super::hash::digest_by_name(c, hash.openssl()).map_err(|_| operation_failed(c))?;
    digest.hmac(key.secret().unwrap_or_default(), data).map_err(|_| operation_failed(c))
}

fn sign_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let algorithm_value = arg(c, args, 0);
    let (_, key, algorithm, _, usages) = call.crypto_key(c, args, 1)?;
    let data = call.buffer_source(c, args, 2)?;
    let normalized = normalize(c, algorithm_value, Operation::Sign)?;
    usable(c, &usages, Usage::Sign, &normalized, &algorithm)?;
    let signature = if normalized.algorithm == Algorithm::Hmac {
        hmac(c, &key, &algorithm, &data)?
    } else {
        let (hash, options) = signature_settings(c, &normalized, &algorithm)?;
        let digest = match hash {
            Some(hash) => Some(super::hash::digest_by_name(c, hash.openssl()).map_err(|_| operation_failed(c))?),
            None => None,
        };
        let owned = key.asymmetric().ok_or_else(|| operation_failed(c))?;
        super::sign::sign_message(owned, digest.as_deref(), &data, &options).map_err(|_| operation_failed(c))?
    };
    super::array_buffer(c, signature)
}

fn verify_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let algorithm_value = arg(c, args, 0);
    let (_, key, algorithm, _, usages) = call.crypto_key(c, args, 1)?;
    let signature = call.buffer_source(c, args, 2)?;
    let data = call.buffer_source(c, args, 3)?;
    let normalized = normalize(c, algorithm_value, Operation::Verify)?;
    usable(c, &usages, Usage::Verify, &normalized, &algorithm)?;
    let verified = if normalized.algorithm == Algorithm::Hmac {
        let expected = hmac(c, &key, &algorithm, &data)?;
        expected.len() == signature.len() && openssl::memcmp::eq(&expected, &signature)
    } else {
        let (hash, options) = signature_settings(c, &normalized, &algorithm)?;
        let digest = match hash {
            Some(hash) => Some(super::hash::digest_by_name(c, hash.openssl()).map_err(|_| operation_failed(c))?),
            None => None,
        };
        let owned = key.asymmetric().ok_or_else(|| operation_failed(c))?;
        super::sign::verify_message(owned, digest.as_deref(), &data, &signature, &options).unwrap_or(false)
    };
    Ok(c.boolean(verified))
}

// ---- encrypt / decrypt ---------------------------------------------------------------

fn cipher_usable(c: &mut Context<'_>, usages: &[Usage], usage: Usage, normalized: &Normalized, algorithm: &KeyAlgorithm) -> Result<(), RootedError> {
    if normalized.algorithm != algorithm.algorithm || !usages.contains(&usage) {
        return Err(errors::dom(c, "The requested operation is not valid for the provided key", "InvalidAccessError"));
    }
    Ok(())
}

/// The AES modes WebCrypto runs, and the raw block mode AES-CTR's wrapping counter needs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AesMode {
    Ecb,
    Cbc,
    Ctr,
    Gcm,
    Wrap,
}

impl AesMode {
    /// The OpenSSL cipher for a key of `bits`.
    fn cipher(self, bits: u32) -> String {
        match self {
            Self::Ecb => format!("aes-{bits}-ecb"),
            Self::Cbc => format!("aes-{bits}-cbc"),
            Self::Ctr => format!("aes-{bits}-ctr"),
            Self::Gcm => format!("aes-{bits}-gcm"),
            Self::Wrap => format!("id-aes{bits}-wrap"),
        }
    }
}

fn key_bits(key: &[u8]) -> u32 {
    key.len() as u32 * BITS_PER_BYTE
}

/// One whole AES operation in a non-AEAD mode.
fn run_cipher(mode: AesMode, key: &[u8], iv: Option<&[u8]>, data: &[u8], decrypt: bool) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    let cipher = openssl::cipher::Cipher::fetch(None, &mode.cipher(key_bits(key)), None)?;
    let mut context = openssl::cipher_ctx::CipherCtx::new()?;
    if mode == AesMode::Wrap {
        // SAFETY: the context is live; key wrap must be allowed before use.
        unsafe { openssl_sys::EVP_CIPHER_CTX_set_flags(foreign_types::ForeignTypeRef::as_ptr(&*context), openssl_sys::EVP_CIPHER_CTX_FLAG_WRAP_ALLOW) };
    }
    if decrypt {
        context.decrypt_init(Some(&cipher), Some(key), iv)?;
    } else {
        context.encrypt_init(Some(&cipher), Some(key), iv)?;
    }
    context.set_padding(mode == AesMode::Cbc);
    let mut output = Vec::new();
    context.cipher_update_vec(data, &mut output)?;
    context.cipher_final_vec(&mut output)?;
    Ok(output)
}

fn aes_gcm(key: &[u8], iv: &[u8], aad: Option<&[u8]>, tag_bytes: usize, data: &[u8], decrypt: bool) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    let cipher = openssl::cipher::Cipher::fetch(None, &AesMode::Gcm.cipher(key_bits(key)), None)?;
    let mut context = openssl::cipher_ctx::CipherCtx::new()?;
    if decrypt {
        context.decrypt_init(Some(&cipher), None, None)?;
    } else {
        context.encrypt_init(Some(&cipher), None, None)?;
    }
    context.set_iv_length(iv.len())?;
    if decrypt {
        context.decrypt_init(None, Some(key), Some(iv))?;
    } else {
        context.encrypt_init(None, Some(key), Some(iv))?;
    }
    if let Some(aad) = aad {
        context.cipher_update(aad, None)?;
    }
    let mut output = Vec::new();
    if decrypt {
        if data.len() < tag_bytes {
            return Err(openssl::error::ErrorStack::get());
        }
        let (body, tag) = data.split_at(data.len() - tag_bytes);
        context.set_tag(tag)?;
        context.cipher_update_vec(body, &mut output)?;
        context.cipher_final_vec(&mut output)?;
    } else {
        context.cipher_update_vec(data, &mut output)?;
        context.cipher_final_vec(&mut output)?;
        let mut tag = vec![0; tag_bytes];
        context.tag(&mut tag)?;
        output.extend(tag);
    }
    Ok(output)
}

/// AES-CTR with a counter of `length` bits: the counter block wraps within those bits.
fn aes_ctr(key: &[u8], counter: &[u8], length: u32, data: &[u8]) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    if length >= CTR_MAX_LENGTH_BITS {
        return run_cipher(AesMode::Ctr, key, Some(counter), data, false);
    }
    // Encrypt block by block so the low `length` bits wrap instead of carrying.
    let mut block = counter.to_vec();
    let mut output = Vec::with_capacity(data.len());
    for chunk in data.chunks(AES_BLOCK_BYTES) {
        let stream = run_cipher(AesMode::Ecb, key, None, &block, false)?;
        output.extend(chunk.iter().zip(stream).map(|(byte, key)| byte ^ key));
        increment_counter(&mut block, length);
    }
    Ok(output)
}

fn increment_counter(block: &mut [u8], length: u32) {
    let mut remaining = length;
    for byte in block.iter_mut().rev() {
        if remaining == 0 {
            break;
        }
        let width = remaining.min(BITS_PER_BYTE);
        let mask = if width == BITS_PER_BYTE { u8::MAX } else { (1u8 << width) - 1 };
        let next = (*byte & mask).wrapping_add(1) & mask;
        *byte = (*byte & !mask) | next;
        remaining -= width;
        if next != 0 {
            break;
        }
    }
}

fn cipher_operation(c: &mut Context<'_>, normalized: &Normalized, key: &Key, algorithm: &KeyAlgorithm, data: &[u8], decrypt: bool) -> Result<Vec<u8>, RootedError> {
    match normalized.algorithm {
        Algorithm::RsaOaep => {
            use crate::modules::crypto_asymmetric_operations::{rsa, RsaOperation, RsaOptions};
            let label = optional_bytes_member(c, normalized, "RsaOaepParams", "label")?;
            let hash = algorithm.hash.unwrap_or(Hash::Sha1);
            let digest = crate::modules::crypto_digest::Digest::fetch(hash.openssl()).map_err(|_| operation_failed(c))?;
            let owned = key.asymmetric().ok_or_else(|| operation_failed(c))?;
            let options = RsaOptions { padding: openssl::rsa::Padding::PKCS1_OAEP, digest: Some(&digest), mgf1_digest: Some(&digest), label: label.as_deref().filter(|label| !label.is_empty()) };
            let operation = if decrypt { RsaOperation::PrivateDecrypt } else { RsaOperation::PublicEncrypt };
            rsa(owned, operation, data, &options).map_err(|_| operation_failed(c))
        }
        Algorithm::AesCbc => {
            let iv = bytes_member(c, normalized, "AesCbcParams", "iv")?;
            if iv.len() != AES_BLOCK_BYTES {
                return Err(errors::dom(c, "algorithm.iv must contain exactly 16 bytes", "OperationError"));
            }
            run_cipher(AesMode::Cbc, key.secret().unwrap_or_default(), Some(&iv), data, decrypt).map_err(|_| operation_failed(c))
        }
        Algorithm::AesCtr => {
            let counter = bytes_member(c, normalized, "AesCtrParams", "counter")?;
            let length = number_member(c, normalized, "AesCtrParams", "length")? as u32;
            if counter.len() != AES_BLOCK_BYTES {
                return Err(errors::dom(c, "algorithm.counter must contain exactly 16 bytes", "OperationError"));
            }
            if length == 0 || length > CTR_MAX_LENGTH_BITS {
                return Err(errors::dom(c, "AES-CTR algorithm.length must be between 1 and 128", "OperationError"));
            }
            aes_ctr(key.secret().unwrap_or_default(), &counter, length, data).map_err(|_| operation_failed(c))
        }
        Algorithm::AesGcm => {
            let iv = bytes_member(c, normalized, "AesGcmParams", "iv")?;
            let aad = optional_bytes_member(c, normalized, "AesGcmParams", "additionalData")?;
            let tag_bits = optional_number_member(c, normalized, "tagLength")?.map(|bits| bits as u32).unwrap_or(GCM_DEFAULT_TAG_BITS);
            if !GCM_TAG_BITS.contains(&tag_bits) {
                let message = format!("{tag_bits} is not a valid AES-GCM tag length");
                return Err(errors::dom(c, &message, "OperationError"));
            }
            let tag_bytes = (tag_bits / BITS_PER_BYTE) as usize;
            aes_gcm(key.secret().unwrap_or_default(), &iv, aad.as_deref(), tag_bytes, data, decrypt).map_err(|_| operation_failed(c))
        }
        Algorithm::AesKw => {
            run_cipher(AesMode::Wrap, key.secret().unwrap_or_default(), None, data, decrypt).map_err(|_| operation_failed(c))
        }
        _ => Err(unrecognized(c)),
    }
}

fn encrypt_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    cipher_body(c, call, args, false)
}

fn decrypt_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    cipher_body(c, call, args, true)
}

fn cipher_body(c: &mut Context<'_>, call: Call, args: &[RootId], decrypt: bool) -> Outcome {
    let algorithm_value = arg(c, args, 0);
    let (_, key, algorithm, _, usages) = call.crypto_key(c, args, 1)?;
    let data = call.buffer_source(c, args, 2)?;
    let operation = if decrypt { Operation::Decrypt } else { Operation::Encrypt };
    let normalized = normalize(c, algorithm_value, operation)?;
    let usage = if decrypt { Usage::Decrypt } else { Usage::Encrypt };
    cipher_usable(c, &usages, usage, &normalized, &algorithm)?;
    let output = cipher_operation(c, &normalized, &key, &algorithm, &data, decrypt)?;
    super::array_buffer(c, output)
}

// ---- deriveBits / deriveKey ----------------------------------------------------------

fn derive_bits(c: &mut Context<'_>, normalized: &Normalized, key: &Key, algorithm: &KeyAlgorithm, length: Option<u32>) -> Result<Vec<u8>, RootedError> {
    if normalized.algorithm != algorithm.algorithm {
        return Err(errors::dom(c, "Key algorithm mismatch", "InvalidAccessError"));
    }
    match normalized.algorithm {
        Algorithm::Hkdf | Algorithm::Pbkdf2 => {
            let Some(length) = length else {
                return Err(errors::dom(c, "length cannot be null", "OperationError"));
            };
            if length % BITS_PER_BYTE != 0 {
                return Err(errors::dom(c, "length must be a multiple of 8", "OperationError"));
            }
            let bytes = (length / BITS_PER_BYTE) as usize;
            let secret = key.secret().unwrap_or_default();
            if normalized.algorithm == Algorithm::Hkdf {
                let hash = hash_member(c, normalized, "HkdfParams")?;
                let salt = bytes_member(c, normalized, "HkdfParams", "salt")?;
                let info = bytes_member(c, normalized, "HkdfParams", "info")?;
                super::kdf::hkdf_derive(c, hash.openssl(), secret, &salt, &info, bytes).map_err(|_| operation_failed(c))
            } else {
                let hash = hash_member(c, normalized, "Pbkdf2Params")?;
                let salt = bytes_member(c, normalized, "Pbkdf2Params", "salt")?;
                let iterations = number_member(c, normalized, "Pbkdf2Params", "iterations")? as u32;
                if iterations == 0 {
                    return Err(errors::dom(c, "iterations cannot be zero", "OperationError"));
                }
                super::kdf::pbkdf2_derive(c, hash.openssl(), secret, &salt, iterations, bytes).map_err(|_| operation_failed(c))
            }
        }
        _ => {
            let public = member(c, normalized, "EcdhKeyDeriveParams", "public")?;
            let Some((public_key, public_algorithm, ..)) = crypto_key_state(c, public)? else {
                let message = "Failed to normalize algorithm: algorithm.public is not of type CryptoKey.";
                return Err(errors::throw(c, CryptoError::InvalidArgTypeMessage(message.to_owned())));
            };
            if public_key.type_name() != "public" {
                return Err(errors::dom(c, "algorithm.public must be a public key", "InvalidAccessError"));
            }
            if public_algorithm.algorithm != algorithm.algorithm {
                return Err(errors::dom(c, "The public and private keys must be of the same type", "InvalidAccessError"));
            }
            if public_algorithm.curve != algorithm.curve {
                return Err(errors::dom(c, "Named curve mismatch", "InvalidAccessError"));
            }
            let (Some(private), Some(public)) = (key.asymmetric(), public_key.asymmetric()) else {
                return Err(operation_failed(c));
            };
            let secret = super::dh::derive(private, public).map_err(|_| operation_failed(c))?;
            match length {
                None => Ok(secret),
                Some(length) if length as usize > secret.len() * BITS_PER_BYTE as usize => Err(errors::dom(c, "derived bit length is too small", "OperationError")),
                Some(length) => {
                    let mut bits = secret[..(length.div_ceil(BITS_PER_BYTE)) as usize].to_vec();
                    let spare = length % BITS_PER_BYTE;
                    if spare != 0 {
                        if let Some(last) = bits.last_mut() {
                            *last &= u8::MAX << (BITS_PER_BYTE - spare);
                        }
                    }
                    Ok(bits)
                }
            }
        }
    }
}

fn length_argument(c: &mut Context<'_>, value: RootId) -> Result<Option<u32>, RootedError> {
    if is_nullish(c, value) {
        return Ok(None);
    }
    Ok(Some(c.to_number(value)? as u32))
}

fn derive_bits_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let algorithm_value = arg(c, args, 0);
    let (_, key, algorithm, _, usages) = call.crypto_key(c, args, 1)?;
    let length = arg(c, args, 2);
    let length = length_argument(c, length)?;
    let normalized = normalize(c, algorithm_value, Operation::DeriveBits)?;
    if !usages.contains(&Usage::DeriveBits) {
        return Err(errors::dom(c, "baseKey does not have deriveBits usage", "InvalidAccessError"));
    }
    let bits = derive_bits(c, &normalized, &key, &algorithm, length)?;
    super::array_buffer(c, bits)
}

/// The key length a derived key algorithm needs (`get key length`).
fn key_length(c: &mut Context<'_>, normalized: &Normalized) -> Result<Option<u32>, RootedError> {
    match normalized.algorithm {
        kind if kind.is_aes() => {
            let length = number_member(c, normalized, "AesDerivedKeyParams", "length")? as u32;
            if !AES_KEY_BITS.contains(&length) {
                return Err(errors::dom(c, "AES key length must be 128, 192, or 256 bits", "OperationError"));
            }
            Ok(Some(length))
        }
        Algorithm::Hmac => {
            let hash = hash_member(c, normalized, "HmacImportParams")?;
            match optional_number_member(c, normalized, "length")? {
                Some(length) => Ok(Some(length as u32)),
                None => Ok(Some(hash.block_bits())),
            }
        }
        _ => Ok(None),
    }
}

fn derive_key_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let algorithm_value = arg(c, args, 0);
    let (_, key, algorithm, _, usages) = call.crypto_key(c, args, 1)?;
    let derived_algorithm = arg(c, args, 2);
    let extractable = call.boolean(c, args, 3)?;
    let key_usages = call.usages(c, args, 4)?;
    let normalized = normalize(c, algorithm_value, Operation::DeriveBits)?;
    let derived = normalize(c, derived_algorithm, Operation::ImportKey)?;
    if !supports(Operation::GetKeyLength, derived.algorithm) {
        return Err(unrecognized(c));
    }
    if !usages.contains(&Usage::DeriveKey) {
        return Err(errors::dom(c, "baseKey does not have deriveKey usage", "InvalidAccessError"));
    }
    let length = key_length(c, &derived)?;
    let bits = derive_bits(c, &normalized, &key, &algorithm, length)?;
    let undefined = c.undefined();
    import_key(c, KeyFormat::Raw, Some(bits), undefined, &derived, extractable, key_usages)
}

// ---- wrapKey / unwrapKey -------------------------------------------------------------

fn wrap_key_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let format = call.format(c, args, 0)?;
    let (_, key, algorithm, extractable, usages) = call.crypto_key(c, args, 1)?;
    let (_, wrapping, wrapping_algorithm, _, wrapping_usages) = call.crypto_key(c, args, 2)?;
    let wrap_algorithm = arg(c, args, 3);
    let normalized = normalize(c, wrap_algorithm, Operation::WrapKey)?;
    cipher_usable(c, &wrapping_usages, Usage::WrapKey, &normalized, &wrapping_algorithm)?;
    if !extractable {
        return Err(errors::dom(c, "key is not extractable", "InvalidAccessException"));
    }
    let bytes = if format == KeyFormat::Jwk {
        let jwk = export_key(c, format, &key, &algorithm, &usages)?;
        let text = c.stringify_json_rooted(jwk)?;
        let text = c.to_string(text)?;
        text.into_bytes()
    } else {
        export_bytes(c, format, &key, &algorithm)?
    };
    let output = cipher_operation(c, &normalized, &wrapping, &wrapping_algorithm, &bytes, false)?;
    super::array_buffer(c, output)
}

fn unwrap_key_body(c: &mut Context<'_>, call: Call, args: &[RootId]) -> Outcome {
    let format = call.format(c, args, 0)?;
    let wrapped = call.buffer_source(c, args, 1)?;
    let (_, unwrapping, unwrapping_algorithm, _, unwrapping_usages) = call.crypto_key(c, args, 2)?;
    let unwrap_algorithm = arg(c, args, 3);
    let key_algorithm = arg(c, args, 4);
    let extractable = call.boolean(c, args, 5)?;
    let usages = call.usages(c, args, 6)?;
    let normalized = normalize(c, unwrap_algorithm, Operation::UnwrapKey)?;
    let imported = normalize(c, key_algorithm, Operation::ImportKey)?;
    cipher_usable(c, &unwrapping_usages, Usage::UnwrapKey, &normalized, &unwrapping_algorithm)?;
    let bytes = cipher_operation(c, &normalized, &unwrapping, &unwrapping_algorithm, &wrapped, true)?;
    if format == KeyFormat::Jwk {
        let jwk = c.parse_json_rooted(&String::from_utf8_lossy(&bytes)).map_err(|_| data_error(c, "Invalid JWK"))?;
        return import_key(c, format, None, jwk, &imported, extractable, usages);
    }
    let undefined = c.undefined();
    import_key(c, format, Some(bytes), undefined, &imported, extractable, usages)
}

// ---- KeyObject interop ---------------------------------------------------------------

/// `keyObject.toCryptoKey(algorithm, extractable, keyUsages)`.
pub(crate) fn key_object_to_crypto_key(c: &mut Context<'_>, object: RootId, algorithm: RootId, extractable: RootId, usages: RootId) -> Outcome {
    let call = Call { method: "toCryptoKey" };
    let Some(key) = keys::key_object_key(c, object)? else {
        return Err(errors::throw(c, CryptoError::InvalidThis("KeyObject")));
    };
    let extractable = c.truthy_rooted(extractable)?;
    let usages = call.usages(c, &[usages], 0)?;
    let normalized = normalize(c, algorithm, Operation::ImportKey)?;
    if usages.is_empty() && key.type_name() != "public" {
        let message = format!("Usages cannot be empty when importing a {} key.", key.type_name());
        return Err(errors::dom(c, &message, "SyntaxError"));
    }
    let undefined = c.undefined();
    match &*key {
        Key::Secret(bytes) => import_key(c, KeyFormat::Raw, Some(bytes.to_vec()), undefined, &normalized, extractable, usages),
        Key::Asymmetric(owned) => {
            let (format, encoding) = if owned.kind() == KeyKind::Public { (KeyFormat::Spki, Encoding::Spki) } else { (KeyFormat::Pkcs8, Encoding::Pkcs8) };
            let der = owned.export(Format::Der, encoding, None, None).map_err(|error| keys::key_error(c, error))?;
            import_key(c, format, Some(der), undefined, &normalized, extractable, usages)
        }
    }
}
