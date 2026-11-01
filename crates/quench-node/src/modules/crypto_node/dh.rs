//! `ECDH`, `DiffieHellman`, `DiffieHellmanGroup`, `crypto.diffieHellman()` and
//! `getCurves()`.

use super::errors::{self, CryptoError};
use super::{arg, attach, construct_if_called, define_methods, get, with_native, Class, Context, Members, Native, Outcome};
use openssl::bn::{BigNum, BigNumContext, BigNumRef};
use openssl::ec::{EcGroup, EcKey, EcPoint, PointConversionForm};
use openssl::nid::Nid;
use openssl::pkey::Private;
use quench_runtime::{RootId, RootedError};
use std::ffi::{c_char, c_int, CStr};

/// The generator a DiffieHellman uses when none is given.
const DEFAULT_GENERATOR: u32 = 2;
/// `EC_builtin_curve`, from openssl/ec.h.
#[repr(C)]
struct BuiltinCurve {
    nid: c_int,
    comment: *const c_char,
}

unsafe extern "C" {
    fn EC_get_builtin_curves(curves: *mut BuiltinCurve, count: usize) -> usize;
}

/// The point encodings `getPublicKey`/`convertKey` accept.
fn point_format(c: &mut Context<'_>, format: RootId) -> Result<PointConversionForm, RootedError> {
    if !c.truthy_rooted(format)? {
        return Ok(PointConversionForm::UNCOMPRESSED);
    }
    let text = c.to_string(format)?;
    match text.as_str() {
        "compressed" => Ok(PointConversionForm::COMPRESSED),
        "hybrid" => Ok(PointConversionForm::HYBRID),
        "uncompressed" => Ok(PointConversionForm::UNCOMPRESSED),
        _ => Err(errors::throw(c, CryptoError::EcdhInvalidFormat(text))),
    }
}

/// `encode(buffer, encoding)`: a string unless the encoding is absent or `'buffer'`.
fn encode(c: &mut Context<'_>, bytes: Vec<u8>, encoding: RootId) -> Outcome {
    super::encode_output(c, bytes, encoding)
}

// ---- ECDH ----------------------------------------------------------------------------

/// An ECDH party: its curve and, once generated or set, its key.
pub(crate) struct Ecdh {
    group: EcGroup,
    key: Option<EcKey<Private>>,
}

fn curve_group(name: &str) -> Option<EcGroup> {
    let name = std::ffi::CString::new(name).ok()?;
    // SAFETY: OBJ_sn2nid reads a NUL-terminated name.
    let nid = unsafe { openssl_sys::OBJ_sn2nid(name.as_ptr()) };
    if nid == openssl_sys::NID_undef {
        return None;
    }
    let _errors = crate::modules::crypto_provider_facts::ErrorScope::new();
    EcGroup::from_curve_name(Nid::from_raw(nid)).ok()
}

/// `new ECDH(curve)`.
pub(crate) fn ecdh_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if let Some(instance) = construct_if_called(c, this, Class::Ecdh, args)? {
        return Ok(instance);
    }
    let curve = arg(c, args, 0);
    let name = super::string_argument(c, curve, "curve")?;
    let Some(group) = curve_group(&name) else {
        return Err(errors::throw(c, CryptoError::InvalidCurve));
    };
    attach(c, this, Native::Ecdh(Ecdh { group, key: None }))?;
    Ok(c.undefined())
}

fn ecdh<R>(c: &mut Context<'_>, this: RootId, f: impl FnOnce(&mut Ecdh) -> R) -> Result<R, RootedError> {
    let result = with_native(c, this, |native| match native {
        Native::Ecdh(state) => Some(f(state)),
        _ => None,
    })?;
    result.flatten().ok_or_else(|| errors::throw(c, CryptoError::PlainType("Illegal invocation")))
}

fn public_point(group: &EcGroup, bytes: &[u8]) -> Option<EcPoint> {
    let mut context = BigNumContext::new().ok()?;
    let _errors = crate::modules::crypto_provider_facts::ErrorScope::new();
    EcPoint::from_bytes(group, bytes, &mut context).ok()
}

