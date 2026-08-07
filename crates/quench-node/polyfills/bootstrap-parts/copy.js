const __nodeBufferCopyNumber = (value) => {
  const result = Math.trunc(Number(value));
  return Number.isNaN(result) ? 0 : result;
};
const __nodeBufferCopyTarget = (target) =>
  target instanceof Uint8Array
    ? target
    : new Uint8Array(target.buffer, target.byteOffset, target.byteLength);
const __nodeBufferCopy = (
  source,
  target,
  targetStart,
  sourceStart,
  sourceEnd,
) => {
  if (targetStart >= target.length || sourceStart >= sourceEnd) return 0;
  const end = Math.min(sourceEnd, source.length);
  const count = Math.min(end - sourceStart, target.length - targetStart);
  const bytes = new Uint8Array(count);
  bytes.set(source.subarray(sourceStart, sourceStart + count));
  target.set(bytes, targetStart);
  return count;
};
const __nodeBufferCopyRangeError = (name, rule, value) => {
  const error = new RangeError(
    `The value of "${name}" is out of range. It must be ${rule}. Received ${value}`,
  );
  error.code = "ERR_OUT_OF_RANGE";
  return error;
};
const __nodeBufferCopyValidate = (source, target) => {
  if (!(source instanceof Uint8Array)) {
    const error = new TypeError(
      "Method Buffer.prototype.copy called on incompatible receiver",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!ArrayBuffer.isView(target)) {
    const error = new TypeError(
      'The "target" argument must be an instance of Uint8Array',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeBufferConcatReceived = (value) => {
  if (value == null) return ` Received ${value}`;
  if (typeof value === "number") return ` Received type number (${value})`;
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (ArrayBuffer.isView(value)) return " Received an instance of Buffer";
  const name = value.constructor?.name === "NodeBuffer"
    ? "Buffer"
    : value.constructor?.name;
  return ` Received an instance of ${name || "Object"}`;
};
const __nodeBufferConcatValidate = (list, totalLength) => {
  if (!Array.isArray(list)) {
    const error = new TypeError(
      `The "list" argument must be an instance of Array.${
        __nodeBufferConcatReceived(list)
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const invalidIndex = list.findIndex((item) => !ArrayBuffer.isView(item));
  if (invalidIndex >= 0) {
    const item = list[invalidIndex];
    const error = new TypeError(
      `The "list[${invalidIndex}]" argument must be an instance of Buffer or Uint8Array.${
        __nodeBufferConcatReceived(item)
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    totalLength !== undefined &&
    (typeof totalLength !== "number" ||
      !Number.isInteger(totalLength) ||
      totalLength < 0)
  ) {
    const message = Number.isInteger(totalLength)
      ? `The value of "length" is out of range. It must be >= 0 && <= 9007199254740991. Received ${totalLength}`
      : `The value of "length" is out of range. It must be an integer. Received ${totalLength}`;
    const error = new RangeError(message);
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeBufferConcatLength = (list, totalLength) => {
  if (totalLength !== undefined) return totalLength;
  return list.reduce((sum, item) => sum + item.byteLength, 0);
};
const __nodeBufferConcatCopy = (output, item, offset) => {
  const bytes = new Uint8Array(item.buffer, item.byteOffset, item.byteLength);
  const count = Math.min(bytes.length, output.length - offset);
  if (count > 0) output.set(bytes.subarray(0, count), offset);
  return offset + count;
};
const __nodeBufferFillRangeInvalid = (start, end, length) =>
  start < 0 || end < 0 || start > length || end > length;
NodeBuffer.prototype.includes = function (value, byteOffset, encoding) {
  return this.indexOf(value, byteOffset, encoding) !== -1;
};
const __nodeBufferFillRange = (length, start, end, encoding) => {
  if (end === undefined) end = length;
  if (typeof start === "string") {
    encoding = start;
    start = 0;
    end = length;
  } else if (typeof end === "string") {
    encoding = end;
    end = length;
  }
  if (typeof start !== "number" || typeof end !== "number") {
    const name = typeof start !== "number" ? "start" : "end";
    const value = name === "start" ? start : end;
    const received = value !== null && typeof value === "object"
      ? `an instance of ${value.constructor?.name || "Object"}`
      : `type ${typeof value}`;
    const error = new TypeError(
      `The "${name}" argument must be of type number. Received ${received}`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const toIndex = (input, fallback) => {
    const number = Math.trunc(Number(input));
    return Number.isNaN(number) ? fallback : number;
  };
  start = toIndex(start, 0);
  end = toIndex(end, length);
  if (__nodeBufferFillRangeInvalid(start, end, length)) {
    const error = new RangeError("The value is out of range");
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return { start, end, encoding };
};
const __nodeBufferFillString = (value, encoding) => {
  if (encoding !== undefined && typeof encoding !== "string") {
    const error = new TypeError(
      `The "encoding" argument must be of type string. Received type ${typeof encoding} (${
        String(encoding)
      })`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    String(encoding).toLowerCase() === "hex" &&
    (value.length % 2 || !/^[0-9a-f]*$/i.test(value))
  ) {
    const error = new TypeError('The "value" argument is invalid');
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  return NodeBuffer.from(value, encoding);
};
const __nodeBufferFillPattern = (value, encoding) => {
  if (value === null || value === undefined || typeof value === "number") {
    return new NodeBuffer([Number(value) || 0]);
  }
  if (typeof value === "string") return __nodeBufferFillString(value, encoding);
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  return NodeBuffer.from(String(value));
};
const __nodeBufferCompareValidate = (target, values) => {
  if (!(target instanceof Uint8Array)) {
    const error = new TypeError(
      `The "target" argument must be an instance of Buffer or Uint8Array.${
        __nodeBufferConcatReceived(target)
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  for (const value of values) {
    if (value !== undefined && typeof value !== "number") {
      const error = new TypeError("offset arguments must be numbers");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (value !== undefined && (!Number.isFinite(value) || value < 0)) {
      const error = new RangeError("offset is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  }
};
const __nodeBufferCompareRange = (target, source, values) => {
  const [targetStart, targetEnd, sourceStart, sourceEnd] = values;
  const ranges = [
    targetStart === undefined ? 0 : Math.trunc(targetStart),
    targetEnd === undefined ? target.length : Math.trunc(targetEnd),
    sourceStart === undefined ? 0 : Math.trunc(sourceStart),
    sourceEnd === undefined ? source.length : Math.trunc(sourceEnd),
  ];
  if (ranges[1] > target.length || ranges[3] > source.length) {
    const error = new RangeError("offset is out of range");
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return [
    target.subarray(ranges[0], ranges[1]),
    source.subarray(ranges[2], ranges[3]),
  ];
};
const __nodeBufferStringRange = (length, start, end) => {
  const normalize = (value, fallback) => {
    const number = Math.trunc(Number(value));
    return Number.isNaN(number)
      ? value === undefined ? fallback : 0
      : Math.max(0, Math.min(length, number));
  };
  return { start: normalize(start, 0), end: normalize(end, length) };
};
const __nodeBufferBase64String = (buffer, url) => {
  const alphabet =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let result = "";
  for (let index = 0; index < buffer.length; index += 3) {
    const n = (buffer[index] << 16) |
      ((buffer[index + 1] || 0) << 8) |
      (buffer[index + 2] || 0);
    result += alphabet[(n >> 18) & 63] +
      alphabet[(n >> 12) & 63] +
      (index + 1 < buffer.length ? alphabet[(n >> 6) & 63] : "=") +
      (index + 2 < buffer.length ? alphabet[n & 63] : "=");
  }
  return url
    ? result.replace(/=+$/, "").replace(/\+/g, "-").replace(/\//g, "_")
    : result;
};
const __nodeBufferEncodedString = (buffer, encoding) => {
  if (encoding === "hex") {
    const digits = "0123456789abcdef";
    let result = "";
    for (const byte of buffer) {
      const value = Number(byte);
      result += digits[(value >> 4) & 15] + digits[value & 15];
    }
    return result;
  }
  if (encoding === "base64" || encoding === "base64url") {
    return __nodeBufferBase64String(buffer, encoding === "base64url");
  }
  if (encoding === "latin1" || encoding === "binary") {
    return Array.from(buffer, (byte) => String.fromCharCode(byte)).join("");
  }
  if (encoding === "ascii") {
    return Array.from(buffer, (byte) => String.fromCharCode(byte & 0x7f)).join(
      "",
    );
  }
  if (["utf16le", "utf-16le", "ucs2", "ucs-2"].includes(encoding)) {
    let result = "";
    for (let index = 0; index + 1 < buffer.length; index += 2) {
      result += String.fromCharCode(buffer[index] | (buffer[index + 1] << 8));
    }
    return result;
  }
  return new NodeTextDecoder().decode(buffer);
};
NodeBuffer = class NodeBuffer extends __NodeBufferBase01 {
  copy(target, targetStart = 0, sourceStart = 0, sourceEnd = this.length) {
    __nodeBufferCopyValidate(this, target);
    if (__nodeImmutableBuffers.has(target.buffer)) return 0;
    targetStart = __nodeBufferCopyNumber(targetStart);
    sourceStart = __nodeBufferCopyNumber(sourceStart);
    sourceEnd = __nodeBufferCopyNumber(sourceEnd);
    if (targetStart < 0) {
      throw __nodeBufferCopyRangeError("targetStart", ">= 0", targetStart);
    }
    if (sourceStart < 0 || sourceStart > this.length) {
      throw __nodeBufferCopyRangeError(
        "sourceStart",
        `>= 0 && <= ${this.length}`,
        sourceStart,
      );
    }
    if (sourceEnd < 0) {
      throw __nodeBufferCopyRangeError("sourceEnd", ">= 0", sourceEnd);
    }
    return __nodeBufferCopy(
      this,
      __nodeBufferCopyTarget(target),
      targetStart,
      sourceStart,
      sourceEnd,
    );
  }
  static concat(list, totalLength) {
    __nodeBufferConcatValidate(list, totalLength);
    const length = list.length === 0
      ? (totalLength ?? 0)
      : __nodeBufferConcatLength(list, totalLength);
    const output = new NodeBuffer(length);
    let offset = 0;
    list.forEach(
      (item) => (offset = __nodeBufferConcatCopy(output, item, offset)),
    );
    Object.setPrototypeOf(output, globalThis.Buffer.prototype);
    return output;
  }

  fill(value = 0, start = 0, end, encoding = "utf8") {
    const length = this.byteLength;
    if (
      Object.prototype.hasOwnProperty.call(this, "length") &&
      this.length !== length
    ) {
      const error = new RangeError(
        "Attempt to access memory outside buffer bounds",
      );
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    ({ start, end, encoding } = __nodeBufferFillRange(
      length,
      start,
      end,
      encoding,
    ));
    const pattern = __nodeBufferFillPattern(value, encoding);
    if (pattern.length === 0) {
      if (ArrayBuffer.isView(value)) {
        const error = new TypeError('The "value" argument is invalid');
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      return this;
    }
    for (let i = start; i < end; i++) {
      this[i] = pattern[(i - start) % pattern.length];
    }
    return this;
  }

  toString(encoding = "utf8", start = 0, end = this.length) {
    encoding = String(encoding).toLowerCase();
    if (!NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    if (start !== 0 || end !== this.length) {
      const range = __nodeBufferStringRange(this.length, start, end);
      const first = range.start;
      const last = range.end;
      if (last <= first) return "";
      return this.subarray(first, Math.max(first, last)).toString(encoding);
    }
    return __nodeBufferEncodedString(this, encoding);
  }

  equals(other) {
    if (!(other instanceof Uint8Array)) {
      const error = new TypeError(
        `The "otherBuffer" argument must be an instance of Buffer or Uint8Array.${
          __nodeBufferConcatReceived(other)
        }`,
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return (
      this.length === other.length &&
      this.every((value, index) => value === other[index])
    );
  }

  compare(target, targetStart, targetEnd, sourceStart, sourceEnd) {
    const values = [targetStart, targetEnd, sourceStart, sourceEnd];
    __nodeBufferCompareValidate(target, values);
    const [left, right] = __nodeBufferCompareRange(target, this, values);
    return NodeBuffer.compare(right, left);
  }

  _swap(width) {
    if (this.length % width !== 0) {
      throw new Error(`Buffer size must be a multiple of ${width * 8}-bits`);
    }
    for (let offset = 0; offset < this.length; offset += width) {
      for (let i = 0; i < width / 2; i++) {
        const value = this[offset + i];
        this[offset + i] = this[offset + width - i - 1];
        this[offset + width - i - 1] = value;
      }
    }
    return this;
  }

  swap16() {
    return NodeBuffer.prototype._swap.call(this, 2);
  }

  swap32() {
    return NodeBuffer.prototype._swap.call(this, 4);
  }

  swap64() {
    return NodeBuffer.prototype._swap.call(this, 8);
  }

  _readBigInt(offset, littleEndian, signed) {
    NodeBuffer.prototype._integerOffset.call(this, offset, 8);
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return signed
      ? view.getBigInt64(offset, littleEndian)
      : view.getBigUint64(offset, littleEndian);
  }

  _writeBigInt(value, offset, littleEndian, signed) {
    NodeBuffer.prototype._integerOffset.call(this, offset, 8);
    if (typeof value !== "bigint") {
      throw new TypeError('The "value" argument must be a bigint');
    }
    const min = signed ? -(1n << 63n) : 0n;
    const max = signed ? (1n << 63n) - 1n : (1n << 64n) - 1n;
    if (value < min || value > max) {
      const error = new RangeError('The value of "value" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    if (signed) view.setBigInt64(offset, value, littleEndian);
    else view.setBigUint64(offset, value, littleEndian);
    return offset + 8;
  }

  readBigInt64LE(offset = 0) {
    return NodeBuffer.prototype._readBigInt.call(this, offset, true, true);
  }

  readBigInt64BE(offset = 0) {
    return NodeBuffer.prototype._readBigInt.call(this, offset, false, true);
  }

  readBigUInt64LE(offset = 0) {
    return NodeBuffer.prototype._readBigInt.call(this, offset, true, false);
  }

  readBigUInt64BE(offset = 0) {
    return NodeBuffer.prototype._readBigInt.call(this, offset, false, false);
  }

  writeBigInt64LE(value, offset = 0) {
    return NodeBuffer.prototype._writeBigInt.call(
      this,
      value,
      offset,
      true,
      true,
    );
  }

  writeBigInt64BE(value, offset = 0) {
    return NodeBuffer.prototype._writeBigInt.call(
      this,
      value,
      offset,
      false,
      true,
    );
  }

  writeBigUInt64LE(value, offset = 0) {
    return NodeBuffer.prototype._writeBigInt.call(
      this,
      value,
      offset,
      true,
      false,
    );
  }

  writeBigUInt64BE(value, offset = 0) {
    return NodeBuffer.prototype._writeBigInt.call(
      this,
      value,
      offset,
      false,
      false,
    );
  }

  toJSON() {
    return { type: "Buffer", data: Array.from(this) };
  }

  inspect() {
    const limit = Math.min(this.length, __nodeInspectMaxBytes);
    const bytes = Array.from(
      this.subarray(0, limit),
      (byte) => byte.toString(16).padStart(2, "0"),
    );
    const label = this instanceof NodeBuffer ? "Buffer" : "Uint8Array";
    return `<${label} ${bytes.join(" ")}${
      limit < this.length
        ? ` ... ${this.length - limit} more byte${
          this.length - limit === 1 ? "" : "s"
        }`
        : ""
    }>`;
  }

  toLocaleString(...args) {
    return this.toString(...args);
  }
};
const __nodeBufferFromBase = __NodeBufferBase01.from;
NodeBuffer.from = (...args) => {
  const source = __nodeBufferFromBase.apply(__NodeBufferBase01, args);
  if (
    __nodeBufferIsArrayBuffer(args[0]) ||
    args[0] instanceof SharedArrayBuffer
  ) {
    return source;
  }
  const pooled = __nodeBufferPoolFrom(source);
  if (pooled) {
    Object.setPrototypeOf(pooled, NodeBuffer.prototype);
    return pooled;
  }
  const output = new NodeBuffer(source.length);
  output.set(source);
  return output;
};
