//! `Sign` and `Verify` (Writable streams), `crypto.sign()`/`crypto.verify()`, and the RSA
//! encryption functions.

use super::errors::{self, CryptoError};
use super::keys::{asymmetric_key, key_error, Key, KeyUse};
use super::{arg, attach, construct_if_called, define_methods, get, is_undefined, with_native, Class, Context, Members, Native, Outcome};
use crate::modules::crypto_asymmetric::{KeyError, OwnedKey};
use crate::modules::crypto_asymmetric_operations::{self as operations, RsaOperation, RsaOptions, SignatureOptions};
use crate::modules::crypto_digest::Digest;
use openssl::md_ctx::MdCtx;
use openssl::rsa::Padding;
use quench_runtime::{RootId, RootedError};
use std::sync::Arc;

/// A streaming signature's digest in progress; `None` once `sign`/`verify` consumed it.
pub(crate) struct Signer {
    context: Option<(MdCtx, Arc<Digest>)>,
}

/// The key types whose signatures cover the message itself, not a digest of it.
const ONE_SHOT_KEY_TYPES: [&str; 2] = ["ed25519", "ed448"];

fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId], class: Class) -> Outcome {
    if let Some(instance) = construct_if_called(c, this, class, args)? {
        return Ok(instance);
    }
    let (algorithm, options) = (arg(c, args, 0), arg(c, args, 1));
    let name = super::string_argument(c, algorithm, "algorithm")?;
    let digest = Digest::from_sdk_name(&name).map(Arc::new).map_err(|_| errors::throw(c, CryptoError::InvalidDigest(None)))?;
    let mut context = MdCtx::new().map_err(|stack| errors::openssl(c, stack))?;
    context.digest_init(digest.native_md()).map_err(|stack| errors::openssl(c, stack))?;
    attach(c, this, Native::Signer(Signer { context: Some((context, digest)) }))?;
    let stream = crate::modules::stream_shared_vm::module(c)?;
    let writable = get(c, stream, "Writable")?;
    c.call_rooted(writable, this, &[options])?;
    Ok(c.undefined())
}

/// `new Sign(algorithm[, options])`.
pub(crate) fn sign_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    construct(c, this, args, Class::Sign)
}

/// `new Verify(algorithm[, options])`.
pub(crate) fn verify_constructor(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    construct(c, this, args, Class::Verify)
}

fn signer<R>(c: &mut Context<'_>, this: RootId, f: impl FnOnce(&mut Signer) -> R) -> Result<R, RootedError> {
    let result = with_native(c, this, |native| match native {
        Native::Signer(signer) => Some(f(signer)),
        _ => None,
    })?;
    result.flatten().ok_or_else(|| errors::throw(c, CryptoError::PlainType("Illegal invocation")))
}

fn not_initialised(c: &mut Context<'_>) -> RootedError {
    errors::throw(c, CryptoError::InvalidStateMessage("Not initialised"))
}

/// `sign.update(data[, inputEncoding])`, shared by Verify.
pub(crate) fn update(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (data, encoding) = (arg(c, args, 0), arg(c, args, 1));
    let bytes = super::update_data(c, data, encoding)?;
    let updated = signer(c, this, |signer| signer.context.as_mut().map(|(context, _)| context.digest_update(&bytes)))?;
    match updated {
        Some(Ok(())) => Ok(this),
        Some(Err(stack)) => Err(errors::openssl(c, stack)),
        None => Err(not_initialised(c)),
    }
}

/// `_write(chunk, encoding, callback)`: `update`, then `callback()`.
pub(crate) fn write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, encoding, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    super::call_method(c, this, "update", &[chunk, encoding])?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])?;
    Ok(undefined)
}

/// `options[name]` as Node's `getIntOption`: an int32 when present.
fn int_option(c: &mut Context<'_>, options: RootId, name: &str) -> Result<Option<i32>, RootedError> {
    let value = super::option(c, options, name)?;
    if is_undefined(c, value) {
        return Ok(None);
    }
    let number = c.rooted_value(value).and_then(|value| value.as_number());
    match number.filter(|number| *number == f64::from(*number as i32)) {
        Some(number) => Ok(Some(number as i32)),
        None => Err(errors::invalid_value(c, value, &format!("options.{name}"), "is invalid")),
    }
}