/// `ecdh.generateKeys([encoding[, format]])`.
pub(crate) fn ecdh_generate_keys(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let generated = ecdh(c, this, |state| EcKey::generate(&state.group).map(|key| state.key = Some(key)))?;
    generated.map_err(|stack| errors::openssl(c, stack))?;
    ecdh_get_public_key(c, this, args)
}

/// `ecdh.getPublicKey([encoding[, format]])`.
pub(crate) fn ecdh_get_public_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (encoding, format) = (arg(c, args, 0), arg(c, args, 1));
    let form = point_format(c, format)?;
    let bytes = ecdh(c, this, |state| {
        let key = state.key.as_ref()?;
        let mut context = BigNumContext::new().ok()?;
        key.public_key().to_bytes(&state.group, form, &mut context).ok()
    })?;
    match bytes {
        Some(bytes) => encode(c, bytes, encoding),
        None => Err(errors::throw(c, CryptoError::OperationFailed("Failed to get ECDH public key"))),
    }
}

/// `ecdh.getPrivateKey([encoding])`.
pub(crate) fn ecdh_get_private_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    let bytes = ecdh(c, this, |state| {
        let key = state.key.as_ref()?;
        let width = state.group.degree().div_ceil(u8::BITS) as i32;
        key.private_key().to_vec_padded(width).ok()
    })?;
    match bytes {
        Some(bytes) => encode(c, bytes, encoding),
        None => Err(errors::throw(c, CryptoError::OperationFailed("Failed to get ECDH private key"))),
    }
}

/// `ecdh.setPrivateKey(privateKey[, encoding])`: the public key follows from it.
pub(crate) fn ecdh_set_private_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (key, encoding) = (arg(c, args, 0), arg(c, args, 1));
    let bytes = super::binary_like(c, key, "key", encoding)?;
    let set = ecdh(c, this, |state| {
        let _errors = crate::modules::crypto_provider_facts::ErrorScope::new();
        let private = BigNum::from_slice(&bytes).ok()?;
        let mut context = BigNumContext::new().ok()?;
        let mut order = BigNum::new().ok()?;
        state.group.order(&mut order, &mut context).ok()?;
        if private.num_bits() == 0 || private >= order {
            return None;
        }
        let mut public = EcPoint::new(&state.group).ok()?;
        public.mul_generator2(&state.group, &private, &mut context).ok()?;
        let key = EcKey::from_private_components(&state.group, &private, &public).ok()?;
        key.check_key().ok()?;
        state.key = Some(key);
        Some(())
    })?;
    if set.is_none() {
        return Err(errors::throw(c, CryptoError::InvalidKeyType("Private key is not valid for specified curve.")));
    }
    Ok(this)
}

/// `ecdh.setPublicKey(publicKey[, encoding])` (deprecated): replace the public half.
pub(crate) fn ecdh_set_public_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (key, encoding) = (arg(c, args, 0), arg(c, args, 1));
    let bytes = super::binary_like(c, key, "key", encoding)?;
    let set = ecdh(c, this, |state| {
        let point = public_point(&state.group, &bytes)?;
        let key = match &state.key {
            Some(key) => EcKey::from_private_components(&state.group, key.private_key(), &point).ok()?,
            None => {
                let zero = BigNum::new().ok()?;
                EcKey::from_private_components(&state.group, &zero, &point).ok()?
            }
        };
        state.key = Some(key);
        Some(())
    })?;
    if set.is_none() {
        return Err(errors::throw(c, CryptoError::OperationFailed("Failed to convert Buffer to EC_POINT")));
    }
    Ok(this)
}

