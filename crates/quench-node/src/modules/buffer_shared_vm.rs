//! Shared-VM Buffer values are realm-owned Uint8Array views.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const BUFFER_FACTORY: &str = quench_js_check::checked_js!(
    r#"(encode, decode, canonicalEncoding) => {
  const codedTypeError = (message, code) => {
    const error = new TypeError(message);
    error.code = code;
    return error;
  };
  const normalizeEncoding = (encoding) => {
    const normalized = canonicalEncoding(String(encoding));
    if (normalized === undefined) {
      throw codedTypeError(`Unknown encoding: ${encoding}`, "ERR_UNKNOWN_ENCODING");
    }
    return normalized;
  };
  const invalidByteLengthArgument = (value) => {
    let received;
    if (value === null || value === undefined) {
      received = ` Received ${value}`;
    } else if (typeof value === "function") {
      received = ` Received function ${value.name}`;
    } else if (typeof value === "object") {
      received = ` Received an instance of ${value.constructor?.name || "Object"}`;
    } else {
      received = ` Received type ${typeof value} (${String(value)})`;
    }
    return codedTypeError(
      'The "string" argument must be of type string or an instance of Buffer or ArrayBuffer.' + received,
      "ERR_INVALID_ARG_TYPE",
    );
  };
  const byteLengthGetters = [
    Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "byteLength").get,
    ...(typeof SharedArrayBuffer === "undefined"
      ? []
      : [Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, "byteLength").get]),
    Object.getOwnPropertyDescriptor(DataView.prototype, "byteLength").get,
    Object.getOwnPropertyDescriptor(
      Object.getPrototypeOf(Uint8Array.prototype),
      "byteLength",
    ).get,
  ];
  const intrinsicByteLength = (value) => {
    for (const getter of byteLengthGetters) {
      try {
        return getter.call(value);
      } catch {}
    }
    return undefined;
  };
  const makeBuffer = (bytes) => new Buffer(bytes);
  const bytesView = (input) => {
    if (input instanceof ArrayBuffer ||
        (typeof SharedArrayBuffer !== "undefined" && input instanceof SharedArrayBuffer)) {
      try { return new Uint8Array(input); } catch (_) { return new Uint8Array(0); }
    }
    if (ArrayBuffer.isView(input)) {
      try { return new Uint8Array(input.buffer, input.byteOffset, input.byteLength); }
      catch (_) { return new Uint8Array(0); }
    }
    throw codedTypeError("The argument must be a Buffer, TypedArray, DataView, or ArrayBuffer", "ERR_INVALID_ARG_TYPE");
  };
  const isAscii = (input) => {
    const bytes = bytesView(input);
    for (const byte of bytes) if (byte > 0x7F) return false;
    return true;
  };
  const isUtf8 = (input) => {
    const bytes = bytesView(input);
    for (let index = 0; index < bytes.length;) {
      const first = bytes[index++];
      if (first <= 0x7F) continue;
      let continuation;
      let secondMin = 0x80;
      let secondMax = 0xBF;
      if (first >= 0xC2 && first <= 0xDF) continuation = 1;
      else if (first >= 0xE0 && first <= 0xEF) {
        continuation = 2;
        if (first === 0xE0) secondMin = 0xA0;
        if (first === 0xED) secondMax = 0x9F;
      } else if (first >= 0xF0 && first <= 0xF4) {
        continuation = 3;
        if (first === 0xF0) secondMin = 0x90;
        if (first === 0xF4) secondMax = 0x8F;
      } else return false;
      if (index + continuation > bytes.length) return false;
      const second = bytes[index++];
      if (second < secondMin || second > secondMax) return false;
      for (let count = 1; count < continuation; count++) {
        const byte = bytes[index++];
        if (byte < 0x80 || byte > 0xBF) return false;
      }
    }
    return true;
  };

  class Buffer extends Uint8Array {
    static from(value, encoding, length) {
      if (typeof value === "string") {
        return makeBuffer(encode(value, normalizeEncoding(encoding || "utf8")));
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
        const normalized = canonicalEncoding(String(encoding || "utf8")) || "utf8";
        return encode(value, normalized).length;
      }
      const byteLength = intrinsicByteLength(value);
      if (byteLength !== undefined) return byteLength;
      throw invalidByteLengthArgument(value);
    }

    static isBuffer(value) {
      return value instanceof Buffer;
    }

    static isEncoding(encoding) {
      return typeof encoding === "string" && canonicalEncoding(encoding) !== undefined;
    }

    static compare(left, right) {
      if (!(left instanceof Uint8Array) || !(right instanceof Uint8Array)) {
        throw codedTypeError("The \"buf1\" and \"buf2\" arguments must be an instance of Buffer or Uint8Array", "ERR_INVALID_ARG_TYPE");
      }
      const length = Math.min(left.length, right.length);
      for (let index = 0; index < length; index++) {
        if (left[index] !== right[index]) return left[index] < right[index] ? -1 : 1;
      }
      return left.length === right.length ? 0 : left.length < right.length ? -1 : 1;
    }

    toString(encoding = "utf8", start = 0, end = this.length) {
      const normalized = normalizeEncoding(encoding);
      const index = (value, fallback) => {
        const number = Math.trunc(Number(value));
        if (Number.isNaN(number)) return fallback;
        if (number < 0) return 0;
        return Math.min(this.length, number);
      };
      const first = index(start, 0);
      const last = Math.max(first, index(end, 0));
      return decode(Array.from(this.subarray(first, last)), normalized);
    }

    write(value, offset, length, encoding) {
      if (typeof value !== "string") {
        throw codedTypeError('The "string" argument must be of type string', "ERR_INVALID_ARG_TYPE");
      }
      if (typeof offset === "string") {
        encoding = offset;
        offset = 0;
        length = this.length;
      } else if (typeof length === "string") {
        encoding = length;
        length = undefined;
      }
      if (offset === undefined) offset = 0;
      if (typeof offset !== "number") {
        throw codedTypeError('The "offset" argument must be of type number', "ERR_INVALID_ARG_TYPE");
      }
      if (!Number.isInteger(offset) || offset < 0 || offset > this.length) {
        const error = new RangeError(`The value of "offset" is out of range. It must be >= 0 && <= ${this.length}. Received ${offset}`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      if (length === undefined) length = this.length - offset;
      if (typeof length !== "number") {
        throw codedTypeError('The "length" argument must be of type number', "ERR_INVALID_ARG_TYPE");
      }
      if (!Number.isInteger(length) || length < 0 || length > this.length - offset) {
        const error = new RangeError(`The value of "length" is out of range. It must be >= 0 && <= ${this.length - offset}. Received ${length}`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      const normalized = normalizeEncoding(encoding === undefined ? "utf8" : encoding);
      const bytes = encode(value, normalized);
      let count = Math.min(bytes.length, length);
      if (normalized === "utf8" && count < bytes.length) {
        let lead = count;
        while (lead > 0 && (bytes[lead - 1] & 0xC0) === 0x80) lead--;
        if (lead < count) {
          const first = bytes[lead - 1];
          const width = first < 0xE0 ? 2 : first < 0xF0 ? 3 : 4;
          if (lead - 1 + width > count) count = lead - 1;
        } else if (count > 0) {
          const first = bytes[count - 1];
          const width = first < 0x80 ? 1 : first < 0xE0 ? 2 : first < 0xF0 ? 3 : 4;
          if (count - 1 + width > count) count--;
        }
      }
      if (normalized === "utf16le") count -= count % 2;
      this.set(bytes.subarray ? bytes.subarray(0, count) : bytes.slice(0, count), offset);
      return count;
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

    fill(value = 0, offset = 0, end = this.length, encoding = "utf8") {
      if (typeof offset === "string") {
        encoding = offset;
        offset = 0;
        end = this.length;
      } else if (typeof end === "string") {
        encoding = end;
        end = this.length;
      }
      offset = Math.trunc(Number(offset));
      end = Math.trunc(Number(end));
      if (Number.isNaN(offset) || Number.isNaN(end)) {
        throw codedTypeError('The "offset" and "end" arguments must be numbers', "ERR_INVALID_ARG_TYPE");
      }
      if (offset < 0 || offset > this.length || end < 0 || end > this.length) {
        const error = new RangeError("The value of \"offset\" or \"end\" is out of range");
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      if (end <= offset) return this;
      let bytes;
      if (typeof value === "string") bytes = encode(value, normalizeEncoding(encoding));
      else if (value instanceof ArrayBuffer || ArrayBuffer.isView(value)) {
        bytes = value instanceof ArrayBuffer
          ? Array.from(new Uint8Array(value))
          : Array.from(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
      } else {
        return Uint8Array.prototype.fill.call(this, value, offset, end);
      }
      if (bytes.length === 0) return Uint8Array.prototype.fill.call(this, 0, offset, end);
      for (let index = offset; index < end; index++) this[index] = bytes[(index - offset) % bytes.length];
      return this;
    }

    copy(target, targetStart = 0, sourceStart = 0, sourceEnd = this.length) {
      if (!(this instanceof Uint8Array)) {
        throw codedTypeError('The "this" argument must be an instance of Buffer or Uint8Array', "ERR_INVALID_ARG_TYPE");
      }
      if (!ArrayBuffer.isView(target) || target instanceof DataView) {
        throw codedTypeError('The "target" argument must be an instance of Buffer or Uint8Array', "ERR_INVALID_ARG_TYPE");
      }
      targetStart = Math.floor(Number(targetStart));
      sourceStart = Math.floor(Number(sourceStart));
      sourceEnd = Math.floor(Number(sourceEnd));
      if (Number.isNaN(targetStart)) targetStart = 0;
      if (Number.isNaN(sourceStart)) sourceStart = 0;
      if (Number.isNaN(sourceEnd)) sourceEnd = this.length;
      const rangeError = (name, value, message) => {
        const error = new RangeError(message || `The value of "${name}" is out of range. It must be >= 0. Received ${value}`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      };
      if (targetStart < 0) rangeError("targetStart", targetStart);
      if (sourceStart < 0 || sourceStart > this.length) rangeError("sourceStart", sourceStart);
      if (sourceEnd < 0) {
        rangeError("sourceEnd", sourceEnd, `The value of "sourceEnd" is out of range. It must be >= 0. Received ${sourceEnd}`);
      }
      sourceEnd = Math.min(sourceEnd, this.length);
      const targetBytes = new Uint8Array(target.buffer, target.byteOffset, target.byteLength);
      if (sourceEnd <= sourceStart || targetStart >= targetBytes.length) return 0;
      const length = Math.min(sourceEnd - sourceStart, targetBytes.length - targetStart);
      targetBytes.set(new Uint8Array(this.buffer, this.byteOffset + sourceStart, length), targetStart);
      return length;
    }

    compare(target, targetStart = 0, targetEnd = target?.length ?? 0, thisStart = 0, thisEnd = this.length) {
      if (!(target instanceof Uint8Array)) {
        throw codedTypeError('The "target" argument must be an instance of Buffer or Uint8Array', "ERR_INVALID_ARG_TYPE");
      }
      return Buffer.compare(this.subarray(thisStart, thisEnd), target.subarray(targetStart, targetEnd));
    }
  }

  const numericMethods = [
    ["UInt8", 1, "getUint8", "setUint8", false],
    ["Int8", 1, "getInt8", "setInt8", false],
    ["UInt16LE", 2, "getUint16", "setUint16", true],
    ["UInt16BE", 2, "getUint16", "setUint16", false],
    ["Int16LE", 2, "getInt16", "setInt16", true],
    ["Int16BE", 2, "getInt16", "setInt16", false],
    ["UInt32LE", 4, "getUint32", "setUint32", true],
    ["UInt32BE", 4, "getUint32", "setUint32", false],
    ["Int32LE", 4, "getInt32", "setInt32", true],
    ["Int32BE", 4, "getInt32", "setInt32", false],
    ["FloatLE", 4, "getFloat32", "setFloat32", true],
    ["FloatBE", 4, "getFloat32", "setFloat32", false],
    ["DoubleLE", 8, "getFloat64", "setFloat64", true],
    ["DoubleBE", 8, "getFloat64", "setFloat64", false],
  ];
  const numericRangeError = (name, value, minimum, maximum) => {
    const error = new RangeError(`The value of "${name}" is out of range. It must be >= ${minimum} and <= ${maximum}. Received ${value}`);
    error.code = "ERR_OUT_OF_RANGE";
    return error;
  };
  const checkNumericOffset = (buffer, offset, size) => {
    if (typeof offset !== "number") {
      throw codedTypeError('The "offset" argument must be of type number', "ERR_INVALID_ARG_TYPE");
    }
    if (size > buffer.length) {
      const error = new RangeError("Attempt to access memory outside buffer bounds");
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    if (Number.isNaN(offset) || (Number.isFinite(offset) && !Number.isInteger(offset))) {
      const error = new RangeError(`The value of "offset" is out of range. It must be an integer. Received ${offset}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isFinite(offset) || offset < 0 || offset > buffer.length - size) {
      throw numericRangeError("offset", offset, 0, buffer.length - size);
    }
    return offset;
  };
  for (const [suffix, size, getter, setter, littleEndian] of numericMethods) {
    const aliases = suffix.startsWith("UInt")
      ? [suffix, suffix.replace("UInt", "Uint")]
      : [suffix];
    const readMethod = function(offset = 0) {
      offset = checkNumericOffset(this, offset, size);
      return new DataView(this.buffer, this.byteOffset, this.byteLength)[getter](offset, littleEndian);
    };
    const writeMethod = function(value, offset = 0) {
      offset = checkNumericOffset(this, offset, size);
      if (typeof value !== "number") {
        throw codedTypeError('The "value" argument must be of type number', "ERR_INVALID_ARG_TYPE");
      }
      if (getter.includes("Uint")) {
        const maximum = size === 4 ? 0xFFFFFFFF : (2 ** (size * 8)) - 1;
        if (value < 0 || value > maximum || !Number.isInteger(value)) {
          throw numericRangeError("value", value, 0, maximum);
        }
      } else if (getter.includes("Int")) {
        const maximum = (2 ** (size * 8 - 1)) - 1;
        const minimum = -(2 ** (size * 8 - 1));
        if (value < minimum || value > maximum || !Number.isInteger(value)) {
          throw numericRangeError("value", value, minimum, maximum);
        }
      }
      new DataView(this.buffer, this.byteOffset, this.byteLength)[setter](offset, value, littleEndian);
      return offset + size;
    };
    for (const alias of aliases) {
      Buffer.prototype[`read${alias}`] = readMethod;
      Buffer.prototype[`write${alias}`] = writeMethod;
    }
  }
  const variableIntegerLength = (byteLength) => {
    if (typeof byteLength !== "number") {
      throw codedTypeError('The "byteLength" argument must be of type number', "ERR_INVALID_ARG_TYPE");
    }
    if (Number.isNaN(byteLength) || (Number.isFinite(byteLength) && !Number.isInteger(byteLength))) {
      const error = new RangeError(`The value of "byteLength" is out of range. It must be an integer. Received ${byteLength}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isFinite(byteLength) || byteLength < 1 || byteLength > 6) {
      throw numericRangeError("byteLength", byteLength, 1, 6);
    }
    return byteLength;
  };
  const readVariableInteger = (buffer, offset, byteLength, littleEndian, signed) => {
    byteLength = variableIntegerLength(byteLength);
    offset = checkNumericOffset(buffer, offset, byteLength);
    let value = 0;
    for (let index = 0; index < byteLength; index++) {
      const source = littleEndian ? index : byteLength - index - 1;
      value += buffer[offset + source] * (2 ** (8 * index));
    }
    if (signed && value >= 2 ** (8 * byteLength - 1)) value -= 2 ** (8 * byteLength);
    return value;
  };
  const writeVariableInteger = (buffer, value, offset, byteLength, littleEndian, signed) => {
    byteLength = variableIntegerLength(byteLength);
    offset = checkNumericOffset(buffer, offset, byteLength);
    if (typeof value !== "number") {
      throw codedTypeError('The "value" argument must be of type number', "ERR_INVALID_ARG_TYPE");
    }
    const bits = 8 * byteLength;
    const minimum = signed ? -(2 ** (bits - 1)) : 0;
    const maximum = signed ? (2 ** (bits - 1)) - 1 : (2 ** bits) - 1;
    if (!Number.isInteger(value) || value < minimum || value > maximum) {
      const format = (number) => String(number).replace(/(\d)(?=(\d{3})+(?!\d))/g, "$1_");
      const range = signed
        ? byteLength > 4
          ? `>= -(2 ** ${bits - 1}) and < 2 ** ${bits - 1}`
          : `>= ${minimum} and <= ${maximum}`
        : byteLength > 4
          ? `>= 0 and < 2 ** ${bits}`
          : `>= 0 and <= ${maximum}`;
      const received = byteLength > 4 ? format(value) : String(value);
      const error = new RangeError(`The value of "value" is out of range. It must be ${range}. Received ${received}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    let remaining = value < 0 ? value + 2 ** bits : value;
    for (let index = 0; index < byteLength; index++) {
      const target = littleEndian ? index : byteLength - index - 1;
      buffer[offset + target] = remaining & 0xFF;
      remaining = Math.floor(remaining / 256);
    }
    return offset + byteLength;
  };
  for (const [suffix, littleEndian, signed] of [
    ["UIntLE", true, false], ["UIntBE", false, false],
    ["IntLE", true, true], ["IntBE", false, true],
  ]) {
    const aliases = suffix.startsWith("UInt") ? [suffix, suffix.replace("UInt", "Uint")] : [suffix];
    const readMethod = function(offset, byteLength) {
      return readVariableInteger(this, offset, byteLength, littleEndian, signed);
    };
    const writeMethod = function(value, offset, byteLength) {
      return writeVariableInteger(this, value, offset, byteLength, littleEndian, signed);
    };
    for (const alias of aliases) {
      Buffer.prototype[`read${alias}`] = readMethod;
      Buffer.prototype[`write${alias}`] = writeMethod;
    }
  }
  const legacyWrite = (encoding) => function(value, offset, length) {
    offset = offset === undefined ? 0 : offset;
    length = length === undefined ? this.length - offset : length;
    if (typeof offset !== "number" || typeof length !== "number" ||
        offset < 0 || length < 0 || offset + length > this.length) {
      const error = new RangeError("Attempt to access memory outside buffer bounds");
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    return this.write(value, offset, length, encoding);
  };
  Buffer.prototype.asciiWrite = legacyWrite("ascii");
  Buffer.prototype.latin1Write = legacyWrite("latin1");
  Buffer.prototype.utf8Write = legacyWrite("utf8");

  // Node exposes Buffer's static API as enumerable own properties. Packages
  // such as safer-buffer derive their constructor view with `for...in`.
  for (const name of Object.getOwnPropertyNames(Buffer)) {
    if (name === "length" || name === "name" || name === "prototype") continue;
    Object.defineProperty(Buffer, name, { enumerable: true });
  }

  return { Buffer, SlowBuffer: Buffer, isAscii, isUtf8 };
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
    for name in ["isAscii", "isUtf8"] {
        let value = get(context, constructor, name)?;
        set(context, module, name, value)?;
    }
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

pub(crate) fn install_blob_export(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let blob = get(context, global, "Blob")?;
    set(context, module, "Blob", blob)
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
    let Some(encoding) = crate::modules::buffer_codec::canonical_encoding(&encoding_name) else {
        return Err(type_error(
            context,
            &format!("Unknown encoding: {encoding_name}"),
            "ERR_UNKNOWN_ENCODING",
        )?);
    };
    let bytes = crate::modules::buffer_codec::encode_str(&input, encoding);
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
    let Some(canonical) = crate::modules::buffer_codec::canonical_encoding(&encoding) else {
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
    let units = crate::modules::buffer_codec::decode_units(&bytes, canonical);
    Ok(context.string_units_rooted(&units))
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
        match crate::modules::buffer_codec::canonical_encoding(&name) {
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
