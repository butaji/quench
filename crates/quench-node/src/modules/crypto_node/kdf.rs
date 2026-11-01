//! PBKDF2, scrypt and HKDF, synchronous and with a completion callback.

use super::errors::{self, CryptoError};
use super::{arg, callback_argument, complete, get, is_undefined, Context, Outcome};
use crate::modules::crypto_error_shared_vm;
use quench_runtime::{RootId, RootedError};

/// The job failure message a derivation reports.
const DERIVE_FAILED: &str = "Deriving bits failed";
/// HKDF's `info` limit (RFC 5869 leaves it open; Node bounds it).
const HKDF_MAX_INFO_BYTES: usize = 1024;
const INT32_MAX: f64 = i32::MAX as f64;
const UINT32_MAX: f64 = u32::MAX as f64;

/// scrypt's defaults: `N`, `r`, `p` and the memory bound (32 MiB).
const SCRYPT_COST: f64 = 16_384.0;
const SCRYPT_BLOCK_SIZE: f64 = 8.0;
const SCRYPT_PARALLELIZATION: f64 = 1.0;
const SCRYPT_MAX_MEMORY: f64 = (32 << 20) as f64;
/// The width of one OpenSSL error string.
const ERROR_TEXT_BYTES: usize = 256;

fn derive_failed(c: &mut Context<'_>, stack: &openssl::error::ErrorStack) -> RootedError {
    crypto_error_shared_vm::job_failure(c, stack, DERIVE_FAILED)
}

fn digest(c: &mut Context<'_>, name: &str) -> Result<std::sync::Arc<crate::modules::crypto_digest::Digest>, RootedError> {
    super::hash::digest_by_name(c, name).map_err(|_| errors::throw(c, CryptoError::InvalidDigest(Some(name))))
}

// ---- PBKDF2 --------------------------------------------------------------------------

struct Pbkdf2Parameters {
    digest: std::sync::Arc<crate::modules::crypto_digest::Digest>,
    password: Vec<u8>,
    salt: Vec<u8>,
    iterations: u32,
    length: usize,
}

fn pbkdf2_parameters(c: &mut Context<'_>, args: &[RootId]) -> Result<Pbkdf2Parameters, RootedError> {
    let (password, salt, iterations, length, digest_name) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3), arg(c, args, 4));
    let name = super::string_argument(c, digest_name, "digest")?;
    let undefined = c.undefined();
    let password = super::binary_like(c, password, "password", undefined)?;
    let salt = super::binary_like(c, salt, "salt", undefined)?;
    let iterations = super::integer_argument(c, iterations, "iterations", 1.0, INT32_MAX)? as u32;
    let length = super::integer_argument(c, length, "keylen", 0.0, INT32_MAX)? as usize;
    let digest = digest(c, &name)?;
    Ok(Pbkdf2Parameters { digest, password, salt, iterations, length })
}

fn pbkdf2_bytes(c: &mut Context<'_>, parameters: Pbkdf2Parameters) -> Outcome {
    let Pbkdf2Parameters { digest, password, salt, iterations, length } = parameters;
    let derived = crate::modules::crypto_pbkdf2::derive(&digest, &password, &salt, iterations, length);
    let derived = derived.map_err(|stack| derive_failed(c, &stack))?;
    super::buffer(c, derived.to_vec())
}

/// `crypto.pbkdf2Sync(password, salt, iterations, keylen, digest)`.
pub(crate) fn pbkdf2_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let parameters = pbkdf2_parameters(c, args)?;
    pbkdf2_bytes(c, parameters)
}

/// `crypto.pbkdf2(password, salt, iterations, keylen, digest, callback)`.
pub(crate) fn pbkdf2(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let mut inputs: Vec<RootId> = (0..5).map(|index| arg(c, args, index)).collect();
    let mut callback = arg(c, args, 5);
    if c.is_callable_rooted(inputs[4])? {
        callback = inputs[4];
        inputs[4] = c.undefined();
    }
    let parameters = pbkdf2_parameters(c, &inputs)?;
    let callback = callback_argument(c, callback, "callback")?;
    let result = pbkdf2_bytes(c, parameters).map(|key| vec![key]);
    complete(c, callback, result)
}

// ---- scrypt --------------------------------------------------------------------------

struct ScryptParameters {
    password: Vec<u8>,
    salt: Vec<u8>,
    length: usize,
    cost: u64,
    block_size: u64,
    parallelization: u64,
    max_memory: u64,
}