/// The shared secret of an EC private key and a peer's encoded point.
fn ec_secret(group: &EcGroup, private: &BigNumRef, peer: &[u8]) -> Option<Vec<u8>> {
    let point = public_point(group, peer)?;
    let mut context = BigNumContext::new().ok()?;
    let mut product = EcPoint::new(group).ok()?;
    product.mul2(group, &point, private, &mut context).ok()?;
    let (mut x, mut y) = (BigNum::new().ok()?, BigNum::new().ok()?);
    product.affine_coordinates(group, &mut x, &mut y, &mut context).ok()?;
    x.to_vec_padded(group.degree().div_ceil(u8::BITS) as i32).ok()
}

/// `ecdh.computeSecret(otherPublicKey[, inputEncoding][, outputEncoding])`.
pub(crate) fn ecdh_compute_secret(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (key, input_encoding, output_encoding) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let peer = super::binary_like(c, key, "key", input_encoding)?;
    let secret = ecdh(c, this, |state| {
        let key = state.key.as_ref()?;
        ec_secret(&state.group, key.private_key(), &peer)
    })?;
    match secret {
        Some(secret) => encode(c, secret, output_encoding),
        None => Err(errors::throw(c, CryptoError::EcdhInvalidPublicKey)),
    }
}

/// `ECDH.convertKey(key, curve[, inputEncoding[, outputEncoding[, format]]])`.
pub(crate) fn ecdh_convert_key(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (key, curve, input_encoding, output_encoding, format) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3), arg(c, args, 4));
    let name = super::string_argument(c, curve, "curve")?;
    let bytes = super::binary_like(c, key, "key", input_encoding)?;
    let form = point_format(c, format)?;
    let Some(group) = curve_group(&name) else {
        return Err(errors::throw(c, CryptoError::InvalidCurve));
    };
    let converted = public_point(&group, &bytes).and_then(|point| {
        let mut context = BigNumContext::new().ok()?;
        point.to_bytes(&group, form, &mut context).ok()
    });
    match converted {
        Some(bytes) => encode(c, bytes, output_encoding),
        None => Err(errors::throw(c, CryptoError::OperationFailed("Failed to convert Buffer to EC_POINT"))),
    }
}

// ---- Finite-field Diffie-Hellman -----------------------------------------------------

/// A DiffieHellman party: the group, and its key once generated or set.
pub(crate) struct DiffieHellman {
    prime: BigNum,
    generator: BigNum,
    public: Option<BigNum>,
    private: Option<BigNum>,
    verify_error: i32,
}

impl DiffieHellman {
    fn new(prime: BigNum, generator: BigNum) -> Result<Self, openssl::error::ErrorStack> {
        let check = openssl::dh::Dh::from_pqg(prime.to_owned()?, None, generator.to_owned()?)?;
        let mut codes: c_int = 0;
        // SAFETY: DH_check reads the parameters of a live DH and writes its flags to `codes`.
        unsafe { openssl_sys::DH_check(foreign_types::ForeignType::as_ptr(&check), &mut codes) };
        Ok(Self { prime, generator, public: None, private: None, verify_error: codes })
    }

    fn generate(&mut self) -> Result<(), openssl::error::ErrorStack> {
        let mut context = BigNumContext::new()?;
        if self.private.is_none() {
            let parameters = openssl::dh::Dh::from_pqg(self.prime.to_owned()?, None, self.generator.to_owned()?)?;
            let key = parameters.generate_key()?;
            self.private = Some(key.private_key().to_owned()?);
        }
        let mut public = BigNum::new()?;
        if let Some(private) = &self.private {
            public.mod_exp(&self.generator, private, &self.prime, &mut context)?;
        }
        self.public = Some(public);
        Ok(())
    }

    fn width(&self) -> i32 {
        self.prime.num_bytes()
    }
}

fn dh_group(name: &str) -> Option<BigNum> {
    let known = matches!(name, "modp1" | "modp2" | "modp5" | "modp14" | "modp15" | "modp16" | "modp17" | "modp18");
    known.then(|| crate::modules::crypto_asymmetric_generate::standard_group(name).ok()).flatten()
}

