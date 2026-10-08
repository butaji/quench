//! The hash surface used by the pinned framework package paths.
//!
//! `etag` calls `createHash("sha1").update(string, "utf8").digest("base64")`.
//! Keep that Node-facing wrapper in the guest realm and the digest primitive
//! in Rust; it shares the SHA-1 implementation with the legacy Node module.

use crate::host::NodeHost;
use quench_runtime_next::{NativeContext, RootId, RootedError};

const CRYPTO_FACTORY: &str = quench_js_check::checked_js!(
    r#"(sha1, Buffer) => {
  const states = new WeakMap();
  const unsupportedDigest = (algorithm) => {
    const error = new Error(`Digest method not supported: ${algorithm}`);
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
      if (name !== "sha1" && name !== "sha-1") throw unsupportedDigest(algorithm);
      states.set(this, { chunks: [], lifecycle: "open" });
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
      state.lifecycle = "finalized";
      const input = state.chunks.flat();
      state.chunks = [];
      const bytes = Buffer.from(sha1(input));
      return encoding === undefined || encoding === "buffer"
        ? bytes
        : bytes.toString(encoding);
    }
  }

  return { createHash: (algorithm) => new Hash(algorithm) };
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(CRYPTO_FACTORY, "node:crypto/shared.js")?;
    let sha1 = context.host_function(crate::host::shared_vm::operation("cryptoHashSha1"))?;
    let global = context.global_root()?;
    let buffer = get(context, global, "Buffer")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[sha1, buffer])
}

pub(crate) fn sha1(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(input) = args.first().copied() else {
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

    let digest = crate::modules::crypto_sha1::digest(&bytes);
    let values = digest
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
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
