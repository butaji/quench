//! Shared-VM TextDecoder facade over the Node-owned decoder semantics.

use crate::host::NodeHost;
use quench_runtime_next::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(canonicalEncoding, decodeBytes) => {
  const decoders = new WeakMap();
  const stateFor = (receiver) => {
    const state = decoders.get(receiver);
    if (state === undefined) {
      throw new TypeError("Illegal invocation");
    }
    return state;
  };

  class TextDecoder {
    constructor(label = "utf-8", options = {}) {
      const requested = String(label);
      const encoding = canonicalEncoding(requested);
      if (encoding === undefined) {
        throw new RangeError(`The encoding label provided ('${requested}') is invalid.`);
      }
      decoders.set(this, {
        encoding,
        fatal: Boolean(options?.fatal),
        ignoreBOM: Boolean(options?.ignoreBOM),
      });
    }

    get encoding() { return stateFor(this).encoding; }
    get fatal() { return stateFor(this).fatal; }
    get ignoreBOM() { return stateFor(this).ignoreBOM; }

    decode(input = undefined) {
      const state = stateFor(this);
      let view;
      if (input === undefined) {
        view = new Uint8Array(0);
      } else if (input instanceof ArrayBuffer ||
                 Object.prototype.toString.call(input) === "[object ArrayBuffer]") {
        view = new Uint8Array(input);
      } else if (ArrayBuffer.isView(input)) {
        view = new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
      } else {
        throw new TypeError(
          'The "input" argument must be an instance of ArrayBuffer or ArrayBufferView.'
        );
      }
      const decoded = decodeBytes(Array.from(view), state.encoding, state.fatal);
      return !state.ignoreBOM && decoded.startsWith("\uFEFF")
        ? decoded.slice(1)
        : decoded;
    }
  }

  return TextDecoder;
}"#
);

pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let canonical = context.host_function(crate::host::shared_vm::operation(
        "textDecoderCanonicalEncoding",
    ))?;
    let decode = context.host_function(crate::host::shared_vm::operation("textDecoderDecode"))?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:text-decoder/shared-vm.js")?;
    let undefined = context.undefined();
    let constructor = context.call_rooted(factory, undefined, &[canonical, decode])?;
    let install = context.evaluate_script_rooted(
        "(value) => Object.defineProperty(globalThis, 'TextDecoder', { value, writable: true, configurable: true })",
        "node:text-decoder/install-global.js",
    )?;
    context.call_rooted(install, undefined, &[constructor])?;
    Ok(())
}

pub(crate) fn canonical_encoding(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(root) = args.first().copied() else {
        return Ok(context.undefined());
    };
    let Some(label) = context.string_text(root)? else {
        return Ok(context.undefined());
    };
    Ok(match super::canonical_encoding(&label) {
        Some(encoding) => context.string_rooted(encoding),
        None => context.undefined(),
    })
}

pub(crate) fn decode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let bytes = byte_array(context, args.first().copied())?;
    let Some(encoding_root) = args.get(1).copied() else {
        return Err(RootedError::host("TextDecoder encoding is missing"));
    };
    let encoding = context
        .string_text(encoding_root)?
        .ok_or_else(|| RootedError::host("TextDecoder encoding is not a string"))?;
    let fatal = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let Some(text) = super::decode_bytes(&encoding, &bytes, fatal).ok() else {
        let error = context.type_error_rooted("The encoded data was not valid UTF-8")?;
        return Err(context.throw(error));
    };
    Ok(context.string_rooted(&text))
}

fn byte_array(
    context: &mut NativeContext<'_, NodeHost>,
    root: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let Some(root) = root else {
        return Ok(Vec::new());
    };
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(root, length_key)?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default()
        .max(0.0) as usize;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let key = context.string_rooted(&index.to_string());
        let value = context.get_property_rooted(root, key)?;
        bytes.push(
            context
                .rooted_value(value)
                .and_then(|value| value.as_number())
                .unwrap_or_default() as u8,
        );
    }
    Ok(bytes)
}
