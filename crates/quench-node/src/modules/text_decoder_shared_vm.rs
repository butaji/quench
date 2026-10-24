//! Shared-VM TextDecoder facade over the Node-owned decoder semantics.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

#[cfg(not(quench_icu))]
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
      return decodeBytes(Array.from(view), state.encoding, state.fatal, state.ignoreBOM);
    }
  }

  class TextEncoder {
    get encoding() { return "utf-8"; }
    get fatal() { return false; }
    get ignoreBOM() { return true; }
    encode(input = "") {
      return new Uint8Array(globalThis.Buffer.from(String(input), "utf8"));
    }
    encodeInto(input, destination) {
      if (!(destination instanceof Uint8Array)) {
        throw new TypeError('The "destination" argument must be an instance of Uint8Array');
      }
      const text = String(input);
      let read = 0;
      let written = 0;
      for (const character of text) {
        const bytes = globalThis.Buffer.from(character, "utf8");
        if (written + bytes.length > destination.length) break;
        destination.set(bytes, written);
        written += bytes.length;
        read += character.length;
      }
      return { read, written };
    }
  }

  return { TextDecoder, TextEncoder };
}"#
);

/// Installs `TextDecoder` and `TextEncoder` as lazy data properties, built on first use.
pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let initializer =
        context.host_function(crate::host::shared_vm::operation("textDecoderInitialize"))?;
    let global = context.global_root()?;
    context.define_lazy_data_properties_rooted(
        global,
        &["TextDecoder", "TextEncoder"],
        false,
        initializer,
    )?;
    context.release_root(initializer);
    Ok(())
}

pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(quench_icu)]
    let constructors = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "util.js",
        "internal/encoding.js",
    )?;
    #[cfg(not(quench_icu))]
    let constructors = {
        let canonical = context.host_function(crate::host::shared_vm::operation(
            "textDecoderCanonicalEncoding",
        ))?;
        let decode =
            context.host_function(crate::host::shared_vm::operation("textDecoderDecode"))?;
        let factory = context.evaluate_script_rooted(FACTORY, "node:text-decoder/shared-vm.js")?;
        let undefined = context.undefined();
        let constructors = context.call_rooted(factory, undefined, &[canonical, decode])?;
        constructors
    };
    let undefined = context.undefined();
    let decoder_key = context.string_rooted("TextDecoder");
    let constructor = context.get_property_rooted(constructors, decoder_key)?;
    let encoder_key = context.string_rooted("TextEncoder");
    let encoder = context.get_property_rooted(constructors, encoder_key)?;
    let install = context.evaluate_script_rooted(
        "(decoder, encoder) => { Object.defineProperty(globalThis, 'TextDecoder', { value: decoder, writable: true, configurable: true }); Object.defineProperty(globalThis, 'TextEncoder', { value: encoder, writable: true, configurable: true }); }",
        "node:text-decoder/install-global.js",
    )?;
    context.call_rooted(install, undefined, &[constructor, encoder])?;
    Ok(undefined)
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
    match crate::modules::text_decoder_codec::canonical_encoding(&label) {
        Some(encoding) if encoding != "replacement" => Ok(context.string_rooted(&encoding)),
        encoding => {
            let label = if encoding.is_some() {
                "replacement".encode_utf16().collect()
            } else {
                context.to_string_code_units(root)?
            };
            let mut units = "The \"".encode_utf16().collect::<Vec<_>>();
            units.extend(label);
            units.extend("\" encoding is not supported".encode_utf16());
            let message = context.string_units_rooted(&units);
            let error = context.range_error_rooted("")?;
            context.assign_named_property_rooted(error, "message", message)?;
            super::node_errors_shared_vm::decorate_range_error(
                context,
                error,
                "ERR_ENCODING_NOT_SUPPORTED",
            )?;
            Err(context.throw(error))
        }
    }
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
    let Some(text) =
        crate::modules::text_decoder_codec::decode_bytes(&encoding, &bytes, fatal).ok()
    else {
        // Node shares the ISO-8859-8 decoder with its logical-order alias,
        // including that decoder's canonical name in fatal diagnostics.
        let failure_encoding = if encoding == "iso-8859-8-i" {
            "iso-8859-8"
        } else {
            &encoding
        };
        let error = context.type_error_rooted(&format!(
            "The encoded data was not valid for encoding {failure_encoding}"
        ))?;
        super::node_errors_shared_vm::decorate_type_error(
            context,
            error,
            "ERR_ENCODING_INVALID_ENCODED_DATA",
        )?;
        return Err(context.throw(error));
    };
    let ignore_bom = args
        .get(3)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let text = if !ignore_bom && matches!(encoding.as_str(), "utf-8" | "utf-16le" | "utf-16be") {
        text.strip_prefix('\u{feff}').unwrap_or(&text)
    } else {
        &text
    };
    Ok(context.string_rooted(text))
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
