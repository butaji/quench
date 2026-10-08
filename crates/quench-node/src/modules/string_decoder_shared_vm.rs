//! Shared-VM StringDecoder facade over the Node-owned byte decoder.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(decodeChunk, canonicalEncoding, Buffer, maxStringBytes) => {
  const decoders = new WeakMap();
  const codedTypeError = (message, code) => {
    const error = new TypeError(message);
    error.code = code;
    return error;
  };
  const unitsToString = (units) => {
    let text = "";
    const chunkSize = 0x4000;
    for (let offset = 0; offset < units.length; offset += chunkSize) {
      text += String.fromCharCode(...units.slice(offset, offset + chunkSize));
    }
    return text;
  };
  const byteView = (value) => {
    if (!ArrayBuffer.isView(value)) {
      let received = "an invalid value";
      if (value === null) received = "null";
      else if (value === undefined) received = "undefined";
      else if (typeof value === "boolean") received = "a boolean";
      else if (typeof value === "number") received = "a number";
      else if (typeof value === "string") received = "a string";
      throw codedTypeError(
        `The "buf" argument must be an instance of Buffer, TypedArray, or DataView. Received ${received}`,
        "ERR_INVALID_ARG_TYPE",
      );
    }
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  };
  const assertStringSize = (pendingLength, inputLength) => {
    if (pendingLength + inputLength > maxStringBytes) {
      throw codedTypeError(
        "Cannot create a string longer than the maximum allowed length",
        "ERR_STRING_TOO_LONG",
      );
    }
  };
  const updateLastFields = (decoder, state, lastTotal) => {
    decoder.lastTotal = lastTotal;
    decoder.lastNeed = Math.max(0, lastTotal - state.pending.length);
    decoder.lastChar.fill(0);
    decoder.lastChar.set(state.pending.slice(0, decoder.lastChar.length));
  };
  const decode = (state, bytes, final) => decodeChunk(
    state.pending,
    bytes,
    state.encoding,
    final,
  );
  const stateFor = (receiver) => {
    const state = decoders.get(receiver);
    if (state === undefined) {
      throw codedTypeError(
        "Cannot call StringDecoder method on an incompatible receiver",
        "ERR_INVALID_THIS",
      );
    }
    return state;
  };

  function StringDecoder(encoding) {
    if ((this === undefined || this === globalThis) && !new.target) {
      return new StringDecoder(encoding);
    }
    const requested = encoding === undefined ? "utf8" : String(encoding);
    const canonical = canonicalEncoding(requested);
    if (canonical === undefined) {
      throw codedTypeError(`Unknown encoding: ${requested}`, "ERR_UNKNOWN_ENCODING");
    }
    const state = { encoding: canonical, pending: [] };
    decoders.set(this, state);
    this.encoding = canonical;
    this.lastNeed = 0;
    this.lastTotal = 0;
    this.lastChar = Buffer.alloc(4);
    return this;
  }

  StringDecoder.prototype.write = function (input) {
    const state = stateFor(this);
    const view = byteView(input);
    assertStringSize(state.pending.length, view.byteLength);
    const bytes = Array.from(view);
    const result = decode(state, bytes, false);
    state.pending = result.pending;
    updateLastFields(this, state, result.lastTotal);
    return unitsToString(result.units);
  };

  StringDecoder.prototype.end = function (input) {
    const state = stateFor(this);
    const view = input === undefined ? undefined : byteView(input);
    assertStringSize(state.pending.length, view === undefined ? 0 : view.byteLength);
    const bytes = view === undefined ? [] : Array.from(view);
    const result = decode(state, bytes, true);
    state.pending = result.pending;
    updateLastFields(this, state, result.lastTotal);
    return unitsToString(result.units);
  };

  StringDecoder.prototype.text = function (input, offset = 0) {
    stateFor(this);
    const view = byteView(input);
    const start = Math.max(0, Math.trunc(Number(offset)) || 0);
    const result = decodeChunk([], Array.from(view.subarray(start)), "utf8", true);
    return unitsToString(result.units);
  };

  return { StringDecoder };
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(FACTORY, "node:string_decoder/shared.js")?;
    let decode = context.host_function(crate::host::shared_vm::operation("stringDecoderChunk"))?;
    let canonical =
        context.host_function(crate::host::shared_vm::operation("bufferCanonicalEncoding"))?;
    let global = context.global_root()?;
    let buffer_name = context.string_rooted("Buffer");
    let buffer = context.get_property_rooted(global, buffer_name)?;
    let max_string_bytes =
        context.number(crate::modules::string_decoder_codec::MAX_STRING_BYTES as f64);
    let undefined = context.undefined();
    context.call_rooted(
        factory,
        undefined,
        &[decode, canonical, buffer, max_string_bytes],
    )
}

pub(crate) fn decode_chunk(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let prior_pending = byte_array(context, args.first().copied())?;
    let input = byte_array(context, args.get(1).copied())?;
    let encoding = args
        .get(2)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("StringDecoder encoding is not a string"))?;
    let final_input = args
        .get(3)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let mode = if final_input {
        crate::modules::string_decoder_codec::DecodeMode::Final
    } else {
        crate::modules::string_decoder_codec::DecodeMode::Streaming
    };
    let decoded = crate::modules::string_decoder_codec::decode_chunk_units(
        &prior_pending,
        &input,
        &encoding,
        mode,
    );
    let object = context.object_rooted()?;
    let units = decoded
        .units
        .iter()
        .map(|unit| context.number(f64::from(*unit)))
        .collect::<Vec<_>>();
    let units = context.array_rooted(&units)?;
    set(context, object, "units", units)?;
    let pending = decoded
        .pending
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let pending = context.array_rooted(&pending)?;
    set(context, object, "pending", pending)?;
    let total = context.number(decoded.last_total as f64);
    set(context, object, "lastTotal", total)?;
    Ok(object)
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

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install shared StringDecoder property {name}"
        )))
    }
}
