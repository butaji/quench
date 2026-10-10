//! The hash surface used by the pinned framework package paths.
//!
//! `etag` calls `createHash("sha1").update(string, "utf8").digest("base64")`.
//! Keep that Node-facing wrapper in the guest realm and the digest primitive
//! in Rust; it shares the SHA-1 implementation with the legacy Node module.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const CRYPTO_FACTORY: &str = quench_js_check::checked_js!(
    r#"(hashDigest, hmacDigest, Buffer, randomBytes) => {
  const states = new WeakMap();
  const secretKeys = new WeakMap();
  let repeatedHmacDigestWarningEmitted = false;
  const unsupportedDigest = (algorithm) => {
    const error = new Error(`Invalid digest: ${algorithm}`);
    error.code = "ERR_OSSL_EVP_UNSUPPORTED";
    return error;
  };
  const finalized = () => {
    const error = new Error("Digest already called");
    error.code = "ERR_CRYPTO_HASH_FINALIZED";
    return error;
  };

  class Hash {
    constructor(algorithm) {
      if (typeof algorithm !== "string") {
        const error = new TypeError('The "algorithm" argument must be of type string');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const name = algorithm.toLowerCase();
      if (!getHashes().includes(algorithm) && !getHashes().includes(name)) throw unsupportedDigest(algorithm);
      states.set(this, { name, chunks: [], lifecycle: "open" });
    }

    update(data, encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      const bytes = typeof data === "string"
        ? Buffer.from(data, encoding === undefined ? "utf8" : encoding)
        : Buffer.from(data);
      state.chunks.push(Array.from(bytes));
      return this;
    }

    digest(encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      const outputEncoding = encoding === undefined || encoding === "buffer"
        ? undefined
        : String(encoding);
      state.lifecycle = "finalized";
      const input = state.chunks.flat();
      state.chunks = [];
      const bytes = Buffer.from(hashDigest(state.name, input));
      return outputEncoding === undefined
        ? bytes
        : bytes.toString(outputEncoding);
    }

    end(data, encoding) {
      if (data !== undefined) this.update(data, encoding);
      const state = states.get(this);
      state.streamResult = this.digest();
      return this;
    }

    read() {
      const state = states.get(this);
      const result = state.streamResult;
      state.streamResult = undefined;
      return result;
    }
  }

  class Hmac {
    constructor(algorithm, key) {
      if (typeof algorithm !== "string") {
        const error = new TypeError(`The "hmac" argument must be of type string. Received ${String(algorithm)}`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const requestedName = algorithm.toLowerCase();
      const name = requestedName === "dss1" ? "sha1" : requestedName;
      if (!getHashes().includes(algorithm) && !getHashes().includes(name)) throw unsupportedDigest(algorithm);
      if (key === undefined || key === null) {
        const error = new TypeError('The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const keyBytes = secretKeys.has(key)
        ? Buffer.from(secretKeys.get(key))
        : Buffer.from(key);
      states.set(this, { name, key: Array.from(keyBytes), chunks: [], lifecycle: "open" });
    }

    update(data, encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      const bytes = typeof data === "string"
        ? Buffer.from(data, encoding === undefined ? "utf8" : encoding)
        : Buffer.from(data);
      state.chunks.push(Array.from(bytes));
      return this;
    }

    digest(encoding) {
      const state = states.get(this);
      const outputEncoding = encoding === undefined || encoding === "buffer"
        ? undefined
        : String(encoding);
      if (state.lifecycle !== "open") {
        if (!repeatedHmacDigestWarningEmitted) {
          repeatedHmacDigestWarningEmitted = true;
          process.emitWarning("Calling Hmac.digest() more than once is deprecated.", {
            type: "DeprecationWarning",
            code: "DEP0206",
          });
        }
        const empty = Buffer.alloc(0);
        return outputEncoding === undefined ? empty : empty.toString(outputEncoding);
      }
      state.lifecycle = "finalized";
      const input = state.chunks.flat();
      state.chunks = [];
      const bytes = Buffer.from(hmacDigest(state.name, state.key, input));
      return outputEncoding === undefined
        ? bytes
        : bytes.toString(outputEncoding);
    }

    end(data, encoding) {
      if (data !== undefined) this.update(data, encoding);
      const state = states.get(this);
      state.streamResult = this.digest();
      return this;
    }

    read() {
      const state = states.get(this);
      const result = state.streamResult;
      state.streamResult = undefined;
      return result;
    }
  }

  const hashNames = Object.freeze([
    "RSA-SHA1", "blake2b512", "blake2s256", "md5", "ripemd160",
    "sha1", "sha224", "sha256", "sha384", "sha512",
    "sha3-224", "sha3-256", "sha3-384", "sha3-512",
  ].sort());
  function getHashes() { return hashNames.slice(); }
  function createSecretKey(key) {
    const bytes = Buffer.from(key);
    const result = Object.create(null);
    secretKeys.set(result, Array.from(bytes));
    Object.defineProperties(result, {
      type: { value: "secret", enumerable: true },
      symmetricKeySize: { get() { return secretKeys.get(this).length; }, enumerable: true },
      export: { value() { return Buffer.from(secretKeys.get(this)); } },
    });
    return result;
  }

  const randomBuffer = (size) => Buffer.from(randomBytes(size));
  const randomFillSync = (buffer, offset = 0, size = buffer.length - offset) => {
    const bytes = randomBytes(size);
    for (let i = 0; i < size; i++) buffer[offset + i] = bytes[i];
    return buffer;
  };
  const randomUUID = () => {
    const bytes = randomBuffer(16);
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    const hex = bytes.toString("hex");
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  };
  return {
    createHash: (algorithm) => new Hash(algorithm),
    createHmac: (algorithm, key) => new Hmac(algorithm, key),
    createSecretKey,
    getFips: () => 0,
    getHashes,
    randomBytes: randomBuffer,
    randomFillSync,
    randomUUID,
  };
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(CRYPTO_FACTORY, "node:crypto/shared.js")?;
    let hash = context.host_function(crate::host::shared_vm::operation("cryptoHash"))?;
    let hmac = context.host_function(crate::host::shared_vm::operation("cryptoHmac"))?;
    let random_bytes = context.host_function(crate::host::shared_vm::operation("cryptoRandomBytes"))?;
    let global = context.global_root()?;
    let buffer = get(context, global, "Buffer")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[hash, hmac, buffer, random_bytes])
}

pub(crate) fn random_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(size) = args.first().copied().and_then(|size| context.rooted_value(size)).and_then(|value| value.as_number()) else {
        return Err(type_error(context, "The \"size\" argument must be of type number" )?);
    };
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 || size > 16_777_216.0 {
        return Err(type_error(context, "The \"size\" argument must be a non-negative integer" )?);
    }
    let mut bytes = vec![0_u8; size as usize];
    openssl::rand::rand_bytes(&mut bytes).map_err(|_| RootedError::host("crypto random generation failed"))?;
    let values = bytes.into_iter().map(|byte| context.number(f64::from(byte))).collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn hash(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args.first().copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto hash algorithm is not a string"))?;
    let Some(input) = args.get(1).copied() else {
        return Err(type_error(
            context,
            "The hash input must be an array of bytes",
        )?);
    };
    let length = get(context, input, "length")?;
    let Some(length) = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
    else {
        return Err(type_error(
            context,
            "The hash input must be an array of bytes",
        )?);
    };
    let Ok(length) = usize::try_from(length as u64) else {
        return Err(type_error(context, "The hash input is too large")?);
    };
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let byte = get(context, input, &index.to_string())?;
        let Some(byte) = context
            .rooted_value(byte)
            .and_then(|value| value.as_number())
            .filter(|byte| byte.is_finite() && *byte >= 0.0 && *byte <= f64::from(u8::MAX))
        else {
            return Err(type_error(
                context,
                "The hash input must contain byte values",
            )?);
        };
        bytes.push(byte as u8);
    }

    let digest = if algorithm.eq_ignore_ascii_case("sha1") {
        crate::modules::crypto_sha1::digest(&bytes)
    } else {
        let algorithm = openssl::hash::MessageDigest::from_name(&algorithm)
            .ok_or_else(|| RootedError::host("Digest method not supported"))?;
        openssl::hash::hash(algorithm, &bytes)
            .map_err(|_| RootedError::host("crypto digest failed"))?
            .to_vec()
    };
    let values = digest
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn hmac(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args.first().copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto HMAC algorithm is not a string"))?;
    let key_root = args.get(1).copied()
        .ok_or_else(|| RootedError::host("crypto HMAC key is missing"))?;
    let input_root = args.get(2).copied()
        .ok_or_else(|| RootedError::host("crypto HMAC input is missing"))?;
    let key = byte_array(context, key_root)?;
    let input = byte_array(context, input_root)?;
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    // OpenSSL rejects a zero-length HMAC key. A single zero byte has the same
    // block-padded HMAC key representation as an empty key.
    let openssl_key = if key.is_empty() { vec![0] } else { key };
    let key = openssl::pkey::PKey::hmac(&openssl_key)
        .map_err(|error| RootedError::host(format!("crypto HMAC key is invalid ({} bytes): {error}", openssl_key.len())))?;
    let mut signer = openssl::sign::Signer::new(digest, &key)
        .map_err(|error| RootedError::host(format!("crypto HMAC initialization failed: {error}")))?;
    signer.update(&input)
        .map_err(|_| RootedError::host("crypto HMAC update failed"))?;
    let output = signer.sign_to_vec()
        .map_err(|_| RootedError::host("crypto HMAC failed"))?;
    let values = output.iter().map(|byte| context.number(f64::from(*byte))).collect::<Vec<_>>();
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
    let length = context.rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto byte input is not array-like"))? as usize;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let value = get(context, root, &index.to_string())?;
        let byte = context.rooted_value(value)
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
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn type_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(message)?;
    Ok(context.throw(error))
}
