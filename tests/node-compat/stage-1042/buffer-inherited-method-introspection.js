const assert = require("assert");
const { Buffer } = require("buffer");

const names = Object.getOwnPropertyNames(Buffer.prototype);
for (const method of ["copy", "swap16", "readBigInt64LE", "writeBigInt64LE"]) {
  assert.ok(names.includes(method));
  assert.strictEqual(typeof Buffer.prototype[method], "function");
}
