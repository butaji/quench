const __nodeBufferStringByteLength = (value, encoding) => {
  const normalized = String(encoding || "utf8").toLowerCase();
  if (["ascii", "latin1", "binary"].includes(normalized)) return value.length;
  if (["ucs2", "ucs-2", "utf16le", "utf-16le"].includes(normalized)) {
    return value.length * 2;
  }
  if (normalized === "hex") return Math.floor(value.length / 2);
  if (normalized === "base64" || normalized === "base64url") {
    return NodeBuffer.from(value, normalized).length;
  }
  return new NodeTextEncoder().encode(value).length;
};
const __nodeBufferByteLength = (value, encoding) => {
  if (typeof value === "string") {
    return __nodeBufferStringByteLength(value, encoding);
  }
  if (
    __nodeBufferIsArrayBuffer(value) ||
    value instanceof SharedArrayBuffer ||
    ArrayBuffer.isView(value)
  ) {
    return value.byteLength;
  }
  const error = new TypeError(
    `The "string" argument must be of type string or an instance of Buffer or ArrayBuffer.${__nodeBufferFromReceived(
      value
    )}`
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
const __nodeUtf8ByteInfo = (byte) => {
  if (byte <= 0x7f) return [0, 0];
  if (byte >= 0xc2 && byte <= 0xdf) return [1, byte & 0x1f];
  if (byte >= 0xe0 && byte <= 0xef) return [2, byte & 0x0f];
  if (byte >= 0xf0 && byte <= 0xf4) return [3, byte & 0x07];
  return undefined;
};
const __nodeUtf8ContinuationByte = (byte, code) =>
  byte >= 0x80 && byte <= 0xbf ? (code << 6) | (byte & 0x3f) : undefined;
const __nodeUtf8Continuation = (value, index, needed, code) => {
  for (let offset = 1; offset <= needed; offset++) {
    code = __nodeUtf8ContinuationByte(value[index + offset], code);
    if (code === undefined) return undefined;
  }
  return code;
};
const __nodeUtf8CodeValid = (needed, code) =>
  !(
    (needed === 2 && code < 0x800) ||
    (needed === 3 && code < 0x10000) ||
    code > 0x10ffff ||
    (code >= 0xd800 && code <= 0xdfff)
  );
const __nodeIsValidUtf8 = (value) => {
  for (let index = 0; index < value.length; index++) {
    const info = __nodeUtf8ByteInfo(value[index]);
    if (!info) return false;
    const [needed, initialCode] = info;
    if (!needed) continue;
    const code = __nodeUtf8Continuation(value, index, needed, initialCode);
    if (code === undefined) return false;
    if (!__nodeUtf8CodeValid(needed, code)) return false;
    index += needed;
  }
  return true;
};
const __nodeValidateUtf8Input = (value) => {
  if (value instanceof ArrayBuffer && __nodeDetachedBuffers.has(value)) {
    throw Object.assign(new TypeError("ArrayBuffer is detached"), { code: "ERR_INVALID_STATE" });
  }
  if (value instanceof Uint8Array) return value;
  throw Object.assign(new TypeError('The "input" argument must be an instance of Uint8Array'), { code: "ERR_INVALID_ARG_TYPE" });
};
const __nodeCopyBytesValidateView = (view, offset, length) => {
  if (!ArrayBuffer.isView(view)) {
    throw Object.assign(new TypeError('The "view" argument must be an instance of TypedArray'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    typeof offset !== "number" ||
    (length !== undefined && typeof length !== "number")
  ) {
    throw Object.assign(new TypeError("offset and length must be numbers"), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
const __nodeCopyBytesRangeInvalid = (offset, length) =>
  offset === Infinity ||
  offset === -Infinity ||
  !Number.isFinite(length) ||
  !Number.isInteger(offset) ||
  !Number.isInteger(length) ||
  offset < 0 ||
  length < 0;
const __nodeCopyBytesRange = (view, offset, length) => {
  const elementSize = view.BYTES_PER_ELEMENT || 1;
  const elementLength =
    view.length === undefined ? view.byteLength : view.length;
  if (length === undefined) {
    length = offset >= elementLength ? 0 : elementLength - offset;
  }
  if (__nodeCopyBytesRangeInvalid(offset, length)) {
    throw Object.assign(new RangeError("The requested range is outside the bounds of the view"), { code: "ERR_OUT_OF_RANGE" });
  }
  offset = Math.min(Math.trunc(offset), elementLength);
  length = Math.min(Math.trunc(length), elementLength - offset);
  return { byteLength: length * elementSize, byteOffset: offset * elementSize };
};
const __nodeCopyBytesOutput = (view, range) => {
  const bytes = new Uint8Array(
    view.buffer,
    view.byteOffset + range.byteOffset,
    range.byteLength
  );
  const output = new NodeBuffer(range.byteLength);
  output.set(bytes);
  const alignment = view.BYTES_PER_ELEMENT || 1;
  if (alignment > 1 && output.byteOffset % alignment !== 0) {
    const aligned = new Uint8Array(new ArrayBuffer(range.byteLength));
    aligned.set(output);
    Object.setPrototypeOf(aligned, NodeBuffer.prototype);
    return aligned;
  }
  return output;
};
const __nodeBufferInvalidOffset = (offset, limit) =>
  offset < 0 || offset > limit;
const __nodeBufferInvalidLength = (size, offset, limit) =>
  size < 0 || offset + size > limit;
const __nodeBufferArrayBufferRange = (value, encoding, length) => {
  let offset = Number(encoding);
  if (!Number.isFinite(offset)) offset = Number.isNaN(offset) ? 0 : offset;
  offset = Math.trunc(offset);
  if (__nodeBufferInvalidOffset(offset, value.byteLength)) {
    throw Object.assign(new RangeError('"offset" is outside of buffer bounds'), { code: "ERR_BUFFER_OUT_OF_BOUNDS" });
  }
  let size = value.byteLength - offset;
  if (length !== undefined) {
    const numericLength = Number(length);
    if (numericLength === Infinity || numericLength === -Infinity) {
      throw Object.assign(new RangeError('"length" is outside of buffer bounds'), { code: "ERR_BUFFER_OUT_OF_BOUNDS" });
    }
    size = Number.isNaN(numericLength) ? 0 : Math.trunc(numericLength);
  }
  if (__nodeBufferInvalidLength(size, offset, value.byteLength)) {
    throw Object.assign(new RangeError('"length" is outside of buffer bounds'), { code: "ERR_BUFFER_OUT_OF_BOUNDS" });
  }
  return { offset, size };
};
const __nodeBufferFromArrayBuffer = (value, encoding, length) => {
  const range = __nodeBufferArrayBufferRange(value, encoding, length);
  return length === undefined
    ? new NodeBuffer(value, range.offset)
    : new NodeBuffer(value, range.offset, range.size);
};
const __nodeBufferFromHex = (value) => {
  const output = new NodeBuffer(Math.floor(value.length / 2));
  let written = 0;
  for (
    let index = 0;
    index + 1 < value.length && written < output.length;
    index += 2
  ) {
    if (!/^[0-9a-f]{2}$/i.test(value.slice(index, index + 2))) break;
    output[written++] = parseInt(value.slice(index, index + 2), 16);
  }
  return output.subarray(0, written);
};
const __nodeBufferFromBase64 = (value) => {
  if (/^\s*=/.test(value)) return new NodeBuffer(0);
  const alphabet =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const clean = value
    .replace(/\s+/g, "")
    .replace(/[^A-Za-z0-9+/_-]/g, "")
    .replace(/=+$/, "")
    .replace(/-/g, "+")
    .replace(/_/g, "/");
  const output = new NodeBuffer(Math.floor((clean.length * 6) / 8));
  let buffer = 0;
  let bits = 0;
  let index = 0;
  for (const char of clean) {
    buffer = (buffer << 6) | alphabet.indexOf(char);
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      output[index++] = (buffer >> bits) & 255;
    }
  }
  return output;
};
const __nodeBufferFromText = (value, encoding) => {
  if (encoding === "hex") return __nodeBufferFromHex(value);
  if (encoding === "base64" || encoding === "base64url") {
    return __nodeBufferFromBase64(value);
  }
  if (["ascii", "latin1", "binary"].includes(encoding)) {
    const output = new NodeBuffer(value.length);
    for (let index = 0; index < value.length; index++) {
      output[index] = value.charCodeAt(index) & 0xff;
    }
    return output;
  }
  if (["ucs2", "ucs-2", "utf16le", "utf-16le"].includes(encoding)) {
    const output = new NodeBuffer(value.length * 2);
    for (let index = 0; index < value.length; index++) {
      const code = value.charCodeAt(index);
      output[index * 2] = code & 0xff;
      output[index * 2 + 1] = code >> 8;
    }
    return output;
  }
  const output = new Uint8Array(new NodeTextEncoder().encode(value));
  Object.setPrototypeOf(output, NodeBuffer.prototype);
  return output;
};
const __nodeBufferFromTypedArray = (value) => {
  if (value.length === undefined) return new NodeBuffer(value);
  const output = new NodeBuffer(value.length);
  for (let index = 0; index < value.length; index++) {
    output[index] = Number(value[index]) & 0xff;
  }
  return output;
};
const __nodeBufferFromArrayLike = (value) => {
  if (!value || typeof value !== "object" || !("length" in value)) {
    return undefined;
  }
  const length = Math.max(0, Math.trunc(Number(value.length)) || 0);
  const output = new NodeBuffer(length);
  for (let index = 0; index < length; index++) {
    output[index] = Number(value[index]) || 0;
  }
  return output;
};
const __nodeBufferFromObject = (value) => {
  if (value && value.type === "Buffer" && Array.isArray(value.data)) {
    return new NodeBuffer(value.data);
  }
  if (
    value &&
    !ArrayBuffer.isView(value) &&
    (value.buffer instanceof ArrayBuffer ||
      value.buffer instanceof SharedArrayBuffer)
  ) {
    return NodeBuffer.from(value.buffer);
  }
  if (ArrayBuffer.isView(value)) return __nodeBufferFromTypedArray(value);
  if (Array.isArray(value)) return new NodeBuffer(value);
  return __nodeBufferFromArrayLike(value);
};
const __nodeBufferValidateEncoding = (value, encoding) => {
  if (typeof encoding === "string") encoding = encoding.toLowerCase();
  if (
    typeof value === "string" &&
    typeof encoding === "string" &&
    !NodeBuffer.isEncoding(encoding)
  ) {
    throw Object.assign(new TypeError(`Unknown encoding: ${encoding}`), { code: "ERR_UNKNOWN_ENCODING" });
  }
  return encoding;
};
const __nodeBufferFromPrimitive = (value, encoding) => {
  if (
    value instanceof String ||
    (typeof value === "object" &&
      Object.prototype.toString.call(value) === "[object String]")
  ) {
    let text = "";
    for (let index = 0; index < value.length; index++) text += value[index];
    return NodeBuffer.from(text, encoding);
  }
  if (
    value &&
    typeof value === "object" &&
    typeof value[Symbol.toPrimitive] === "function"
  ) {
    const primitive = value[Symbol.toPrimitive]("string");
    if (typeof primitive === "string") {
      return NodeBuffer.from(primitive, encoding);
    }
  }
  return undefined;
};
class NodeBuffer extends Uint8Array {
  get parent() {
    if (this === NodeBuffer.prototype) return undefined;
    return this.buffer;
  }
  get offset() {
    if (this === NodeBuffer.prototype) return undefined;
    return this.byteOffset;
  }
  static from(value, encoding, length) {
    encoding = __nodeBufferValidateEncoding(value, encoding);
    if (
      __nodeBufferIsArrayBuffer(value) ||
      value instanceof SharedArrayBuffer
    ) {
      return __nodeBufferFromArrayBuffer(value, encoding, length);
    }
    const primitive = __nodeBufferFromPrimitive(value, encoding);
    if (primitive) return primitive;
    if (typeof value === "string") return __nodeBufferFromText(value, encoding);
    const output = __nodeBufferFromObject(value);
    if (output) return output;
    const error = new TypeError(
      `The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object.${__nodeBufferFromReceived(
        value
      )}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
}
NodeBuffer.prototype.toString = function toString(
  encoding = "utf8",
  start = 0,
  end = this.length
) {
  const first = Math.max(0, Math.trunc(Number(start)) || 0);
  const last = Math.min(this.length, Math.trunc(Number(end)) || 0);
  return __nodeBufferEncodedString(
    this.subarray(first, last),
    String(encoding).toLowerCase()
  );
};
NodeBuffer.concat = (list, totalLength) => {
  if (!Array.isArray(list)) {
    throw new TypeError("The list argument must be an Array");
  }
  if (
    totalLength !== undefined &&
    (!Number.isInteger(totalLength) || totalLength < 0)
  ) {
    throw new RangeError("The value of length is out of range");
  }
  const length =
    totalLength ?? list.reduce((sum, item) => sum + item.byteLength, 0);
  const output = new NodeBuffer(length);
  let offset = 0;
  for (const item of list) {
    if (!ArrayBuffer.isView(item)) {
      throw new TypeError("list items must be buffers");
    }
    const count = Math.min(item.byteLength, length - offset);
    output.set(new Uint8Array(item.buffer, item.byteOffset, count), offset);
    offset += count;
    if (offset === length) break;
  }
  return output;
};
NodeBuffer._validateSize = (size) => {
  if (typeof size !== "number") {
    throw Object.assign(new TypeError('The "size" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isFinite(size) || size < 0 || size > 0x7fffffff) {
    throw Object.assign(new RangeError('The value of "size" is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
  return Math.trunc(size);
};
NodeBuffer.alloc = (size, fill = 0, encoding) => {
  const length = NodeBuffer._validateSize(size);
  __nodeAllocatorCounts.zeroFilled++;
  return new NodeBuffer(length).fill(fill, 0, length, encoding);
};
NodeBuffer.allocUnsafe = (size) => {
  __nodeAllocatorCounts.uninitialized++;
  return new NodeBuffer(NodeBuffer._validateSize(size));
};
NodeBuffer.allocUnsafeSlow = NodeBuffer.allocUnsafe;
NodeBuffer.of = (...values) => new NodeBuffer(values);
NodeBuffer.copyBytesFrom = (view, offset = 0, length) => {
  __nodeCopyBytesValidateView(view, offset, length);
  return __nodeCopyBytesOutput(
    view,
    __nodeCopyBytesRange(view, offset, length)
  );
};
NodeBuffer.isBuffer = (value) => value instanceof NodeBuffer;
NodeBuffer.isEncoding = (encoding) => {
  if (typeof encoding !== "string") return false;
  return "hex utf8 utf-8 ascii latin1 binary base64 base64url ucs2 ucs-2 utf16le utf-16le"
    .split(" ")
    .includes(encoding.toLowerCase());
};
NodeBuffer.isUtf8 = (value) =>
  __nodeIsValidUtf8(__nodeValidateUtf8Input(value));
NodeBuffer.byteLength = (value, encoding = "utf8") =>
  __nodeBufferByteLength(value, encoding);
NodeBuffer.prototype.subarray = function subarray(
  begin = 0,
  end = this.length
) {
  const index = (value, fallback) => {
    const number = Math.trunc(Number(value));
    if (Number.isNaN(number)) return fallback;
    return Math.max(
      0,
      Math.min(this.length, number < 0 ? this.length + number : number)
    );
  };
  const start = index(begin, 0);
  const finish = Math.max(start, index(end, this.length));
  return new NodeBuffer(this.buffer, this.byteOffset + start, finish - start);
};
NodeBuffer.prototype.slice = function slice(start = 0, end = this.length) {
  return this.subarray(start, end);
};
NodeBuffer.isAscii = (value) => {
  if (value instanceof ArrayBuffer && __nodeDetachedBuffers.has(value)) {
    throw Object.assign(new TypeError("ArrayBuffer is detached"), { code: "ERR_INVALID_STATE" });
  }
  if (!(value instanceof Uint8Array)) {
    throw Object.assign(new TypeError('The "input" argument must be an instance of Uint8Array'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  return value.every((byte) => byte < 0x80);
};