/// The RSA padding, PSS salt length and DSA signature encoding a key argument carries.
fn signature_options(c: &mut Context<'_>, options: RootId) -> Result<SignatureOptions, RootedError> {
    let padding = int_option(c, options, "padding")?.map(Padding::from_raw);
    let salt_length = int_option(c, options, "saltLength")?;
    let mut ieee_p1363 = false;
    if c.is_object_rooted(options)? {
        let encoding = get(c, options, "dsaEncoding")?;
        ieee_p1363 = match c.string_text(encoding)?.as_deref() {
            _ if is_undefined(c, encoding) => false,
            Some("der") => false,
            Some("ieee-p1363") => true,
            _ => return Err(errors::invalid_value(c, encoding, "options.dsaEncoding", "is invalid")),
        };
    }
    Ok(SignatureOptions { padding, salt_length, ieee_p1363 })
}

fn asymmetric(c: &mut Context<'_>, key: &Key) -> Result<OwnedKey, RootedError> {
    key.asymmetric().cloned().ok_or_else(|| errors::throw(c, CryptoError::InvalidKeyObjectType("secret", "private")))
}

fn finish_digest(c: &mut Context<'_>, this: RootId) -> Result<(Vec<u8>, Arc<Digest>), RootedError> {
    let taken = signer(c, this, |signer| signer.context.take())?;
    let Some((mut context, digest)) = taken else {
        return Err(not_initialised(c));
    };
    let mut hashed = vec![0; digest.size()];
    let length = context.digest_final(&mut hashed).map_err(|stack| errors::openssl(c, stack))?;
    hashed.truncate(length);
    Ok((hashed, digest))
}

fn requires_message(key: &OwnedKey) -> bool {
    key.key_type().is_some_and(|name| ONE_SHOT_KEY_TYPES.contains(&name))
}

/// `sign.sign(privateKey[, outputEncoding])`.
pub(crate) fn sign(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (options, encoding) = (arg(c, args, 0), arg(c, args, 1));
    if !c.truthy_rooted(options)? {
        return Err(errors::throw(c, CryptoError::SignKeyRequired));
    }
    let key = asymmetric_key(c, options, KeyUse::ConsumePrivate)?;
    let settings = signature_options(c, options)?;
    let key = asymmetric(c, &key)?;
    if requires_message(&key) {
        return Err(errors::throw(c, CryptoError::UnsupportedOperation));
    }
    let (hashed, digest) = finish_digest(c, this)?;
    let signature = operations::sign_prehashed(&key, &digest, &hashed, &settings).map_err(|error| key_error(c, error))?;
    super::encode_output(c, signature, encoding)
}

/// `verify.verify(publicKey, signature[, signatureEncoding])`.
pub(crate) fn verify(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (options, signature, encoding) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let key = asymmetric_key(c, options, KeyUse::ConsumePublic)?;
    let settings = signature_options(c, options)?;
    let signature = super::binary_like(c, signature, "signature", encoding)?;
    let key = asymmetric(c, &key)?;
    if requires_message(&key) {
        return Err(errors::throw(c, CryptoError::UnsupportedOperation));
    }
    let (hashed, digest) = finish_digest(c, this)?;
    let verified = match operations::verify_prehashed(&key, &digest, &hashed, &signature, &settings) {
        Ok(verified) => verified,
        Err(KeyError::Unsupported(_)) => false,
        Err(error) => return Err(key_error(c, error)),
    };
    Ok(c.boolean(verified))
}

