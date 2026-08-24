"use strict";
const assert = require("assert");
const p = Buffer.prototype;
for (const fn of ["UInt8", "UInt16LE", "UInt16BE", "UInt32LE", "UInt32BE", "UIntLE", "UIntBE", "BigUInt64LE", "BigUInt64BE"]) {
  const lower = fn.replace(/UInt/g, "Uint");
  assert.strictEqual(p[`write${fn}`], p[`write${lower}`]);
  assert.strictEqual(p[`read${fn}`], p[`read${lower}`]);
}
