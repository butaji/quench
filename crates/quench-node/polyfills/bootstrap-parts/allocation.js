const __nodeBufferAllocate = (size, fill, encoding) => {
  const length = NodeBuffer._validateSize(size);
  __nodeAllocatorCounts.zeroFilled++;
  return new NodeBuffer(length).fill(fill, 0, length, encoding);
};
const __NodeBufferBase04 = NodeBuffer;
NodeBuffer = class NodeBuffer extends __NodeBufferBase04 {
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
      const bits = signed ? 63 : 64;
      const received = String(value).replace(/(\d)(?=(\d\d\d)+(?!\d))/g, "$1_");
      const error = new RangeError(
        `The value of "value" is out of range. It must be >= ${min}n and < 2n ** ${bits}n. Received ${received}n`,
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    if (signed) view.setBigInt64(offset, value, littleEndian);
    else view.setBigUint64(offset, value, littleEndian);
    return offset + 8;
  }
  readBigInt64LE(offset = 0) {
    return this._readBigInt(offset, true, true);
  }
  readBigInt64BE(offset = 0) {
    return this._readBigInt(offset, false, true);
  }
  readBigUInt64LE(offset = 0) {
    return this._readBigInt(offset, true, false);
  }
  readBigUInt64BE(offset = 0) {
    return this._readBigInt(offset, false, false);
  }
  writeBigInt64LE(value, offset = 0) {
    return this._writeBigInt(value, offset, true, true);
  }
  writeBigInt64BE(value, offset = 0) {
    return this._writeBigInt(value, offset, false, true);
  }
  writeBigUInt64LE(value, offset = 0) {
    return this._writeBigInt(value, offset, true, false);
  }
  writeBigUInt64BE(value, offset = 0) {
    return this._writeBigInt(value, offset, false, false);
  }
  subarray(begin = 0, end = this.length) {
    const view = Uint8Array.prototype.subarray.call(this, begin, end);
    return new NodeBuffer(view.buffer, view.byteOffset, view.byteLength);
  }
  slice(begin = 0, end = this.length) {
    return this.subarray(begin, end);
  }
  static copyBytesFrom(view, offset = 0, length) {
    return __NodeBufferBase04.copyBytesFrom(view, offset, length);
  }
  static of(...values) {
    return new NodeBuffer(values);
  }
  static alloc(size, fill = 0, encoding) {
    return __nodeBufferAllocate(size, fill, encoding);
  }
  static allocUnsafe(size) {
    __nodeAllocatorCounts.uninitialized++;
    return new NodeBuffer(NodeBuffer._validateSize(size));
  }
  static allocUnsafeSlow(size) {
    __nodeAllocatorCounts.uninitialized++;
    return new NodeBuffer(NodeBuffer._validateSize(size));
  }
};
NodeBuffer.prototype[Symbol.for("nodejs.util.inspect.custom")] =
  NodeBuffer.prototype.inspect;
const __nodeBufferFromWithAlignment = NodeBuffer.from;
const __nodeBufferFromTypeError = (value) => {
  let received;
  if (value === undefined) received = "Received undefined";
  else if (value === null) received = "Received null";
  else if (typeof value === "symbol") {
    received = `Received type symbol (${String(value)})`;
  } else if (typeof value === "bigint") {
    received = `Received type bigint (${String(value)}n)`;
  } else if (typeof value === "function") received = "Received function ";
  else if (Object.getPrototypeOf(value) === null) {
    received = "Received [Object: null prototype] {}";
  } else {
    const name = value?.constructor?.name ||
      Object.prototype.toString.call(value).slice(8, -1);
    received = `Received an instance of ${name}`;
  }
  const error = new TypeError(
    "The first argument must be of type string or an instance of Buffer, " +
      "ArrayBuffer, or Array or an Array-like Object. " +
      received,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  return error;
};
const __nodeBufferFromIsPrimitiveUnsupported = (value) =>
  value == null || ["function", "symbol", "bigint"].includes(typeof value);
const __nodeBufferFromIsBinaryView = (value) =>
  typeof value === "string" ||
  __nodeBufferIsArrayBuffer(value) ||
  (typeof SharedArrayBuffer !== "undefined" &&
    value instanceof SharedArrayBuffer) ||
  value instanceof Uint8Array ||
  ArrayBuffer.isView(value) ||
  Array.isArray(value);
const __nodeBufferFromIsBufferLike = (value) =>
  (value?.type === "Buffer" && Array.isArray(value.data)) ||
  (value?.buffer && __nodeBufferIsArrayBuffer(value.buffer)) ||
  Boolean(value?.[Symbol.toPrimitive]) ||
  Object.prototype.toString.call(value) === "[object String]";
const __nodeBufferFromIsUnsupported = (value) => {
  if (__nodeBufferFromIsPrimitiveUnsupported(value)) return true;
  if (__nodeBufferFromIsBinaryView(value)) return false;
  if (__nodeBufferFromIsBufferLike(value)) return false;
  return !(value && typeof value === "object" && typeof value.length === "number");
};
const __nodeBufferFromSpecialValue = (value) => {
  if (value?.type === "Buffer" && Array.isArray(value.data)) return value.data;
  if (!ArrayBuffer.isView(value) && value?.buffer && __nodeBufferIsArrayBuffer(value.buffer)) {
    return value.buffer;
  }
  return undefined;
};
const __nodeBufferNormalizeFromValue = (value) => {
  const special = __nodeBufferFromSpecialValue(value);
  if (special !== undefined) return special;
  if (Object.prototype.toString.call(value) === "[object String]") return String(value);
  if (value?.[Symbol.toPrimitive]) return value[Symbol.toPrimitive]("string");
  if (ArrayBuffer.isView(value) && !(value instanceof Uint8Array)) return Array.from(value);
  return value;
};
NodeBuffer.from = (value, ...args) => {
  if (__nodeBufferFromIsUnsupported(value)) {
    throw __nodeBufferFromTypeError(value);
  }
  value = __nodeBufferNormalizeFromValue(value);
  const result = __nodeBufferFromWithAlignment.call(NodeBuffer, value, ...args);
  if (
    __nodeBufferIsArrayBuffer(value) ||
    (typeof SharedArrayBuffer !== "undefined" &&
      value instanceof SharedArrayBuffer)
  ) {
    Object.defineProperties(result, {
      parent: { value, configurable: true },
      offset: { value: result.byteOffset, configurable: true },
    });
  }
  if (typeof value === "string") {
    const pooled = __nodeBufferPoolFrom(result);
    if (pooled) {
      Object.setPrototypeOf(pooled, NodeBuffer.prototype);
      return pooled;
    }
  }
  if (typeof value !== "string" || result.byteOffset % 8 === 0) return result;
  const aligned = new Uint8Array(result);
  Object.setPrototypeOf(aligned, NodeBuffer.prototype);
  return aligned;
};