/// One scrypt parameter and its long-form alias (`N`/`cost`, ...): either, never both.
fn scrypt_option(c: &mut Context<'_>, options: RootId, short: &str, long: &str, default: f64) -> Result<f64, RootedError> {
    let short_value = get(c, options, short)?;
    let mut value = default;
    let has_short = !is_undefined(c, short_value);
    if has_short {
        value = super::integer_argument(c, short_value, short, 0.0, UINT32_MAX)?;
    }
    let long_value = get(c, options, long)?;
    if !is_undefined(c, long_value) {
        if has_short {
            return Err(errors::throw(c, CryptoError::ScryptInvalidParameter));
        }
        value = super::integer_argument(c, long_value, long, 0.0, UINT32_MAX)?;
    }
    Ok(if value == 0.0 { default } else { value })
}

fn scrypt_parameters(c: &mut Context<'_>, args: &[RootId]) -> Result<ScryptParameters, RootedError> {
    let (password, salt, length, options) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let undefined = c.undefined();
    let password = super::binary_like(c, password, "password", undefined)?;
    let salt = super::binary_like(c, salt, "salt", undefined)?;
    let length = super::integer_argument(c, length, "keylen", 0.0, INT32_MAX)? as usize;
    let (mut cost, mut block_size, mut parallelization, mut max_memory) =
        (SCRYPT_COST, SCRYPT_BLOCK_SIZE, SCRYPT_PARALLELIZATION, SCRYPT_MAX_MEMORY);
    if c.truthy_rooted(options)? && c.is_object_rooted(options)? {
        cost = scrypt_option(c, options, "N", "cost", SCRYPT_COST)?;
        block_size = scrypt_option(c, options, "r", "blockSize", SCRYPT_BLOCK_SIZE)?;
        parallelization = scrypt_option(c, options, "p", "parallelization", SCRYPT_PARALLELIZATION)?;
        let memory = get(c, options, "maxmem")?;
        if !is_undefined(c, memory) {
            let maximum = crate::modules::util_shared_vm::MAX_SAFE_INTEGER;
            max_memory = super::integer_argument(c, memory, "maxmem", 0.0, maximum)?;
            if max_memory == 0.0 {
                max_memory = SCRYPT_MAX_MEMORY;
            }
        }
    }
    let parameters = ScryptParameters {
        password,
        salt,
        length,
        cost: cost as u64,
        block_size: block_size as u64,
        parallelization: parallelization as u64,
        max_memory: max_memory as u64,
    };
    // EVP_PBE_scrypt with no output checks the parameters alone.
    // SAFETY: a null key asks only for the parameter check; the slices outlive the call.
    let valid = unsafe {
        openssl_sys::EVP_PBE_scrypt(
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            parameters.cost,
            parameters.block_size,
            parameters.parallelization,
            parameters.max_memory,
            std::ptr::null_mut(),
            0,
        )
    } == 1;
    if !valid {
        let stack = openssl::error::ErrorStack::get();
        let reason = stack.errors().first().map(|error| crypto_error_shared_vm::message(error.code(), ERROR_TEXT_BYTES));
        return Err(errors::throw(c, CryptoError::InvalidScryptParams(reason)));
    }
    Ok(parameters)
}

fn scrypt_bytes(c: &mut Context<'_>, parameters: ScryptParameters) -> Outcome {
    let mut key = vec![0; parameters.length];
    if !key.is_empty() {
        let derived = openssl::pkcs5::scrypt(
            &parameters.password,
            &parameters.salt,
            parameters.cost,
            parameters.block_size,
            parameters.parallelization,
            parameters.max_memory,
            &mut key,
        );
        derived.map_err(|stack| derive_failed(c, &stack))?;
    }
    super::buffer(c, key)
}

/// `crypto.scryptSync(password, salt, keylen[, options])`.
pub(crate) fn scrypt_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let parameters = scrypt_parameters(c, args)?;
    scrypt_bytes(c, parameters)
}

/// `crypto.scrypt(password, salt, keylen[, options], callback)`.
pub(crate) fn scrypt(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let mut inputs: Vec<RootId> = (0..4).map(|index| arg(c, args, index)).collect();
    let mut callback = arg(c, args, 4);
    if c.is_callable_rooted(inputs[3])? {
        callback = inputs[3];
        inputs[3] = c.undefined();
    }
    let parameters = scrypt_parameters(c, &inputs)?;
    let callback = callback_argument(c, callback, "callback")?;
    let result = scrypt_bytes(c, parameters).map(|key| vec![key]);
    complete(c, callback, result)
}

