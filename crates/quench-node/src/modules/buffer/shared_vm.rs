//! Shared-VM Buffer values are realm-owned Uint8Array views.

use crate::host::NodeHost;
use quench_runtime::value::Value as LegacyValue;
use rqj::{NativeContext, RootId, RootedError};

const BUFFER_FACTORY: &str = quench_js_check::checked_js!(
    r#"(encode, decode, canonicalEncoding) => {
  const codedTypeError = (message, code) => {
    const error = new TypeError(message);
    error.code = code;
    return error;
  };
  const normalizeEncoding = (encoding) => {
    const requested = encoding || "utf8";
    const normalized = canonicalEncoding(String(requested));
    if (normalized === undefined) {
      throw codedTypeError(`Unknown encoding: ${encoding}`, "ERR_UNKNOWN_ENCODING");
    }
    return normalized;
  };
  const makeBuffer = (bytes) => new Buffer(bytes);

  class Buffer extends Uint8Array {
    static from(value, encoding, length) {
      if (typeof value === "string") {
        return makeBuffer(encode(value, normalizeEncoding(encoding)));
      }
      return new Buffer(value, encoding, length);
    }

    static alloc(size, fill = 0, encoding) {
      if (typeof size !== "number") {
        throw codedTypeError('The "size" argument must be of type number', "ERR_INVALID_ARG_TYPE");
      }
      if (!Number.isFinite(size) || size < 0 || size > 0x7fffffff) {
        const error = new RangeError("The value of \"size\" is out of range");
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      const result = new Buffer(Math.trunc(size));
      if (typeof fill === "string") {
        const pattern = Buffer.from(fill, encoding);
        if (pattern.length > 0) {
          for (let index = 0; index < result.length; index++) {
            result[index] = pattern[index % pattern.length];
          }
        }
      } else {
        result.fill(fill);
      }
      return result;
    }

    static allocUnsafe(size) {
      return Buffer.alloc(size);
    }

    static allocUnsafeSlow(size) {
      return Buffer.alloc(size);
    }

    static concat(list, totalLength) {
      if (!Array.isArray(list)) {
        throw codedTypeError('The "list" argument must be an instance of Array', "ERR_INVALID_ARG_TYPE");
      }
      const length = totalLength === undefined
        ? list.reduce((sum, item) => sum + item.byteLength, 0)
        : Math.max(0, Math.trunc(Number(totalLength)) || 0);
      const result = new Buffer(length);
      let offset = 0;
      for (const item of list) {
        if (!ArrayBuffer.isView(item) || !(item instanceof Uint8Array)) {
          throw codedTypeError(`The "list[${list.indexOf(item)}]" argument must be an instance of Buffer or Uint8Array`, "ERR_INVALID_ARG_TYPE");
        }
        const count = Math.min(item.byteLength, result.length - offset);
        result.set(new Uint8Array(item.buffer, item.byteOffset, count), offset);
        offset += count;
        if (offset === result.length) break;
      }
      return result;
    }

    static byteLength(value, encoding = "utf8") {
      if (typeof value === "string") {
        return encode(value, normalizeEncoding(encoding)).length;
      }
      if (value === null || value === undefined) {
        throw codedTypeError('The "string" argument must be of type string or an instance of Buffer or ArrayBuffer', "ERR_INVALID_ARG_TYPE");
      }
      return value.byteLength;
    }

    static isBuffer(value) {
      return value instanceof Buffer;
    }

    static isEncoding(encoding) {
      return typeof encoding === "string" && canonicalEncoding(encoding) !== undefined;
    }

    toString(encoding = "utf8", start = 0, end = this.length) {
      const normalized = normalizeEncoding(encoding);
      const index = (value, fallback) => {
        const number = Math.trunc(Number(value));
        if (Number.isNaN(number)) return fallback;
        return Math.max(0, Math.min(this.length, number < 0 ? this.length + number : number));
      };
      const first = index(start, 0);
      const last = Math.max(first, index(end, this.length));
      return decode(Array.from(this.subarray(first, last)), normalized);
    }

    slice(start = 0, end = this.length) {
      return this.subarray(start, end);
    }

    equals(other) {
      if (!(other instanceof Uint8Array)) {
        throw codedTypeError(
          'The "otherBuffer" argument must be an instance of Buffer or Uint8Array',
          "ERR_INVALID_ARG_TYPE",
        );
      }
      if (other.byteLength !== this.byteLength) return false;
      for (let index = 0; index < this.byteLength; index++) {
        if (this[index] !== other[index]) return false;
      }
      return true;
    }
  }

  for (const name of [
    'from', 'alloc', 'allocUnsafe', 'allocUnsafeSlow', 'concat',
    'byteLength', 'isBuffer', 'isEncoding',
  ]) {
    const descriptor = Object.getOwnPropertyDescriptor(Buffer, name);
    Object.defineProperty(Buffer, name, { ...descriptor, enumerable: true });
  }

  return { Buffer, SlowBuffer: Buffer };
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(BUFFER_FACTORY, "node:buffer/shared.js")?;
    let encode = context.host_function(crate::host::shared_vm::operation("bufferEncode"))?;
    let decode = context.host_function(crate::host::shared_vm::operation("bufferDecode"))?;
    let canonical_encoding =
        context.host_function(crate::host::shared_vm::operation("bufferCanonicalEncoding"))?;
    let undefined = context.undefined();
    let constructor =
        context.call_rooted(factory, undefined, &[encode, decode, canonical_encoding])?;

    let module = context.object_rooted()?;
    let buffer = get(context, constructor, "Buffer")?;
    set(context, module, "Buffer", buffer)?;
    let slow_buffer = get(context, constructor, "SlowBuffer")?;
    set(context, module, "SlowBuffer", slow_buffer)?;
    let global = context.global_root()?;
    for name in ["atob", "btoa"] {
        if let Some(value) = optional_get(context, global, name)? {
            set(context, module, name, value)?;
        }
    }
    Ok(module)
}

pub(crate) fn install_global(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    let constructor = get(context, module, "Buffer")?;
    let installer = context.evaluate_script_rooted(
        "(value) => Object.defineProperty(globalThis, 'Buffer', { value, writable: true, configurable: true })",
        "node:buffer/install-global.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(installer, undefined, &[constructor])?;
    Ok(())
}

pub(crate) fn encode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(input) = args.first().copied() else {
        return Err(type_error(
            context,
            "The \"string\" argument must be of type string",
            "ERR_INVALID_ARG_TYPE",
        )?);
    };
    let Some(input) = context.string_text(input)? else {
        return Err(type_error(
            context,
            "The \"string\" argument must be of type string",
            "ERR_INVALID_ARG_TYPE",
        )?);
    };
    let encoding_name = encoding(context, args.get(1).copied())?;
    let Some(encoding) = crate::modules::buffer_enc::canonical_encoding(&encoding_name) else {
        return Err(type_error(
            context,
            &format!("Unknown encoding: {encoding_name}"),
            "ERR_UNKNOWN_ENCODING",
        )?);
    };
    let bytes = crate::modules::buffer_enc::encode_str(&input, encoding);
    number_array(context, &bytes)
}

pub(crate) fn decode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(values) = args.first().copied() else {
        return Err(type_error(
            context,
            "The bytes argument must be an array",
            "ERR_INVALID_ARG_TYPE",
        )?);
    };
    let encoding = encoding(context, args.get(1).copied())?;
    let Some(canonical) = crate::modules::buffer_enc::canonical_encoding(&encoding) else {
        return Err(type_error(
            context,
            &format!("Unknown encoding: {encoding}"),
            "ERR_UNKNOWN_ENCODING",
        )?);
    };
    let length = get(context, values, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default()
        .max(0.0) as usize;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let value = get(context, values, &index.to_string())?;
        bytes.push(
            context
                .rooted_value(value)
                .and_then(|value| value.as_number())
                .unwrap_or_default() as u8,
        );
    }
    let text = match crate::modules::buffer_enc::decode_str(&bytes, canonical) {
        LegacyValue::String(text) => text,
        LegacyValue::StringUnits(units) => String::from_utf16_lossy(&units),
        _ => unreachable!("Buffer decoding always returns a string"),
    };
    Ok(context.string_rooted(&text))
}

