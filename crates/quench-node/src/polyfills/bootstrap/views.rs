//! Polyfill: `views`

pub const JS: &str = quench_js_check::checked_js!(r#"const __NodeBufferBase03 = NodeBuffer;
const __nodeValidateVariableByteLength = (byteLength) => {
  if (typeof byteLength !== "number") {
    throw Object.assign(new TypeError('The "byteLength" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isInteger(byteLength) || byteLength < 1 || byteLength > 6) {
    const integerMessage =
      Number.isNaN(byteLength) ||
      (Number.isFinite(byteLength) && !Number.isInteger(byteLength));
    const message = integerMessage
      ? `The value of "byteLength" is out of range. It must be an integer. Received ${byteLength}`
      : `The value of "byteLength" is out of range. It must be >= 1 and <= 6. Received ${byteLength}`;
    throw Object.assign(new RangeError(message), { code: "ERR_OUT_OF_RANGE" });
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
    throw Object.assign(new RangeError(`The value of "value" is out of range. It must be >= ${min} and ${bound}. Received ${received}`), { code: "ERR_OUT_OF_RANGE" });
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
    throw Object.assign(new RangeError(message), { code: "ERR_OUT_OF_RANGE" });
  }
};
NodeBuffer = class NodeBuffer extends __NodeBufferBase03 {
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
for (const [name, write, byteLength, littleEndian, signed] of [
  ["writeUInt16BE", true, 2, false, false],
  ["writeUInt32LE", true, 4, true, false],
  ["writeUInt32BE", true, 4, false, false],
  ["readInt8", false, 1, false, true],
  ["readInt16LE", false, 2, true, true],
  ["readInt16BE", false, 2, false, true],
  ["readInt32LE", false, 4, true, true],
  ["readInt32BE", false, 4, false, true],
  ["writeInt8", true, 1, false, true],
  ["writeInt16LE", true, 2, true, true],
  ["writeInt16BE", true, 2, false, true],
  ["writeInt32LE", true, 4, true, true],
  ["writeInt32BE", true, 4, false, true]
]) {
  const method = function (value, offset = 0) {
    const hostMethod = this[`_quench${name[0].toUpperCase()}${name.slice(1)}`];
    if (typeof hostMethod === "function") {
      return write ? hostMethod(value, offset) : hostMethod(value ?? 0);
    }
    return NodeBuffer.prototype[write ? "_writeInteger" : "_readInteger"].call(
      this,
      ...(write ? [value, offset] : [value ?? 0]),
      byteLength,
      littleEndian,
      signed
    );
  };
  Object.defineProperty(method, "name", { value: name });
  Object.defineProperty(NodeBuffer.prototype, name, {
    configurable: true,
    writable: true,
    value: method
  });
}
for (const [name, write, littleEndian] of [
  ["readFloatLE", false, true],
  ["readFloatBE", false, false],
  ["writeFloatLE", true, true],
  ["writeFloatBE", true, false]
]) {
  const method = function (value, offset = 0) {
    if (!write) offset = value ?? 0;
    NodeBuffer.prototype._floatOffset.call(this, offset, 4);
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    if (write) {
      view.setFloat32(offset, value, littleEndian);
      return offset + 4;
    }
    return view.getFloat32(offset, littleEndian);
  };
  Object.defineProperty(method, "name", { value: name });
  Object.defineProperty(NodeBuffer.prototype, name, {
    configurable: true,
    writable: true,
    value: method
  });
}
NodeBuffer.prototype.readUint32BE = NodeBuffer.prototype.readUInt32BE;
NodeBuffer.prototype.readUint32LE = NodeBuffer.prototype.readUInt32LE;
NodeBuffer.prototype.writeUintLE = NodeBuffer.prototype.writeUIntLE;
"#);