/// A `sizeOrKey`/`generator` argument's bytes (a string under `encoding`).
fn group_bytes(c: &mut Context<'_>, value: RootId, encoding: RootId) -> Result<Vec<u8>, RootedError> {
    if super::is_string(c, value) {
        return super::string_bytes(c, value, encoding);
    }
    Ok(super::buffer_bytes(c, value).unwrap_or_default())
}

fn number_or_bytes(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    Ok(c.rooted_value(value).and_then(|value| value.as_number()).is_some()
        || super::is_string(c, value)
        || super::buffer_bytes(c, value).is_some())
}

const GROUP_ARGUMENT_TYPES: &str = "one of type number or string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView";

/// `new DiffieHellman(sizeOrKey[, keyEncoding][, generator][, generatorEncoding])`.
pub(crate) fn diffie_hellman_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if let Some(instance) = construct_if_called(c, this, Class::DiffieHellman, args)? {
        return Ok(instance);
    }
    let (size_or_key, mut key_encoding, mut generator, mut generator_encoding) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    if !number_or_bytes(c, size_or_key)? {
        return Err(errors::invalid_type(c, size_or_key, "sizeOrKey", GROUP_ARGUMENT_TYPES));
    }
    let size = c.rooted_value(size_or_key).and_then(|value| value.as_number());
    if size.is_some() {
        super::integer_argument(c, size_or_key, "sizeOrKey", i32::MIN as f64, i32::MAX as f64)?;
    }
    if c.truthy_rooted(key_encoding)? {
        let name = c.string_text(key_encoding)?;
        let encoding = name.as_deref().is_some_and(|name| name == "buffer" || super::canonical_encoding(name).is_some());
        if !encoding {
            generator_encoding = generator;
            generator = key_encoding;
            key_encoding = c.undefined();
        }
    }
    let generator = if !c.truthy_rooted(generator)? {
        BigNum::from_u32(DEFAULT_GENERATOR).map_err(|stack| errors::openssl(c, stack))?
    } else if let Some(number) = c.rooted_value(generator).and_then(|value| value.as_number()) {
        super::integer_argument(c, generator, "generator", i32::MIN as f64, i32::MAX as f64)?;
        BigNum::from_u32(number as u32).map_err(|stack| errors::openssl(c, stack))?
    } else if number_or_bytes(c, generator)? {
        let bytes = group_bytes(c, generator, generator_encoding)?;
        BigNum::from_slice(&bytes).map_err(|stack| errors::openssl(c, stack))?
    } else {
        return Err(errors::invalid_type(c, generator, "generator", GROUP_ARGUMENT_TYPES));
    };
    let prime = match size {
        Some(bits) => {
            let generated = openssl::dh::Dh::generate_params(bits as u32, generator.to_dec_str().ok().and_then(|text| text.parse().ok()).unwrap_or(DEFAULT_GENERATOR));
            let generated = if bits < 0.0 { Err(openssl::error::ErrorStack::get()) } else { generated };
            match generated {
                Ok(parameters) => parameters.prime_p().to_owned().map_err(|stack| errors::openssl(c, stack))?,
                Err(_) => return Err(dh_modulus_error(c)),
            }
        }
        None => {
            let bytes = group_bytes(c, size_or_key, key_encoding)?;
            BigNum::from_slice(&bytes).map_err(|stack| errors::openssl(c, stack))?
        }
    };
    let state = DiffieHellman::new(prime, generator).map_err(|stack| errors::openssl(c, stack))?;
    install_diffie_hellman(c, this, state)
}

/// The error OpenSSL raises for a prime size it cannot generate.
fn dh_modulus_error(c: &mut Context<'_>) -> RootedError {
    let stack = openssl::error::ErrorStack::get();
    if !stack.errors().is_empty() {
        return errors::openssl(c, stack);
    }
    errors::throw(c, CryptoError::Plain("modulus too small"))
}

fn install_diffie_hellman(c: &mut Context<'_>, this: RootId, state: DiffieHellman) -> Outcome {
    let verify_error = c.number(f64::from(state.verify_error));
    attach(c, this, Native::DiffieHellman(state))?;
    let key = c.string_rooted("verifyError");
    c.define_data_property_rooted(this, key, verify_error, true, true, true)?;
    Ok(c.undefined())
}

