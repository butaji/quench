const __NodeBufferBase03 = NodeBuffer;
const __nodeValidateVariableByteLength = (byteLength) => {
  if (typeof byteLength !== "number") {
    const error = new TypeError(
      'The "byteLength" argument must be of type number'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(byteLength) || byteLength < 1 || byteLength > 6) {
    const integerMessage =
      Number.isNaN(byteLength) ||
      (Number.isFinite(byteLength) && !Number.isInteger(byteLength));
    const message = integerMessage
      ? `The value of "byteLength" is out of range. It must be an integer. Received ${byteLength}`
      : `The value of "byteLength" is out of range. It must be >= 1 and <= 6. Received ${byteLength}`;
    const error = new RangeError(message);
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeValidateVariableValue = (value, min, max) => {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < min ||
    value > max
  ) {
    const bits = Math.log2(max + 1);
    const bound = bits > 32 ? `< 2 ** ${bits}` : `<= ${max}`;
    const received =
      bits > 32
        ? String(value).replace(/(\d)(?=(\d\d\d)+(?!\d))/g, "$1_")
        : value;
    const error = new RangeError(
      `The value of "value" is out of range. It must be >= ${min} and ${bound}. Received ${received}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeValidateVariableOffset = (offset, length, byteLength) => {
  if (typeof offset !== "number") {
    return NodeBuffer.prototype._integerOffset.call(
      { length },
      offset,
      byteLength
    );
  }
  if (!Number.isInteger(offset) || offset < 0 || offset + byteLength > length) {
    const message =
      Number.isNaN(offset) ||
      (Number.isFinite(offset) && !Number.isInteger(offset))
        ? `The value of "offset" is out of range. It must be an integer. Received ${offset}`
        : `The value of "offset" is out of range. It must be >= 0 and <= ${
            length - byteLength
          }. Received ${offset}`;
    const error = new RangeError(message);
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
NodeBuffer = class NodeBuffer extends __NodeBufferBase03 {
  writeUInt16BE(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      2,
      false,
      false
    );
  }

  writeUInt32LE(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      4,
      true,
      false
    );
  }

  writeUInt32BE(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      4,
      false,
      false
    );
  }

  readInt8(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(this, offset, 1, false, true);
  }

  readInt16LE(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(this, offset, 2, true, true);
  }

  readInt16BE(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(this, offset, 2, false, true);
  }

  readInt32LE(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(this, offset, 4, true, true);
  }

  readInt32BE(offset = 0) {
    return NodeBuffer.prototype._readInteger.call(this, offset, 4, false, true);
  }

  writeInt8(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      1,
      false,
      true
    );
  }

  writeInt16LE(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      2,
      true,
      true
    );
  }

  writeInt16BE(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      2,
      false,
      true
    );
  }

  writeInt32LE(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      4,
      true,
      true
    );
  }

  writeInt32BE(value, offset = 0) {
    return NodeBuffer.prototype._writeInteger.call(
      this,
      value,
      offset,
      4,
      false,
      true
    );
  }

  readFloatLE(offset = 0) {
    NodeBuffer.prototype._floatOffset.call(this, offset, 4);
    return new DataView(
      this.buffer,
      this.byteOffset,
      this.byteLength
    ).getFloat32(offset, true);
  }

  readFloatBE(offset = 0) {
    NodeBuffer.prototype._floatOffset.call(this, offset, 4);
    return new DataView(
      this.buffer,
      this.byteOffset,
      this.byteLength
    ).getFloat32(offset, false);
  }

  writeFloatLE(value, offset = 0) {
    NodeBuffer.prototype._floatOffset.call(this, offset, 4);
    new DataView(this.buffer, this.byteOffset, this.byteLength).setFloat32(
      offset,
      value,
      true
    );
    return offset + 4;
  }

  writeFloatBE(value, offset = 0) {
    NodeBuffer.prototype._floatOffset.call(this, offset, 4);
    new DataView(this.buffer, this.byteOffset, this.byteLength).setFloat32(
      offset,
      value,
      false
    );
    return offset + 4;
  }

  readUIntLE(offset, byteLength) {
    NodeBuffer.prototype._validateVariableInteger.call(
      this,
      0,
      offset,
      byteLength
    );
    let value = 0;
    for (let i = 0; i < byteLength; i++) {
      value += this[offset + i] * 2 ** (8 * i);
    }
    return value;
  }

  readUIntBE(offset, byteLength) {
    NodeBuffer.prototype._validateVariableInteger.call(
      this,
      0,
      offset,
      byteLength
    );
    let value = 0;
    for (let i = 0; i < byteLength; i++) value = value * 256 + this[offset + i];
    return value;
  }

  writeUIntLE(value, offset, byteLength) {
    NodeBuffer.prototype._validateVariableInteger.call(
      this,
      value,
      offset,
      byteLength,
      false
    );
    for (let i = 0; i < byteLength; i++) {
      this[offset + i] = value & 0xff;
      value = Math.floor(value / 256);
    }
    return offset + byteLength;
  }

  writeUIntBE(value, offset, byteLength) {
    NodeBuffer.prototype._validateVariableInteger.call(
      this,
      value,
      offset,
      byteLength,
      false
    );
    for (let i = byteLength - 1; i >= 0; i--) {
      this[offset + i] = value & 0xff;
      value = Math.floor(value / 256);
    }
    return offset + byteLength;
  }

  readIntLE(offset, byteLength) {
    const value = NodeBuffer.prototype.readUIntLE.call(
      this,
      offset,
      byteLength
    );
    const limit = 2 ** (byteLength * 8 - 1);
    return value >= limit ? value - 2 ** (byteLength * 8) : value;
  }

  readIntBE(offset, byteLength) {
    const value = NodeBuffer.prototype.readUIntBE.call(
      this,
      offset,
      byteLength
    );
    const limit = 2 ** (byteLength * 8 - 1);
    return value >= limit ? value - 2 ** (byteLength * 8) : value;
  }

  writeIntLE(value, offset, byteLength) {
    NodeBuffer.prototype._validateVariableInteger.call(
      this,
      value,
      offset,
      byteLength,
      true
    );
    const modulus = 2 ** (byteLength * 8);
    return NodeBuffer.prototype.writeUIntLE.call(
      this,
      value < 0 ? modulus + value : value,
      offset,
      byteLength
    );
  }

  writeIntBE(value, offset, byteLength) {
    NodeBuffer.prototype._validateVariableInteger.call(
      this,
      value,
      offset,
      byteLength,
      true
    );
    const modulus = 2 ** (byteLength * 8);
    return NodeBuffer.prototype.writeUIntBE.call(
      this,
      value < 0 ? modulus + value : value,
      offset,
      byteLength
    );
  }

  _validateVariableInteger(value, offset, byteLength, signed = false) {
    __nodeValidateVariableByteLength(byteLength);
    __nodeValidateVariableOffset(offset, this.length, byteLength);
    const min = signed ? -(2 ** (8 * byteLength - 1)) : 0;
    const max = signed
      ? 2 ** (8 * byteLength - 1) - 1
      : 2 ** (8 * byteLength) - 1;
    __nodeValidateVariableValue(value, min, max);
  }
};
