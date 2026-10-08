//! Minimal shared CommonJS hash surface used by framework-owned HTTP ETags.

use crate::host::NodeHost;
use base64::Engine;
use rqj::{NativeContext, RootId, RootedError};
use sha2::digest::Digest;

type Context<'a> = NativeContext<'a, NodeHost>;

const MODULE_FACTORY: &str = r#"((digest) => ({
  createHash(algorithm) {
    let input = '';
    const hash = {
      update(chunk, encoding = 'utf8') {
        if (typeof chunk === 'string') {
          input += chunk;
        } else if (chunk && typeof chunk.toString === 'function') {
          input += chunk.toString(encoding);
        } else {
          throw new TypeError('shared crypto hash input must be a string');
        }
        return hash;
      },
      digest(encoding = 'hex') {
        return digest(algorithm, input, encoding);
      },
    };
    return hash;
  },
}) )"#;

pub(crate) fn module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let digest = context.host_function(crate::host::shared_vm::operation("cryptoHashDigest"))?;
    let factory = context.evaluate_script_rooted(MODULE_FACTORY, "node:crypto/shared-hash.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[digest])
}

pub(crate) fn hash_digest(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_default()
        .to_ascii_lowercase();
    let input = args
        .get(1)
        .copied()
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_default();
    let encoding = args
        .get(2)
        .copied()
        .filter(|root| {
            context
                .rooted_value(*root)
                .is_some_and(|value| !value.is_undefined())
        })
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_else(|| "hex".to_owned())
        .to_ascii_lowercase();
    let bytes = match algorithm.as_str() {
        "sha1" => sha1::Sha1::digest(input.as_bytes()).to_vec(),
        "sha224" => sha2::Sha224::digest(input.as_bytes()).to_vec(),
        "sha256" => sha2::Sha256::digest(input.as_bytes()).to_vec(),
        "sha384" => sha2::Sha384::digest(input.as_bytes()).to_vec(),
        "sha512" => sha2::Sha512::digest(input.as_bytes()).to_vec(),
        _ => {
            let error = context.type_error_rooted("Digest method not supported")?;
            let code = context.string_rooted("ERR_CRYPTO_INVALID_DIGEST");
            let key = context.string_rooted("code");
            let accepted = context.set_property_rooted(error, key, code, error)?;
            if !accepted {
                return Err(RootedError::host("cannot set crypto digest error code"));
            }
            return Err(context.throw(error));
        }
    };
    let result = match encoding.as_str() {
        "hex" => hex::encode(bytes),
        "base64" => base64::engine::general_purpose::STANDARD.encode(bytes),
        _ => {
            let error = context
                .type_error_rooted("Unknown encoding: digest encoding must be 'hex' or 'base64'")?;
            let code = context.string_rooted("ERR_UNKNOWN_ENCODING");
            let key = context.string_rooted("code");
            let accepted = context.set_property_rooted(error, key, code, error)?;
            if !accepted {
                return Err(RootedError::host("cannot set crypto encoding error code"));
            }
            return Err(context.throw(error));
        }
    };
    Ok(context.string_rooted(&result))
}