/// `new DiffieHellmanGroup(name)`.
pub(crate) fn diffie_hellman_group_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if let Some(instance) = construct_if_called(c, this, Class::DiffieHellmanGroup, args)? {
        return Ok(instance);
    }
    let name = arg(c, args, 0);
    let name = super::string_argument(c, name, "name")?;
    let Some(prime) = dh_group(&name) else {
        return Err(errors::throw(c, CryptoError::UnknownDhGroup));
    };
    let generator = BigNum::from_u32(DEFAULT_GENERATOR).map_err(|stack| errors::openssl(c, stack))?;
    let state = DiffieHellman::new(prime, generator).map_err(|stack| errors::openssl(c, stack))?;
    install_diffie_hellman(c, this, state)
}

fn dh<R>(c: &mut Context<'_>, this: RootId, f: impl FnOnce(&mut DiffieHellman) -> R) -> Result<R, RootedError> {
    let result = with_native(c, this, |native| match native {
        Native::DiffieHellman(state) => Some(f(state)),
        _ => None,
    })?;
    result.flatten().ok_or_else(|| errors::throw(c, CryptoError::PlainType("Illegal invocation")))
}

/// `dh.generateKeys([encoding])`.
pub(crate) fn dh_generate_keys(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    dh(c, this, DiffieHellman::generate)?.map_err(|stack| errors::openssl(c, stack))?;
    dh_get_public_key(c, this, args)
}

fn dh_value(c: &mut Context<'_>, this: RootId, encoding: RootId, missing: &'static str, read: fn(&DiffieHellman) -> Option<Vec<u8>>) -> Outcome {
    match dh(c, this, |state| read(state))? {
        Some(bytes) => encode(c, bytes, encoding),
        None => Err(errors::throw(c, CryptoError::InvalidStateMessage(missing))),
    }
}

/// `dh.getPrime([encoding])`.
pub(crate) fn dh_get_prime(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    dh_value(c, this, encoding, "", |state| Some(state.prime.to_vec()))
}

/// `dh.getGenerator([encoding])`.
pub(crate) fn dh_get_generator(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    dh_value(c, this, encoding, "", |state| Some(state.generator.to_vec()))
}

/// `dh.getPublicKey([encoding])`.
pub(crate) fn dh_get_public_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    dh_value(c, this, encoding, "No public key - did you forget to generate one?", |state| state.public.as_ref().map(|value| value.to_vec()))
}

/// `dh.getPrivateKey([encoding])`.
pub(crate) fn dh_get_private_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    dh_value(c, this, encoding, "No private key - did you forget to generate one?", |state| state.private.as_ref().map(|value| value.to_vec()))
}

fn dh_set(c: &mut Context<'_>, this: RootId, args: &[RootId], private: bool) -> Outcome {
    let (key, encoding) = (arg(c, args, 0), arg(c, args, 1));
    let bytes = super::binary_like(c, key, "key", encoding)?;
    let value = BigNum::from_slice(&bytes).map_err(|stack| errors::openssl(c, stack))?;
    dh(c, this, |state| if private { state.private = Some(value) } else { state.public = Some(value) })?;
    Ok(this)
}

/// `dh.setPublicKey(publicKey[, encoding])`.
pub(crate) fn dh_set_public_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    dh_set(c, this, args, false)
}

/// `dh.setPrivateKey(privateKey[, encoding])`.
pub(crate) fn dh_set_private_key(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    dh_set(c, this, args, true)
}

/// Why a peer's public value is unusable, as `DH_check_pub_key` reports it.
enum PeerError {
    TooSmall,
    TooLarge,
    Invalid,
}