pub(crate) fn build(c: &mut Context<'_>, writable: RootId) -> Result<Vec<(Class, RootId)>, RootedError> {
    let sign = super::subclass(c, "cryptoSignConstructor", "Sign", writable)?;
    let prototype = get(c, sign, "prototype")?;
    define_methods(
        c,
        prototype,
        &[("_write", "cryptoSignWrite"), ("update", "cryptoSignUpdate"), ("sign", "cryptoSignSign")],
        Members::Assigned,
    )?;
    let verify = super::subclass(c, "cryptoVerifyConstructor", "Verify", writable)?;
    let prototype = get(c, verify, "prototype")?;
    define_methods(
        c,
        prototype,
        &[("_write", "cryptoSignWrite"), ("update", "cryptoSignUpdate"), ("verify", "cryptoVerifyVerify")],
        Members::Assigned,
    )?;
    Ok(vec![(Class::Sign, sign), (Class::Verify, verify)])
}

/// `crypto.createSign(algorithm[, options])`.
pub(crate) fn create_sign(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::Sign)?;
    let values = [arg(c, args, 0), arg(c, args, 1)];
    c.construct_rooted(constructor, constructor, &values)
}

/// `crypto.createVerify(algorithm[, options])`.
pub(crate) fn create_verify(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let constructor = super::class(c, Class::Verify)?;
    let values = [arg(c, args, 0), arg(c, args, 1)];
    c.construct_rooted(constructor, constructor, &values)
}

// ---- One-shot signatures -------------------------------------------------------------

/// `EVP_DigestSign` over the whole message, with `digest` or the key's default.
pub(crate) fn sign_message(key: &OwnedKey, digest: Option<&Digest>, data: &[u8], options: &SignatureOptions) -> Result<Vec<u8>, KeyError> {
    let private = key.private_ref().ok_or(KeyError::Unsupported("private key required"))?;
    let mut context = MdCtx::new()?;
    let pkey_context = context.digest_sign_init(digest.map(Digest::native_md), private)?;
    operations::apply_signature_options(pkey_context, private.id(), options)?;
    let mut signature = Vec::new();
    context.digest_sign_to_vec(data, &mut signature)?;
    if options.ieee_p1363 {
        operations::to_p1363(private, &signature)
    } else {
        Ok(signature)
    }
}

/// `EVP_DigestVerify` over the whole message.
pub(crate) fn verify_message(key: &OwnedKey, digest: Option<&Digest>, data: &[u8], signature: &[u8], options: &SignatureOptions) -> Result<bool, KeyError> {
    let public = key.public_ref();
    let converted;
    let signature = if options.ieee_p1363 {
        match operations::from_p1363(public, signature)? {
            Some(value) => {
                converted = value;
                &converted
            }
            None => return Ok(false),
        }
    } else {
        signature
    };
    let mut context = MdCtx::new()?;
    let pkey_context = context.digest_verify_init(digest.map(Digest::native_md), public)?;
    operations::apply_signature_options(pkey_context, public.id(), options)?;
    Ok(context.digest_verify(data, signature).unwrap_or(false))
}

struct OneShot {
    digest: Option<Arc<Digest>>,
    data: Vec<u8>,
    key: OwnedKey,
    options: SignatureOptions,
}

fn one_shot(c: &mut Context<'_>, algorithm: RootId, data: RootId, key: RootId, key_use: KeyUse, callback: RootId) -> Result<OneShot, RootedError> {
    let name = if super::is_nullish(c, algorithm) { None } else { Some(super::string_argument(c, algorithm, "algorithm")?) };
    if !is_undefined(c, callback) {
        super::callback_argument(c, callback, "callback")?;
    }
    let undefined = c.undefined();
    let data = super::binary_like(c, data, "data", undefined)?;
    if !c.truthy_rooted(key)? {
        return Err(errors::throw(c, CryptoError::SignKeyRequired));
    }
    let options = signature_options(c, key)?;
    let key = asymmetric_key(c, key, key_use)?;
    let key = asymmetric(c, &key)?;
    let digest = match name {
        Some(name) => Some(Digest::from_sdk_name(&name).map(Arc::new).map_err(|_| errors::throw(c, CryptoError::InvalidDigest(Some(&name))))?),
        None => None,
    };
    Ok(OneShot { digest, data, key, options })
}