pub(crate) fn canonical_encoding(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(input) = args.first().copied() else {
        return Ok(context.undefined());
    };
    let Some(name) = context.string_text(input)? else {
        return Ok(context.undefined());
    };
    Ok(
        match crate::modules::buffer_enc::canonical_encoding(&name) {
            Some(name) => context.string_rooted(name),
            None => context.undefined(),
        },
    )
}

fn encoding(
    context: &mut NativeContext<'_, NodeHost>,
    root: Option<RootId>,
) -> Result<String, RootedError> {
    let Some(root) = root else {
        return Ok("utf8".to_string());
    };
    if context
        .rooted_value(root)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok("utf8".to_string());
    }
    context.string_text(root)?.ok_or_else(|| {
        type_error(
            context,
            "The \"encoding\" argument must be of type string",
            "ERR_INVALID_ARG_TYPE",
        )
        .expect_err("type_error always returns the constructed exception")
    })
}

fn number_array(
    context: &mut NativeContext<'_, NodeHost>,
    bytes: &[u8],
) -> Result<RootId, RootedError> {
    let values = bytes
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

fn type_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
    code: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(message)?;
    let key = context.string_rooted("code");
    let value = context.string_rooted(code);
    context.set_property_rooted(error, key, value, error)?;
    Ok(context.throw(error))
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn optional_get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<Option<RootId>, RootedError> {
    let value = get(context, object, name)?;
    Ok(context
        .rooted_value(value)
        .filter(|value| !value.is_undefined())
        .map(|_| value))
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
            "cannot install shared Buffer property {name}"
        )))
    }
}