/// `dh.computeSecret(otherPublicKey[, inputEncoding][, outputEncoding])`.
pub(crate) fn dh_compute_secret(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (key, input_encoding, output_encoding) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let peer = super::binary_like(c, key, "key", input_encoding)?;
    let secret = dh(c, this, |state| -> Result<Option<Vec<u8>>, PeerError> {
        let Some(private) = &state.private else {
            return Ok(None);
        };
        let peer = BigNum::from_slice(&peer).map_err(|_| PeerError::Invalid)?;
        let one = BigNum::from_u32(1).map_err(|_| PeerError::Invalid)?;
        let mut limit = BigNum::new().map_err(|_| PeerError::Invalid)?;
        limit.checked_sub(&state.prime, &one).map_err(|_| PeerError::Invalid)?;
        if peer <= one {
            return Err(PeerError::TooSmall);
        }
        if peer >= limit {
            return Err(PeerError::TooLarge);
        }
        let mut context = BigNumContext::new().map_err(|_| PeerError::Invalid)?;
        let mut secret = BigNum::new().map_err(|_| PeerError::Invalid)?;
        secret.mod_exp(&peer, private, &state.prime, &mut context).map_err(|_| PeerError::Invalid)?;
        secret.to_vec_padded(state.width()).map(Some).map_err(|_| PeerError::Invalid)
    })?;
    match secret {
        Ok(Some(secret)) => encode(c, secret, output_encoding),
        Ok(None) => Err(errors::throw(c, CryptoError::InvalidStateMessage("No private key - did you forget to generate one?"))),
        Err(PeerError::TooSmall) => Err(errors::throw(c, CryptoError::InvalidKeyLengthMessage("Supplied key is too small"))),
        Err(PeerError::TooLarge) => Err(errors::throw(c, CryptoError::InvalidKeyType("Unspecified validation error"))),
        Err(PeerError::Invalid) => Err(errors::throw(c, CryptoError::InvalidKeyType("Invalid key"))),
    }
}

pub(crate) fn build(c: &mut Context<'_>) -> Result<Vec<(Class, RootId)>, RootedError> {
    let shared = [
        ("generateKeys", "cryptoDhGenerateKeys"),
        ("computeSecret", "cryptoDhComputeSecret"),
        ("getPrime", "cryptoDhGetPrime"),
        ("getGenerator", "cryptoDhGetGenerator"),
        ("getPublicKey", "cryptoDhGetPublicKey"),
        ("getPrivateKey", "cryptoDhGetPrivateKey"),
    ];
    let diffie_hellman = c.host_constructor_named(crate::host::shared_vm::operation("cryptoDiffieHellmanConstructor"), "DiffieHellman")?;
    let prototype = get(c, diffie_hellman, "prototype")?;
    define_methods(c, prototype, &shared, Members::Assigned)?;
    define_methods(c, prototype, &[("setPublicKey", "cryptoDhSetPublicKey"), ("setPrivateKey", "cryptoDhSetPrivateKey")], Members::Assigned)?;
    let group = c.host_constructor_named(crate::host::shared_vm::operation("cryptoDiffieHellmanGroupConstructor"), "DiffieHellmanGroup")?;
    let prototype = get(c, group, "prototype")?;
    define_methods(c, prototype, &shared, Members::Assigned)?;
    let ecdh = c.host_constructor_named(crate::host::shared_vm::operation("cryptoEcdhConstructor"), "ECDH")?;
    let prototype = get(c, ecdh, "prototype")?;
    define_methods(
        c,
        prototype,
        &[
            ("computeSecret", "cryptoEcdhComputeSecret"),
            ("setPrivateKey", "cryptoEcdhSetPrivateKey"),
            ("setPublicKey", "cryptoEcdhSetPublicKey"),
            ("getPrivateKey", "cryptoEcdhGetPrivateKey"),
            ("generateKeys", "cryptoEcdhGenerateKeys"),
            ("getPublicKey", "cryptoEcdhGetPublicKey"),
        ],
        Members::Assigned,
    )?;
    define_methods(c, ecdh, &[("convertKey", "cryptoEcdhConvertKey")], Members::Assigned)?;
    Ok(vec![(Class::DiffieHellman, diffie_hellman), (Class::DiffieHellmanGroup, group), (Class::Ecdh, ecdh)])
}