/// `crypto.sign(algorithm, data, key[, callback])`.
pub(crate) fn sign_one_shot(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (algorithm, data, key, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let request = one_shot(c, algorithm, data, key, KeyUse::ConsumePrivate, callback)?;
    let signature = sign_message(&request.key, request.digest.as_deref(), &request.data, &request.options);
    let result = signature.map_err(|error| key_error(c, error)).and_then(|signature| super::buffer(c, signature));
    if is_undefined(c, callback) {
        return result;
    }
    let result = result.map(|signature| vec![signature]);
    super::complete(c, callback, result)
}

/// `crypto.verify(algorithm, data, key, signature[, callback])`.
pub(crate) fn verify_one_shot(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (algorithm, data, key, signature, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3), arg(c, args, 4));
    let request = one_shot(c, algorithm, data, key, KeyUse::ConsumePublic, callback)?;
    let undefined = c.undefined();
    let signature = super::binary_like(c, signature, "signature", undefined)?;
    let verified = verify_message(&request.key, request.digest.as_deref(), &request.data, &signature, &request.options);
    let result = verified.map_err(|error| key_error(c, error)).map(|verified| c.boolean(verified));
    if is_undefined(c, callback) {
        return result;
    }
    let result = result.map(|verified| vec![verified]);
    super::complete(c, callback, result)
}

// ---- RSA encryption ------------------------------------------------------------------

fn rsa_function(c: &mut Context<'_>, args: &[RootId], operation: RsaOperation) -> Outcome {
    let (options, buffer) = (arg(c, args, 0), arg(c, args, 1));
    let (key_use, default_padding) = match operation {
        RsaOperation::PublicEncrypt => (KeyUse::ConsumePublic, Padding::PKCS1_OAEP),
        RsaOperation::PublicDecrypt => (KeyUse::ConsumePublic, Padding::PKCS1),
        RsaOperation::PrivateEncrypt => (KeyUse::ConsumePrivate, Padding::PKCS1),
        RsaOperation::PrivateDecrypt => (KeyUse::ConsumePrivate, Padding::PKCS1_OAEP),
    };
    let key = asymmetric_key(c, options, key_use)?;
    let padding = super::option(c, options, "padding")?;
    let padding = if c.truthy_rooted(padding)? { Padding::from_raw(c.to_number(padding)? as i32) } else { default_padding };
    let hash = super::option(c, options, "oaepHash")?;
    let hash = if is_undefined(c, hash) { None } else { Some(super::string_argument(c, hash, "key.oaepHash")?) };
    let encoding = super::option(c, options, "encoding")?;
    let label = super::option(c, options, "oaepLabel")?;
    let label = if is_undefined(c, label) { None } else { Some(super::binary_like(c, label, "key.oaepLabel", encoding)?) };
    let input = super::binary_like(c, buffer, "buffer", encoding)?;
    let key = asymmetric(c, &key)?;
    let oaep = padding == Padding::PKCS1_OAEP;
    let digest = match (oaep, hash.as_deref()) {
        (true, name) => {
            let name = name.unwrap_or("sha1");
            Some(Digest::fetch(name).map_err(|_| errors::throw(c, CryptoError::InvalidDigestUsed))?)
        }
        (false, _) => None,
    };
    let rsa_options = RsaOptions { padding, digest: digest.as_ref(), mgf1_digest: digest.as_ref(), label: label.as_deref().filter(|_| oaep) };
    let output = operations::rsa(&key, operation, &input, &rsa_options).map_err(|error| key_error(c, error))?;
    super::buffer(c, output)
}

/// `crypto.publicEncrypt(key, buffer)`.
pub(crate) fn public_encrypt(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    rsa_function(c, args, RsaOperation::PublicEncrypt)
}

/// `crypto.publicDecrypt(key, buffer)`.
pub(crate) fn public_decrypt(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    rsa_function(c, args, RsaOperation::PublicDecrypt)
}

/// `crypto.privateEncrypt(privateKey, buffer)`.
pub(crate) fn private_encrypt(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    rsa_function(c, args, RsaOperation::PrivateEncrypt)
}

/// `crypto.privateDecrypt(privateKey, buffer)`.
pub(crate) fn private_decrypt(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    rsa_function(c, args, RsaOperation::PrivateDecrypt)
}
