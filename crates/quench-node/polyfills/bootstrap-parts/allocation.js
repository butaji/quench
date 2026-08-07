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
    const result = __NodeBufferBase04.copyBytesFrom(view, offset, length);
    return NodeBuffer.from(result);
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
NodeBuffer.from = (value, ...args) => {
  const result = __nodeBufferFromWithAlignment.call(NodeBuffer, value, ...args);
  if (__nodeBufferIsArrayBuffer(value) || value instanceof SharedArrayBuffer) {
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
