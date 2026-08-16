//! Polyfill: `buffer-validation`

pub const JS: &str = quench_js_check::checked_js!(r#"const __NodeBufferBase02 = NodeBuffer;
const __nodeBufferFloatRangeError = (message) => {
  const error = new RangeError(message);
  error.code = "ERR_OUT_OF_RANGE";
  return error;
};
const __nodeBufferValidateIntegerValue = (value, min, max) => {
  if (
    typeof value !== "number" ||
    !Number.isInteger(value) ||
    value < min ||
    value > max
  ) {
    throw Object.assign(new RangeError(`The value of "value" is out of range. It must be >= ${min} and <= ${max}. Received ${value}`), { code: "ERR_OUT_OF_RANGE" });
  }
};
const __nodeBufferValidateDoubleOffset = (length, offset) => {
  if (typeof offset !== "number") {
    throw Object.assign(new TypeError('The "offset" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isInteger(offset)) {
    const message =
      Number.isNaN(offset) || Number.isFinite(offset)
        ? `The value of "offset" is out of range. It must be an integer. Received ${offset}`
        : `The value of "offset" is out of range. It must be >= 0 and <= ${
            length - 8
          }. Received ${offset}`;
    throw Object.assign(new RangeError(message), { code: "ERR_OUT_OF_RANGE" });
  }
  if (offset < 0 || offset + 8 > length) {
    if (offset >= 0 && length < 8) {
      throw Object.assign(new RangeError("Attempt to access memory outside buffer bounds"), { code: "ERR_BUFFER_OUT_OF_BOUNDS" });
    }
    throw Object.assign(new RangeError(`The value of "offset" is out of range. It must be >= 0 and <= ${
        length - 8
      }. Received ${offset}`), { code: "ERR_OUT_OF_RANGE" });
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
      throw Object.assign(new TypeError('The "offset" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
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
    throw Object.assign(new TypeError(`Unknown encoding: ${encoding}`), { code: "ERR_UNKNOWN_ENCODING" });
  }
  if (
    typeof offset !== "number" ||
    !Number.isInteger(offset) ||
    offset < 0 ||
    offset > buffer.length
  ) {
    throw Object.assign(new RangeError(`The value of "offset" is out of range. It must be >= 0 && <= ${buffer.length}. Received ${offset}`), { code: "ERR_OUT_OF_RANGE" });
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
      throw Object.assign(new TypeError('The "offset" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (!Number.isInteger(offset)) {
      const message =
        Number.isNaN(offset) || Number.isFinite(offset)
          ? `The value of "offset" is out of range. It must be an integer. Received ${offset}`
          : `The value of "offset" is out of range. It must be >= 0 and <= ${
              this.length - 8
            }. Received ${offset}`;
      throw Object.assign(new RangeError(message), { code: "ERR_OUT_OF_RANGE" });
    }
    if (offset < 0 || offset + 8 > this.length) {
      if (offset >= 0 && this.length < 8) {
        throw Object.assign(new RangeError("Attempt to access memory outside buffer bounds"), { code: "ERR_BUFFER_OUT_OF_BOUNDS" });
      }
      throw Object.assign(new RangeError(`The value of "offset" is out of range. It must be >= 0 and <= ${
          this.length - 8
        }. Received ${offset}`), { code: "ERR_OUT_OF_RANGE" });
    }
    return new DataView(
      this.buffer,
      this.byteOffset,
      this.byteLength
    ).getFloat64(offset, littleEndian);
  }
  _integerOffset(offset, size) {
    if (typeof offset !== "number") {
      throw Object.assign(new TypeError('The "offset" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (!Number.isInteger(offset)) {
      const message =
        Number.isNaN(offset) || Number.isFinite(offset)
          ? `The value of "offset" is out of range. It must be an integer. Received ${offset}`
          : 'The value of "offset" is out of range';
      throw Object.assign(new RangeError(message), { code: "ERR_OUT_OF_RANGE" });
    }
    if (offset < 0) {
      throw Object.assign(new RangeError('The value of "offset" is out of range'), { code: "ERR_OUT_OF_RANGE" });
    }
    if (offset + size > this.length) {
      throw Object.assign(new RangeError("Attempt to access memory outside buffer bounds"), { code: "ERR_BUFFER_OUT_OF_BOUNDS" });
    }
    return offset;
  }
  _floatOffset(offset, size) {
    if (typeof offset !== "number") {
      throw Object.assign(new TypeError('The "offset" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (offset === Infinity || offset === -Infinity) {
      throw __nodeBufferFloatRangeError(
        `The value of "offset" is out of range. It must be >= 0 and <= ${
          this.length - size
        }. Received ${offset}`
      );
    }
    if (!Number.isInteger(offset)) {
      throw __nodeBufferFloatRangeError(
        `The value of "offset" is out of range. It must be an integer. Received ${offset}`
      );
    }
    if (offset < 0 || (this.length >= size && offset + size > this.length)) {
      throw __nodeBufferFloatRangeError(
        `The value of "offset" is out of range. It must be >= 0 and <= ${
          this.length - size
        }. Received ${offset}`
      );
    }
    if (this.length < size) {
      throw Object.assign(new RangeError("Attempt to access memory outside buffer bounds"), { code: "ERR_BUFFER_OUT_OF_BOUNDS" });
    }
    return offset;
  }
  _writeInteger(value, offset, size, littleEndian, signed) {
    NodeBuffer.prototype._integerOffset.call(this, offset, size);
    const max = signed ? 2 ** (size * 8 - 1) - 1 : 2 ** (size * 8) - 1;
    const min = signed ? -(2 ** (size * 8 - 1)) : 0;
    __nodeBufferValidateIntegerValue(value, min, max);
    let integer = signed && value < 0 ? value + 2 ** (size * 8) : value;
    for (let index = 0; index < size; index++) {
      const shift = littleEndian ? index : size - index - 1;
      this[offset + index] = Math.floor(integer / 2 ** (shift * 8)) & 0xff;
    }
    return offset + size;
  }
  _readInteger(offset, size, littleEndian, signed) {
    NodeBuffer.prototype._integerOffset.call(this, offset, size);
    let integer = 0;
    for (let index = 0; index < size; index++) {
      const shift = littleEndian ? index : size - index - 1;
      integer += this[offset + index] * 2 ** (shift * 8);
    }
    if (signed && integer >= 2 ** (size * 8 - 1)) integer -= 2 ** (size * 8);
    return integer;
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
    if (typeof this._quenchReadUInt16LE === "function") return this._quenchReadUInt16LE(offset);
    return NodeBuffer.prototype._readInteger.call(this, offset, 2, true, false);
  }
  readUInt16BE(offset = 0) {
    if (typeof this._quenchReadUInt16BE === "function") return this._quenchReadUInt16BE(offset);
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
    if (typeof this._quenchWriteUInt16LE === "function") return this._quenchWriteUInt16LE(value, offset);
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      2,
      true,
      false
    );
  }
  writeUInt16BE(value, offset = 0) {
    if (typeof this._quenchWriteUInt16BE === "function") return this._quenchWriteUInt16BE(value, offset);
    return NodeBuffer.prototype._writeInteger.call(this, value, offset, 2, false, false);
  }
};
"#);