// ---- HKDF ----------------------------------------------------------------------------

/// Node's `validateByteSource`: a string (UTF-8), an ArrayBuffer or a view.
fn byte_source(c: &mut Context<'_>, value: RootId, name: &str) -> Result<Vec<u8>, RootedError> {
    if super::is_string(c, value) {
        let undefined = c.undefined();
        return super::string_bytes(c, value, undefined);
    }
    super::buffer_bytes(c, value).ok_or_else(|| {
        errors::invalid_type(c, value, name, "of type string or an instance of ArrayBuffer, TypedArray, DataView, or Buffer")
    })
}

/// HKDF's input keying material: a secret KeyObject, a string or bytes.
fn input_key(c: &mut Context<'_>, value: RootId) -> Result<Vec<u8>, RootedError> {
    if let Some(bytes) = super::keys::key_object_secret(c, value)? {
        return Ok(bytes);
    }
    if super::is_string(c, value) {
        let undefined = c.undefined();
        return super::string_bytes(c, value, undefined);
    }
    super::buffer_bytes(c, value).ok_or_else(|| {
        let expected = "of type string or an instance of SecretKeyObject, ArrayBuffer, TypedArray, DataView, or Buffer";
        errors::invalid_type(c, value, "ikm", expected)
    })
}

struct HkdfParameters {
    digest: String,
    key: Vec<u8>,
    salt: Vec<u8>,
    info: Vec<u8>,
    length: usize,
}

fn hkdf_parameters(c: &mut Context<'_>, args: &[RootId]) -> Result<HkdfParameters, RootedError> {
    let (digest_name, key, salt, info, length) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3), arg(c, args, 4));
    let digest = super::string_argument(c, digest_name, "digest")?;
    let key = input_key(c, key)?;
    let salt = byte_source(c, salt, "salt")?;
    let info_bytes = byte_source(c, info, "info")?;
    let maximum = crate::modules::util_shared_vm::MAX_SAFE_INTEGER;
    let length = super::integer_argument(c, length, "length", 0.0, maximum)? as usize;
    if info_bytes.len() > HKDF_MAX_INFO_BYTES {
        let size = c.number(info_bytes.len() as f64);
        return Err(errors::out_of_range(c, "info", "must not contain more than 1024 bytes", size));
    }
    Ok(HkdfParameters { digest, key, salt, info: info_bytes, length })
}

/// HKDF's output bytes, for `hkdf` and WebCrypto's `deriveBits`.
pub(crate) fn hkdf_derive(c: &mut Context<'_>, digest_name: &str, key: &[u8], salt: &[u8], info: &[u8], length: usize) -> Result<Vec<u8>, RootedError> {
    let digest = digest(c, digest_name)?;
    if length > crate::modules::crypto_hkdf::maximum(&digest) {
        return Err(errors::throw(c, CryptoError::InvalidKeyLength));
    }
    let derived = crate::modules::crypto_hkdf::derive(&digest, key, salt, info, length);
    Ok(derived.map_err(|stack| derive_failed(c, &stack))?.to_vec())
}

/// PBKDF2's output bytes, for WebCrypto's `deriveBits`.
pub(crate) fn pbkdf2_derive(c: &mut Context<'_>, digest_name: &str, password: &[u8], salt: &[u8], iterations: u32, length: usize) -> Result<Vec<u8>, RootedError> {
    let digest = digest(c, digest_name)?;
    let derived = crate::modules::crypto_pbkdf2::derive(&digest, password, salt, iterations, length);
    Ok(derived.map_err(|stack| derive_failed(c, &stack))?.to_vec())
}

fn hkdf_bytes(c: &mut Context<'_>, parameters: HkdfParameters) -> Outcome {
    let HkdfParameters { digest, key, salt, info, length } = parameters;
    let bytes = hkdf_derive(c, &digest, &key, &salt, &info, length)?;
    super::array_buffer(c, bytes)
}

/// `crypto.hkdfSync(digest, ikm, salt, info, keylen)`.
pub(crate) fn hkdf_sync(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let parameters = hkdf_parameters(c, args)?;
    hkdf_bytes(c, parameters)
}

/// `crypto.hkdf(digest, ikm, salt, info, keylen, callback)`.
pub(crate) fn hkdf(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let parameters = hkdf_parameters(c, args)?;
    let callback = arg(c, args, 5);
    let callback = callback_argument(c, callback, "callback")?;
    let result = hkdf_bytes(c, parameters).map(|key| vec![key]);
    complete(c, callback, result)
}
