const __NodeBufferBase02 = NodeBuffer;
const __nodeBufferValidateIntegerValue = (value, min, max) => {
  if (
    typeof value !== "number" ||
    !Number.isInteger(value) ||
    value < min ||
    value > max
  ) {
    const error = new RangeError(
      `The value of "value" is out of range. It must be >= ${min} and <= ${max}. Received ${value}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeBufferValidateDoubleOffset = (length, offset) => {
  if (typeof offset !== "number") {
    const error = new TypeError('The "offset" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(offset)) {
    const message =
      Number.isNaN(offset) || Number.isFinite(offset)
        ? `The value of "offset" is out of range. It must be an integer. Received ${offset}`
        : `The value of "offset" is out of range. It must be >= 0 and <= ${
            length - 8
          }. Received ${offset}`;
    const error = new RangeError(message);
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (offset < 0 || offset + 8 > length) {
    if (offset >= 0 && length < 8) {
      const error = new RangeError(
        "Attempt to access memory outside buffer bounds"
      );
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    const error = new RangeError(
      `The value of "offset" is out of range. It must be >= 0 and <= ${
        length - 8
      }. Received ${offset}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeBufferSearchNeedle = (value, encoding) =>
  typeof value === "number"
    ? new Uint8Array([value & 0xff])
    : typeof value === "string"
      ? NodeBuffer.from(value, encoding)
      : value;
const __nodeBufferSearchStart = (length, offset) => {
  let start = Number(offset);
  if (Number.isNaN(start) || start === -Infinity) start = 0;
  if (start < 0) start = Math.max(length + Math.trunc(start), 0);
  return Math.trunc(start);
};
const __nodeBufferSearchMatch = (buffer, needle, start, step) => {
  for (
    let index = start;
    index >= 0 && index + needle.length <= buffer.length;
    index += step
  ) {
    let match = true;
    for (let offset = 0; offset < needle.length; offset++) {
      if (buffer[index + offset] !== needle[offset]) match = false;
    }
    if (match) return index;
  }
  return -1;
};
const __nodeBufferIncludesValidate = (value) => {
  if (
    typeof value !== "number" &&
    typeof value !== "string" &&
    !(value instanceof Uint8Array)
  ) {
    const error = new TypeError(
      `The "value" argument must be one of type number or string or an instance of Buffer or Uint8Array.${__nodeBufferFromReceived(
        value
      )}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __nodeBufferSearchAligned = (encoding, start) =>
  ["ucs2", "ucs-2", "utf16le"].includes(encoding) && start % 2 !== 0;
const __nodeBufferWriteArguments = (offset, length, encoding) => {
  if (typeof offset === "string") {
    if (length !== undefined) {
      const error = new TypeError(
        'The "offset" argument must be of type number'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return { offset: 0, length: undefined, encoding: offset };
  }
  if (typeof length === "string") {
    return { offset, length: undefined, encoding: length };
  }
  return { offset, length, encoding };
};
const __nodeBufferWriteValidate = (buffer, offset, encoding) => {
  if (typeof encoding !== "string" || !NodeBuffer.isEncoding(encoding)) {
    const error = new TypeError(`Unknown encoding: ${encoding}`);
    error.code = "ERR_UNKNOWN_ENCODING";
    throw error;
  }
  if (
    typeof offset !== "number" ||
    !Number.isInteger(offset) ||
    offset < 0 ||
    offset > buffer.length
  ) {
    const error = new RangeError(
      `The value of "offset" is out of range. It must be >= 0 && <= ${buffer.length}. Received ${offset}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeBufferWriteUtf8Count = (value, encoding, count) => {
  if (encoding !== "utf8" && encoding !== "utf-8") return count;
  let complete = 0;
  for (let index = 1; index <= String(value).length; index++) {
    const size = NodeBuffer.from(String(value).slice(0, index), "utf8").length;
    if (size > count) break;
    complete = size;
  }
  return complete;
};
NodeBuffer = class NodeBuffer extends __NodeBufferBase02 {
  includes(value, byteOffset = 0, encoding) {
    __nodeBufferIncludesValidate(value);
    let start = Number(byteOffset);
    if (Number.isNaN(start) || start === -Infinity) start = 0;
    if (start === Infinity) {
      return (
        value === "" || (value instanceof Uint8Array && value.length === 0)
      );
    }
    start =
      start < 0
        ? Math.max(this.length + Math.trunc(start), 0)
        : Math.trunc(start);
    if (__nodeBufferSearchAligned(encoding, start)) return false;
    const needle = __nodeBufferSearchNeedle(value, encoding);
    if (needle.length === 0) return true;
    return __nodeBufferSearchMatch(this, needle, start, 1) >= 0;
  }

  indexOf(value, byteOffset = 0, encoding) {
    if (typeof byteOffset === "string") {
      encoding = byteOffset;
      byteOffset = 0;
    }
    const needle = __nodeBufferSearchNeedle(value, encoding);
    const start = __nodeBufferSearchStart(this.length, byteOffset);
    if (__nodeBufferSearchAligned(encoding, start)) return -1;
    if (start > this.length || start === Infinity) {
      return needle.length === 0 ? this.length : -1;
    }
    if (needle.length === 0) return start;
    return __nodeBufferSearchMatch(this, needle, start, 1);
  }

  lastIndexOf(value, byteOffset = this.length - 1, encoding) {
    if (typeof byteOffset === "string") {
      encoding = byteOffset;
      byteOffset = this.length - 1;
    }
    const needle = __nodeBufferSearchNeedle(value, encoding);
    let end = Number(byteOffset);
    end =
      Number.isNaN(end) || end === Infinity ? this.length - 1 : Math.trunc(end);
    if (end < 0) end = this.length + end;
    if (needle.length === 0) return Math.max(0, Math.min(end, this.length));
    return __nodeBufferSearchMatch(
      this,
      needle,
      Math.min(end, this.length - needle.length),
      -1
    );
  }

  write(value, offset = 0, length, encoding = "utf8") {
    ({ offset, length, encoding } = __nodeBufferWriteArguments(
      offset,
      length,
      encoding
    ));
    __nodeBufferWriteValidate(this, offset, encoding);
    const bytes = NodeBuffer.from(String(value), encoding);
    const requested =
      length === undefined
        ? this.length - offset
        : Math.max(0, Math.trunc(Number(length)) || 0);
    let count = Math.min(requested, this.length - offset, bytes.length);
    const normalized = String(encoding).toLowerCase();
    count = __nodeBufferWriteUtf8Count(value, normalized, count);
    if (
      (normalized === "ucs2" ||
        normalized === "ucs-2" ||
        normalized === "utf16le" ||
        normalized === "utf-16le") &&
      count % 2
    ) {
      count--;
    }
    this.set(bytes.subarray(0, count), offset);
    return count;
  }

  writeDoubleLE(value, offset = 0) {
    return NodeBuffer.prototype._writeDouble.call(this, value, offset, true);
  }

  writeDoubleBE(value, offset = 0) {
    return NodeBuffer.prototype._writeDouble.call(this, value, offset, false);
  }

  _writeDouble(value, offset, littleEndian) {
    __nodeBufferValidateDoubleOffset(this.length, offset);
    new DataView(this.buffer, this.byteOffset, this.byteLength).setFloat64(
      offset,
      Number(value),
      littleEndian
    );
    return offset + 8;
  }

  readDoubleLE(offset = 0) {
    return NodeBuffer.prototype._readDouble.call(this, offset, true);
  }

  readDoubleBE(offset = 0) {
    return NodeBuffer.prototype._readDouble.call(this, offset, false);
  }

  _readDouble(offset, littleEndian) {
    if (typeof offset !== "number") {
      const error = new TypeError(
        'The "offset" argument must be of type number'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(offset)) {
      const message =
        Number.isNaN(offset) || Number.isFinite(offset)
          ? `The value of "offset" is out of range. It must be an integer. Received ${offset}`
          : `The value of "offset" is out of range. It must be >= 0 and <= ${
              this.length - 8
            }. Received ${offset}`;
      const error = new RangeError(message);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (offset < 0 || offset + 8 > this.length) {
      if (offset >= 0 && this.length < 8) {
        const error = new RangeError(
          "Attempt to access memory outside buffer bounds"
        );
        error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
        throw error;
      }
      const error = new RangeError(
        `The value of "offset" is out of range. It must be >= 0 and <= ${
          this.length - 8
        }. Received ${offset}`
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return new DataView(
      this.buffer,
      this.byteOffset,
      this.byteLength
    ).getFloat64(offset, littleEndian);
  }

  _integerOffset(offset, size) {
    if (typeof offset !== "number") {
      const error = new TypeError(
        'The "offset" argument must be of type number'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(offset)) {
      const message =
        Number.isNaN(offset) || Number.isFinite(offset)
          ? `The value of "offset" is out of range. It must be an integer. Received ${offset}`
          : 'The value of "offset" is out of range';
      const error = new RangeError(message);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (offset < 0) {
      const error = new RangeError('The value of "offset" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (offset + size > this.length) {
      const error = new RangeError(
        "Attempt to access memory outside buffer bounds"
      );
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    return offset;
  }

  _floatOffset(offset, size) {
    if (typeof offset !== "number") {
      const error = new TypeError(
        'The "offset" argument must be of type number'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (offset === Infinity || offset === -Infinity) {
      const error = new RangeError(
        `The value of "offset" is out of range. It must be >= 0 and <= ${
          this.length - size
        }. Received ${offset}`
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isInteger(offset)) {
      const error = new RangeError(
        `The value of "offset" is out of range. It must be an integer. Received ${offset}`
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (offset < 0 || (this.length >= size && offset + size > this.length)) {
      const error = new RangeError(
        `The value of "offset" is out of range. It must be >= 0 and <= ${
          this.length - size
        }. Received ${offset}`
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (this.length < size) {
      const error = new RangeError(
        "Attempt to access memory outside buffer bounds"
      );
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    return offset;
  }

  _writeInteger(value, offset, size, littleEndian, signed) {
    NodeBuffer.prototype._integerOffset.call(this, offset, size);
    const max = signed ? 2 ** (size * 8 - 1) - 1 : 2 ** (size * 8) - 1;
    const min = signed ? -(2 ** (size * 8 - 1)) : 0;
    __nodeBufferValidateIntegerValue(value, min, max);
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    if (size === 1) view.setInt8(offset, value);
    else if (size === 2) {
      signed
        ? view.setInt16(offset, value, littleEndian)
        : view.setUint16(offset, value, littleEndian);
    } else {
      signed
        ? view.setInt32(offset, value, littleEndian)
        : view.setUint32(offset, value, littleEndian);
    }
    return offset + size;
  }

  _readInteger(offset, size, littleEndian, signed) {
    NodeBuffer.prototype._integerOffset.call(this, offset, size);
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    if (size === 1) {
      return signed ? view.getInt8(offset) : view.getUint8(offset);
    }
    if (size === 2) {
      return signed
        ? view.getInt16(offset, littleEndian)
        : view.getUint16(offset, littleEndian);
    }
    return signed
      ? view.getInt32(offset, littleEndian)
      : view.getUint32(offset, littleEndian);
  }

  readUInt8(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(
      this,
      offset,
      1,
      false,
      false
    );
  }

  readUInt16LE(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(this, offset, 2, true, false);
  }

  readUInt16BE(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(
      this,
      offset,
      2,
      false,
      false
    );
  }

  readUInt32LE(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(this, offset, 4, true, false);
  }

  readUInt32BE(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(
      this,
      offset,
      4,
      false,
      false
    );
  }

  writeUInt8(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      1,
      false,
      false
    );
  }

  writeUInt16LE(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      2,
      true,
      false
    );
  }
};