/// `crypto.createECDH(curve)`.
pub(crate) fn create_ecdh(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::Ecdh)?;
    let curve = arg(c, args, 0);
    c.construct_rooted(constructor, constructor, &[curve])
}

/// `crypto.createDiffieHellman(...)`.
pub(crate) fn create_diffie_hellman(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::DiffieHellman)?;
    let values: Vec<RootId> = (0..4).map(|index| arg(c, args, index)).collect();
    c.construct_rooted(constructor, constructor, &values)
}

/// `crypto.createDiffieHellmanGroup(name)` and `crypto.getDiffieHellman(name)`.
pub(crate) fn create_diffie_hellman_group(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::DiffieHellmanGroup)?;
    let name = arg(c, args, 0);
    c.construct_rooted(constructor, constructor, &[name])
}

/// The key types `crypto.diffieHellman()` derives with.
const DH_KEY_TYPES: [&str; 4] = ["dh", "ec", "x448", "x25519"];

/// `crypto.diffieHellman({ privateKey, publicKey })`.
pub(crate) fn stateless(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let options = arg(c, args, 0);
    super::object_argument(c, options, "options")?;
    let mut keys = Vec::with_capacity(2);
    for name in ["privateKey", "publicKey"] {
        let value = get(c, options, name)?;
        match super::keys::key_object_key(c, value)? {
            Some(key) => keys.push(key),
            None => return Err(errors::invalid_value(c, value, &format!("options.{name}"), "is invalid")),
        }
    }
    let (private, public) = (keys[0].clone(), keys[1].clone());
    if private.type_name() != "private" {
        return Err(errors::throw(c, CryptoError::InvalidKeyObjectType(private.type_name(), "private")));
    }
    if public.type_name() == "secret" {
        return Err(errors::throw(c, CryptoError::InvalidKeyObjectType(public.type_name(), "public or private")));
    }
    let (Some(private), Some(public)) = (private.asymmetric(), public.asymmetric()) else {
        return Err(RootedError::host("asymmetric keys expected"));
    };
    let types = (private.key_type().unwrap_or_default(), public.key_type().unwrap_or_default());
    if types.0 != types.1 || !DH_KEY_TYPES.contains(&types.0) {
        let pair = format!("{} and {}", types.0, types.1);
        return Err(errors::throw(c, CryptoError::IncompatibleKey("key types for Diffie-Hellman", &pair)));
    }
    let secret = derive(private, public).map_err(|stack| errors::openssl(c, stack))?;
    super::buffer(c, secret)
}

/// `EVP_PKEY_derive` of a private key and a peer's public key.
pub(crate) fn derive(private: &crate::modules::crypto_asymmetric::OwnedKey, public: &crate::modules::crypto_asymmetric::OwnedKey) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    let Some(key) = private.private_ref() else {
        return Err(openssl::error::ErrorStack::get());
    };
    let mut deriver = openssl::derive::Deriver::new(key)?;
    deriver.set_peer(public.public_ref())?;
    deriver.derive_to_vec()
}

/// `crypto.getCurves()`: the built-in curves' short names, sorted.
pub(crate) fn get_curves(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    // SAFETY: a null buffer asks only for the count; the second call fills `count` entries.
    let count = unsafe { EC_get_builtin_curves(std::ptr::null_mut(), 0) };
    let mut curves: Vec<BuiltinCurve> = (0..count).map(|_| BuiltinCurve { nid: 0, comment: std::ptr::null() }).collect();
    unsafe { EC_get_builtin_curves(curves.as_mut_ptr(), count) };
    let mut names: Vec<String> = curves
        .iter()
        .filter_map(|curve| {
            // SAFETY: OBJ_nid2sn returns a static name or null.
            let name = unsafe { openssl_sys::OBJ_nid2sn(curve.nid) };
            (!name.is_null()).then(|| unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned())
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    super::string_array(c, &names)
}
