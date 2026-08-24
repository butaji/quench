"use strict";
const assert = require("assert");
const buffer = Buffer.alloc(8);
for (const offset of ["", "0", null, {}, [], () => {}, true, false]) {
  assert.throws(() => buffer.writeInt8(23, offset), { code: "ERR_INVALID_ARG_TYPE" }, String(offset));
}
for (const offset of [NaN, Infinity, -1, 1.01]) {
  assert.throws(() => buffer.writeInt8(23, offset), { code: "ERR_OUT_OF_RANGE" }, String(offset));
}
for (const byteLength of ["", "0", null, {}, [], () => {}, true, false, undefined]) {
  assert.throws(() => buffer.writeIntBE(23, 0, byteLength), { code: "ERR_INVALID_ARG_TYPE" }, String(byteLength));
}
for (const byteLength of [NaN, 1.01, Infinity, -1]) {
  assert.throws(() => buffer.writeIntBE(23, 0, byteLength), { code: "ERR_OUT_OF_RANGE" }, String(byteLength));
}
for (let size = 1; size <= 6; size++) {
  for (const offset of ["", "0", null, {}, [], () => {}, true, false, undefined]) {
    assert.throws(() => buffer.writeIntBE(0, offset, size), { code: "ERR_INVALID_ARG_TYPE" }, `${size}:${offset}`);
  }
  for (const offset of [Infinity, -1, -4294967295, NaN, 1.01]) {
    assert.throws(() => buffer.writeIntBE(0, offset, size), { code: "ERR_OUT_OF_RANGE" }, `${size}:${offset}`);
  }
}
